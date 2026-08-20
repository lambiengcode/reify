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
/// `kill(pid, 0)` is the portable POSIX existence check. On other platforms this
/// returns `false`, which errs toward reclaiming a lock rather than deadlocking a
/// repository — the safer failure for an advisory lock.
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

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    false
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
