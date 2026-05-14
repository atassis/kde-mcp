use fd_lock::RwLock;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("io error opening lockfile {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("lock already held by pid {pid}")]
    AlreadyHeld { pid: u32 },
}

/// Single-session lockfile per mode (spec §10 layer 3).
///
/// Two layers of defense:
///
/// 1. **In-process singleton check** via a process-global `HashSet<String>`
///    of acquired modes. Prevents same-process double-acquire which POSIX
///    `flock(2)` (advisory, per-open-file-description) is permissive about
///    on Linux.
/// 2. **Cross-process flock** via `fd_lock::RwLock`. Prevents two separate
///    `kde-mcp` binaries from claiming the same mode simultaneously.
///
/// The flock-protected file also carries the holder's PID so the loser of a
/// contention can report who's blocking.
pub struct ModeLock {
    _guard: fd_lock::RwLockWriteGuard<'static, File>,
    path: PathBuf,
    mode: String,
}

impl std::fmt::Debug for ModeLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModeLock")
            .field("mode", &self.mode)
            .field("path", &self.path)
            .field("holder_pid", &std::process::id())
            .finish_non_exhaustive()
    }
}

fn modes_held() -> &'static Mutex<HashSet<String>> {
    static MODES_HELD: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    MODES_HELD.get_or_init(|| Mutex::new(HashSet::new()))
}

impl ModeLock {
    pub fn path_for(mode: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("kde-mcp").join(format!("{mode}.lock"))
    }

    pub fn acquire(mode: &str) -> Result<Self, LockError> {
        // Layer 1: in-process check.
        {
            let mut held = modes_held().lock().expect("modes_held poisoned");
            if held.contains(mode) {
                return Err(LockError::AlreadyHeld {
                    pid: std::process::id(),
                });
            }
            held.insert(mode.to_string());
        }

        // Layer 2: filesystem flock.
        // If anything in this block fails we must remove our entry from the
        // in-process set before returning, otherwise a retry in this process
        // can't succeed.
        let res = Self::acquire_inner(mode);
        if res.is_err() {
            modes_held()
                .lock()
                .expect("modes_held poisoned")
                .remove(mode);
        }
        res
    }

    fn acquire_inner(mode: &str) -> Result<Self, LockError> {
        let path = Self::path_for(mode);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LockError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| LockError::Io {
                path: path.clone(),
                source,
            })?;

        let lock: &'static mut RwLock<File> = Box::leak(Box::new(RwLock::new(file)));

        match lock.try_write() {
            Ok(mut guard) => {
                guard.seek(SeekFrom::Start(0)).map_err(|source| LockError::Io {
                    path: path.clone(),
                    source,
                })?;
                guard.set_len(0).map_err(|source| LockError::Io {
                    path: path.clone(),
                    source,
                })?;
                write!(guard, "{}", std::process::id()).map_err(|source| LockError::Io {
                    path: path.clone(),
                    source,
                })?;
                guard.flush().map_err(|source| LockError::Io {
                    path: path.clone(),
                    source,
                })?;
                Ok(Self {
                    _guard: guard,
                    path,
                    mode: mode.to_string(),
                })
            }
            Err(_) => {
                let pid = read_holder_pid(&path).unwrap_or(0);
                Err(LockError::AlreadyHeld { pid })
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ModeLock {
    fn drop(&mut self) {
        if let Ok(mut held) = modes_held().lock() {
            held.remove(&self.mode);
        }
    }
}

fn read_holder_pid(path: &Path) -> std::io::Result<u32> {
    let mut f = File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    Ok(buf.trim().parse().unwrap_or(0))
}

#[cfg(unix)]
#[allow(dead_code)]
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // signal 0 = liveness probe per POSIX. Returns 0 if the signal *would*
    // be delivered (process alive); -1 with EPERM means the process exists
    // but we lack permission to signal it (still alive).
    let r = unsafe { libc::kill(pid as i32, 0) };
    if r == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
#[allow(dead_code)]
pub fn pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lives_under_xdg_runtime_dir() {
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", dir.path()) };
        let p = ModeLock::path_for("live");
        assert!(p.starts_with(dir.path()));
        assert!(p.ends_with("kde-mcp/live.lock"));
    }

    #[test]
    fn pid_alive_returns_true_for_self_and_false_for_zero() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }
}
