//! System tray indicator + abort menu via `org.kde.StatusNotifierItem`.
//!
//! The tray icon appears in KDE's system tray (and any compatible
//! freedesktop tray host). It's the persistent "Claude is doing something"
//! indicator — colour changes to active when a tool is in flight, tooltip
//! shows the tool name. Right-click menu has "Abort current operation" and
//! "Quit".
//!
//! State is shared across the dispatch wrapper, the tray UI thread, and
//! the cancel-hotkey listener through [`TrayState`]. Updates from any
//! thread show up in the tray within ~100ms (ksni's update rate).

use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Tray, TrayMethods};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;

/// Shared mutable state visible to: dispatch wrapper, tray UI, hotkey
/// listener. All fields are atomic so updates don't need an async lock.
pub struct TrayState {
    /// Number of tool calls currently in flight (usually 0 or 1; can be
    /// >1 if the agent issues concurrent calls).
    pub in_flight: AtomicUsize,
    /// Wire name of the most recently started tool (best-effort; under
    /// concurrency this is racy but only used for display).
    pub current_tool: parking_lot::Mutex<Option<String>>,
    /// Cancel token shared with the policy gate. Tray "Abort" and the
    /// KGlobalAccel hotkey both flip this.
    pub cancel: CancellationToken,
    /// Set to true on `quit`. `main` polls this to drop the rmcp service.
    pub quit_requested: AtomicBool,
    /// Daemon mode (ADR-0012): number of concurrent MCP sessions
    /// currently connected. Stays 0 in stdio mode.
    pub sessions: AtomicUsize,
}

impl TrayState {
    pub fn new(cancel: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            in_flight: AtomicUsize::new(0),
            current_tool: parking_lot::Mutex::new(None),
            cancel,
            quit_requested: AtomicBool::new(false),
            sessions: AtomicUsize::new(0),
        })
    }

    pub fn enter_tool(&self, tool: &str) {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        *self.current_tool.lock() = Some(tool.to_string());
    }

    pub fn leave_tool(&self) {
        let prev = self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if prev <= 1 {
            *self.current_tool.lock() = None;
        }
    }

    pub fn abort(&self) {
        self.cancel.cancel();
    }

    pub fn is_quitting(&self) -> bool {
        self.quit_requested.load(Ordering::Relaxed)
    }
}

struct KdeMcpTray {
    state: Arc<TrayState>,
}

impl Tray for KdeMcpTray {
    fn id(&self) -> String {
        "kde-mcp".into()
    }

    fn title(&self) -> String {
        "kde-mcp".into()
    }

    fn icon_name(&self) -> String {
        // Themed icons that ship with KDE:
        //   - "media-playback-pause-symbolic" when idle (subtle)
        //   - "media-playback-start-symbolic" when active
        // Picking common Plasma-shipped names so any user theme renders
        // something sensible. Phase 6 polish could ship a custom .svg.
        if self.state.in_flight.load(Ordering::Relaxed) > 0 {
            "media-playback-start-symbolic".into()
        } else {
            "preferences-desktop".into()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        Vec::new()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let in_flight = self.state.in_flight.load(Ordering::Relaxed);
        let sessions = self.state.sessions.load(Ordering::Relaxed);
        let current = self.state.current_tool.lock().clone();
        let title = match (in_flight, sessions) {
            (0, 0) => "kde-mcp — idle".into(),
            (0, n) => format!("kde-mcp — idle, {n} session(s)"),
            (k, 0) => format!(
                "kde-mcp — running {} ({k} in flight)",
                current.as_deref().unwrap_or("(tool)")
            ),
            (k, n) => format!(
                "kde-mcp — running {} ({k} in flight, {n} session(s))",
                current.as_deref().unwrap_or("(tool)")
            ),
        };
        ksni::ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: Vec::new(),
            title,
            description: "Right-click to abort or quit. Meta+Esc also aborts.".into(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click is informational only; we don't have a main window
        // to raise. The tooltip auto-refreshes via the periodic poll loop.
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let in_flight = self.state.in_flight.load(Ordering::Relaxed);
        vec![
            StandardItem {
                label: if in_flight > 0 {
                    format!("Abort {in_flight} in-flight call(s)")
                } else {
                    "Abort (no in-flight calls)".into()
                },
                icon_name: "process-stop".into(),
                enabled: in_flight > 0,
                activate: Box::new(|t: &mut Self| t.state.abort()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit kde-mcp".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| {
                    t.state.abort();
                    t.state.quit_requested.store(true, Ordering::Relaxed);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// RAII guard: increments `in_flight` on construction, decrements on drop.
/// Safe across panics — the gate's `?`-propagation and the dispatch
/// future's `await`-cancellation both still trigger Drop.
pub struct TrayGuard {
    state: Arc<TrayState>,
}

impl TrayGuard {
    pub fn enter(state: &Arc<TrayState>, tool: &str) -> Self {
        state.enter_tool(tool);
        Self {
            state: state.clone(),
        }
    }
}

impl Drop for TrayGuard {
    fn drop(&mut self) {
        self.state.leave_tool();
    }
}

/// Spawn the tray on a dedicated OS thread that owns its own current-thread
/// tokio runtime. ksni 0.3 only exposes an async `spawn`; running it on a
/// background thread keeps it independent of the rmcp server's runtime
/// (so a tray bus disconnect can't stall tool dispatch and vice-versa).
pub fn spawn(state: Arc<TrayState>) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::warn!(error=%e, "tray: tokio runtime build failed — no tray");
                return;
            }
        };
        rt.block_on(async move {
            let tray = KdeMcpTray {
                state: state.clone(),
            };
            let handle = match tray.spawn().await {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!(error=%e, "tray: spawn failed — continuing without tray icon");
                    return;
                }
            };
            // Drive tooltip updates while the server is alive. ksni
            // re-renders on `handle.update()`.
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if state.is_quitting() {
                    let _ = handle.shutdown().await;
                    return;
                }
                let _ = handle.update(|_| {}).await;
            }
        });
    });
}
