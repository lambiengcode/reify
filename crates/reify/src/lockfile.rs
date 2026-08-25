//! An advisory lock around indexing.
//!
//! Indexing is a long operation made of many transactions, so SQLite's own locking is
//! the wrong granularity: two concurrent runs do not corrupt the store, but they do
//! interleave into a race whose only user-visible symptom is
//! `database is locked: Error code 5` — a message that tells a developer nothing about
//! what went wrong or what to do.
//!
//! A lock file turns that into a sentence. It also survives a killed process, because
//! a lock naming a pid that no longer exists is recognised as stale and reclaimed
//! rather than blocking the repository forever.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

/// Lock file name, relative to `.reify/`.
pub const LOCK_FILE: &str = "index.lock";

/// A held lock. Released on drop, including on panic.
#[derive(Debug)]
pub struct IndexLock {
    path: PathBuf,
    /// False when the lock was reclaimed as stale, which the caller reports.
    reclaimed_stale: bool,
}

impl IndexLock {
    /// Take the lock, or explain who holds it.
    ///
    /// Uses `create_new` so acquisition is a single atomic filesystem operation; two
    /// processes racing cannot both win.
    pub fn acquire(reify_dir: &Path) -> Result<IndexLock> {
        std::fs::create_dir_all(reify_dir)
            .with_context(|| format!("creating {}", reify_dir.display()))?;
        let path = reify_dir.join(LOCK_FILE);
        let mut reclaimed_stale = false;

        for attempt in 0..2 {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = write!(file, "{}", std::process::id());
                    return Ok(IndexLock {
                        path,
                        reclaimed_stale,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt == 0 => {
                    let holder = std::fs::read_to_string(&path).unwrap_or_default();
                    let pid: Option<u32> = holder.trim().parse().ok();
                    if pid.is_some_and(process_is_alive) {
                        return Err(anyhow!(
                            "another `reify index` is already running in this repository \
                             (process {}). Wait for it to finish, or remove {} if you are \
                             sure it is gone.",
                            holder.trim(),
                            path.display()
                        ));
                    }
                    // The holder is gone: a killed or crashed run. Reclaim it.
                    let _ = std::fs::remove_file(&path);
                    reclaimed_stale = true;
                }
                Err(e) => {
                    return Err(anyhow!(
                        "could not take the index lock at {}: {e}",
                        path.display()
                    ))
                }
            }
        }
        Err(anyhow!(
            "could not take the index lock at {}",
            path.display()
        ))
    }

    /// Whether this lock was taken over from a process that died mid-index.
    ///
    /// Worth surfacing: the store may hold a partial index, so the caller should say
    /// that a previous run did not finish.
    pub fn reclaimed_stale(&self) -> bool {
        self.reclaimed_stale
    }
}

impl Drop for IndexLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Is a process with this id running?
///
/// The lock is only as good as this answer. A liveness check that always says "no"
/// does not err on the safe side — it makes every lock look stale, so the lock stops
/// excluding anything and two indexers write the same store. That is what the
/// `not(unix)` stub used to do, and CI on Windows found it by failing to recognise
/// its own process as alive.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    // SAFETY: `kill` with signal 0 performs no action; it only reports whether the
    // process exists and is signallable.
    unsafe { libc_kill(pid as i32, 0) == 0 }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

/// Win32's answer to `kill(pid, 0)`.
///
/// Declared by hand rather than pulling in a Windows crate, for one question asked
/// once — the same reason `kill` is declared above rather than taking a libc
/// dependency.
#[cfg(windows)]
mod win32 {
    pub type Handle = *mut core::ffi::c_void;
    extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        pub fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
        pub fn CloseHandle(handle: Handle) -> i32;
    }
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    /// The narrowest right to ask "does this exist"; granted across integrity levels
    /// where `PROCESS_QUERY_INFORMATION` is not.
    const QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// Required to wait on the handle at all. Omitting it does not make the wait
    /// stricter — it makes it fail with `WAIT_FAILED`, which reads as "not running"
    /// and silently restores the bug this function exists to fix.
    const SYNCHRONIZE: u32 = 0x0010_0000;
    /// The handle is not signalled, so the process has not exited.
    const WAIT_TIMEOUT: u32 = 258;

    // SAFETY: `OpenProcess` returns null rather than an invalid handle on failure, and
    // the handle is closed on every path that obtained one.
    unsafe {
        let handle = win32::OpenProcess(SYNCHRONIZE | QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            // No such process, or one this user may not query. Either way, treating
            // the lock as reclaimable is the behaviour a dead owner should get.
            return false;
        }
        // Waiting zero milliseconds asks the question without blocking. Preferred over
        // `GetExitCodeProcess`, which reports the sentinel 259 for a running process
        // and cannot distinguish it from one that genuinely exited with 259.
        let state = win32::WaitForSingleObject(handle, 0);
        win32::CloseHandle(handle);
        state == WAIT_TIMEOUT
    }
}

/// Any other platform. Deliberately pessimistic: without a liveness check the lock
/// cannot be trusted, so it refuses to reclaim rather than silently allowing two
/// indexers to share a store.
#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reify-lock-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_lock_is_exclusive_while_held() {
        let dir = temp("exclusive");
        let held = IndexLock::acquire(&dir).unwrap();
        let err = IndexLock::acquire(&dir).unwrap_err().to_string();
        assert!(err.contains("already running"), "{err}");
        assert!(err.contains(&std::process::id().to_string()));
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_is_released_on_drop() {
        let dir = temp("release");
        drop(IndexLock::acquire(&dir).unwrap());
        assert!(IndexLock::acquire(&dir).is_ok(), "the lock must not leak");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_lock_left_by_a_dead_process_is_reclaimed() {
        // A killed index must not block the repository forever.
        let dir = temp("stale");
        // Pid 0 is never a normal user process, so it stands in for a dead holder.
        std::fs::write(dir.join(LOCK_FILE), "999999999").unwrap();
        let lock = IndexLock::acquire(&dir).expect("a stale lock must be reclaimable");
        assert!(lock.reclaimed_stale(), "and the caller must be told");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_lock_file_is_reclaimed_rather_than_fatal() {
        let dir = temp("corrupt");
        std::fs::write(dir.join(LOCK_FILE), "not a pid").unwrap();
        assert!(IndexLock::acquire(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_current_process_is_recognised_as_alive() {
        assert!(process_is_alive(std::process::id()));
    }
}
