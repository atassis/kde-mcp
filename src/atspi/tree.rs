//! AT-SPI tree traversal: flatten a subtree into `FlatElement` records.
//!
//! BFS walk starts at a root `AccessibleProxy`. For each node we fetch role,
//! name, value (best-effort via the Text proxy, since plain `Accessible` has
//! no value), states, bbox (Component), and actions (Action). Children are
//! discovered via `child_count()` + `get_child_at_index(i)`.
//!
//! All AT-SPI calls are wrapped in `.ok()` for non-essential fields — a node
//! whose Component interface fails just gets `bbox: None`. The walk continues
//! rather than aborting on per-node errors, so a single misbehaving app
//! doesn't sink the whole `inspect` response.

use crate::atspi::ref_encoding::encode_element_ref;
use crate::error::ToolError;
use crate::refs::{RefKind, RefStore};
use crate::trust::{Tagged, Trust};
use atspi::AccessibilityConnection;
use atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use atspi::proxy::proxy_ext::ProxyExt;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct FlatElement {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub child_count: u32,
    pub role: Tagged<String>,
    pub name: Tagged<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Tagged<String>>,
    pub states: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<[i32; 4]>,
    pub actions: Vec<String>,
}

pub struct WalkOpts {
    pub max_depth: u32,
    pub max_elements: usize,
    pub role_filter: Option<Vec<String>>,
    pub name_pattern: Option<regex_lite::Regex>,
}

/// Walk a subtree rooted at `root` and return flattened elements plus
/// truncation / warning metadata.
///
/// `parent_id` on the root element is `None`. Each visited node mints a ref
/// in `refs` keyed by the encoded `(bus_name|object_path)` payload, so action
/// tools in later phases can resolve a flat-list `id` back to a proxy.
pub async fn walk(
    conn: &AccessibilityConnection,
    root: AccessibleProxy<'_>,
    refs: Arc<RefStore>,
    opts: WalkOpts,
) -> Result<(Vec<FlatElement>, bool, Vec<String>), ToolError> {
    let zbus_conn = conn.connection();
    let mut out: Vec<FlatElement> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Queue holds (proxy_to_visit, depth, parent_ref_id).
    // The first entry is the root; parent_id stays None.
    let mut queue: VecDeque<(AccessibleProxy<'_>, u32, Option<String>)> = VecDeque::new();
    queue.push_back((root, 0, None));

    let mut truncated = false;

    while let Some((proxy, depth, parent_ref)) = queue.pop_front() {
        if out.len() >= opts.max_elements {
            truncated = true;
            break;
        }

        // Capture identity *before* any fallible calls so we can mint a ref
        // even if metadata fetches fail.
        let bus = proxy.inner().destination().to_string();
        let path = proxy.inner().path().to_string();
        let payload = encode_element_ref(&bus, &path);
        let id = refs.mint(RefKind::Element, payload).await;

        // Role / name / states are core; if they fail we skip the node but
        // continue the walk.
        let role_str = match proxy.get_role().await {
            Ok(r) => r.name().to_string(),
            Err(e) => {
                warnings.push(format!("get_role failed at {bus}{path}: {e}"));
                continue;
            }
        };
        let name_str = proxy.name().await.unwrap_or_default();
        let states_vec: Vec<String> = match proxy.get_state().await {
            Ok(set) => set.iter().map(|s| s.to_static_str().to_string()).collect(),
            Err(_) => Vec::new(),
        };
        let child_count = proxy.child_count().await.unwrap_or(0).max(0) as u32;

        // Filters — apply *after* role/name lookup but before peripherals.
        let passes_role = match &opts.role_filter {
            Some(allow) => allow.iter().any(|r| r.eq_ignore_ascii_case(&role_str)),
            None => true,
        };
        let passes_name = match &opts.name_pattern {
            Some(re) => re.is_match(&name_str),
            None => true,
        };
        let keep = passes_role && passes_name;

        let (bbox, actions, value) = if keep {
            element_peripherals(&proxy).await
        } else {
            (None, Vec::new(), None)
        };

        if keep {
            out.push(FlatElement {
                id: id.clone(),
                parent_id: parent_ref.clone(),
                child_count,
                role: Tagged {
                    trust: Trust::Trusted,
                    text: role_str,
                },
                name: Tagged {
                    trust: Trust::External,
                    text: name_str,
                },
                value,
                states: states_vec,
                bbox,
                actions,
            });
        }

        if depth + 1 > opts.max_depth {
            continue;
        }

        // Enqueue children. We pass the current ref as parent regardless of
        // whether *this* node was kept by the filter — otherwise filtered
        // intermediate nodes break the parent_id chain awkwardly. The
        // tradeoff: a filtered child's parent_id may point at an `id` that
        // isn't in the response. Document this in the response shape.
        for i in 0..child_count as i32 {
            let child_ref = match proxy.get_child_at_index(i).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let child_proxy = match child_ref.into_accessible_proxy(zbus_conn).await {
                Ok(p) => p,
                Err(_) => continue,
            };
            queue.push_back((child_proxy, depth + 1, Some(id.clone())));
        }
    }

    if out.is_empty() && warnings.is_empty() {
        warnings.push(
            "AT-SPI tree empty — target app may have a11y disabled (Electron/Chromium need --force-renderer-accessibility); vision fallback recommended"
                .to_string(),
        );
    }

    Ok((out, truncated, warnings))
}

/// Best-effort: fetch component (bbox), action names, and a textual value
/// via the Text proxy. All failures collapse to `None`/empty — the walk
/// must not abort if peripherals are unavailable.
async fn element_peripherals(
    proxy: &AccessibleProxy<'_>,
) -> (Option<[i32; 4]>, Vec<String>, Option<Tagged<String>>) {
    let proxies = match proxy.proxies().await {
        Ok(p) => p,
        Err(_) => return (None, Vec::new(), None),
    };

    // bbox — Component.get_extents(CoordType::Screen).
    let bbox = match proxies.component().await {
        Ok(c) => match c.get_extents(atspi::CoordType::Screen).await {
            Ok((x, y, w, h)) => Some([x, y, w, h]),
            Err(_) => None,
        },
        Err(_) => None,
    };

    // actions — Action.get_actions() → Vec<Action>; we surface only the names.
    let actions: Vec<String> = match proxies.action().await {
        Ok(a) => match a.get_actions().await {
            Ok(list) => list.into_iter().map(|act| act.name).collect(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    // value — try Text proxy's get_text(0, end). Most editable widgets expose
    // this. Numeric Value proxy returns a different type; ignored in Phase 1.
    let value: Option<Tagged<String>> = match proxies.text().await {
        Ok(t) => match t.character_count().await {
            Ok(n) if n > 0 => match t.get_text(0, n).await {
                Ok(s) if !s.is_empty() => Some(Tagged {
                    trust: Trust::External,
                    text: s,
                }),
                _ => None,
            },
            _ => None,
        },
        Err(_) => None,
    };

    (bbox, actions, value)
}
