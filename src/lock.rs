use fd_lock::RwLock;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

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
/// Acquired at startup, released when this value is dropped. The PID of the
/// holder is written into the file so other instances can report it.
///
/// The fd_lock::RwLock is leaked to obtain a `'static` lifetime for the held
/// guard — this is a documented pattern for process-lifetime singletons. The
/// memory comes back when the process exits.
pub struct ModeLock {
    _guard: fd_lock::RwLockWriteGuard<'static, File>,
    path: PathBuf,
}

impl std::fmt::Debug for ModeLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModeLock")
            .field("path", &self.path)
            .field("holder_pid", &std::process::id())
            .finish_non_exhaustive()
    }
}

impl ModeLock {
    pub fn path_for(mode: &str) -> PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("kde-mcp").join(format!("{mode}.lock"))
    }

    /// Acquire the lock for `mode`. On contention, reads the holder PID from
    /// the file and returns `AlreadyHeld`.
    ///
    /// Stale-lock detection: if the holder PID is no longer alive, we still
    /// surface `AlreadyHeld` (with `holder_pid: 0` if the file is empty or
    /// unparsable) — auto-reclaim would race with a freshly-restarting
    /// holder. The caller (main.rs) can choose to retry; for Phase 0 we
    /// keep it simple.
    pub fn acquire(mode: &str) -> Result<Self, LockError> {
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
    // signal 0 = liveness probe per POSIX. Returns 0 if signal would be
    // delivered; -1 with EPERM means the process exists but we lack
    // permission to signal it (still alive).
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
        // SAFETY: tests in this binary run on the same thread; env mutation
        // is observable cross-test, so each test below uses a unique mode
        // name to dodge interference.
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
