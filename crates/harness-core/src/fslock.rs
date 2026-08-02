//! Cross-process advisory file lock for durable read-modify-write stores.
//!
//! Built on atomic exclusive-create (O_EXCL via `create_new`), the same
//! primitive the event-store writer lock relies on. Stale locks left behind by
//! dead processes are recovered through pid liveness checks. The bounded
//! acquire wait serializes concurrent CLI processes (and threads) so neither
//! loses an update on the queue or team-mailbox journals.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static LOCK_TOKEN: AtomicU64 = AtomicU64::new(1);
const MAX_WAIT: Duration = Duration::from_secs(10);
const BACKOFF: Duration = Duration::from_millis(5);

/// RAII cross-process file lock; the lock file is removed on drop.
#[derive(Debug)]
pub(crate) struct FileLock {
    path: PathBuf,
    contents: String,
    _file: File,
}

impl FileLock {
    /// Acquire the lock file at `path` exclusively, waiting up to a bounded
    /// deadline for any concurrent holder to release. Recovers stale locks left
    /// by dead processes before retrying.
    pub(crate) fn acquire(path: impl Into<PathBuf>) -> io::Result<FileLock> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let started = Instant::now();
        loop {
            match try_create(&path) {
                Ok((file, contents)) => {
                    return Ok(Self {
                        path,
                        contents,
                        _file: file,
                    })
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if stale_lock(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() >= MAX_WAIT {
                        return Err(err);
                    }
                    thread::sleep(BACKOFF);
                }
                Err(err) => return Err(err),
            }
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        if fs::read_to_string(&self.path).is_ok_and(|contents| contents == self.contents) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn try_create(path: &Path) -> io::Result<(File, String)> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    let token = LOCK_TOKEN.fetch_add(1, Ordering::Relaxed);
    let contents = format!("pid={}\ntoken={token}\n", std::process::id());
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok((file, contents))
}

fn stale_lock(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };
    // Empty or unparseable contents mean a holder is mid-create (the lock file
    // exists but its `pid=` line is unwritten, or was torn by a crash). Treat
    // that as actively held so a waiter backs off instead of yanking a live
    // lock; only a parseable pid belonging to a dead process is reclaimable.
    let Some(pid) = contents.lines().find_map(|line| {
        line.strip_prefix("pid=")
            .and_then(|p| p.parse::<u32>().ok())
    }) else {
        return false;
    };
    !process_exists(pid)
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(not(target_os = "linux"))]
fn process_exists(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn second_acquire_blocks_until_first_drops() {
        // arrange — a lock path inside a temp dir
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json.lock");

        // act — first holder keeps the lock, a second thread waits, then the
        // first drops and the second acquires
        let first = FileLock::acquire(&path).expect("first acquire");
        let waiter_path = path.clone();
        let waiter =
            thread::spawn(move || FileLock::acquire(&waiter_path).expect("second acquire"));
        thread::sleep(BACKOFF * 4);
        assert!(
            first.path.exists(),
            "lock file must exist while first holds it"
        );
        drop(first);

        // assert — the waiter acquires only after release and cleans up on drop
        let second = waiter.join().expect("waiter joins");
        assert!(second.path.exists());
        drop(second);
        assert!(!path.exists(), "lock file removed after final drop");
    }

    #[test]
    fn stale_lock_from_dead_process_is_recovered() {
        // arrange — a lock file owned by a pid that cannot exist
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json.lock");
        fs::write(&path, "pid=4294967295\ntoken=99\n").unwrap();

        // act — acquire must recover the stale lock instead of failing
        let lock = FileLock::acquire(&path).expect("recover stale lock");

        // assert — the holder rewrote the lock with the live pid
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains(&format!("pid={}", std::process::id())),
            "lock contents must reflect the live holder: {body}"
        );
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn concurrent_acquirers_serialize_without_loss() {
        // arrange — many threads increment a shared counter under the lock
        let dir = tempdir().unwrap();
        let lock_path = Arc::new(dir.path().join("counter.lock"));
        let counter = Arc::new(std::sync::Mutex::new(0u64));

        // act
        let mut handles = Vec::new();
        for _ in 0..16 {
            let lock_path = Arc::clone(&lock_path);
            let counter = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                let _lock = FileLock::acquire(&*lock_path).expect("acquire under contention");
                let value = *counter.lock().unwrap();
                *counter.lock().unwrap() = value + 1;
            }));
        }
        for handle in handles {
            handle.join().expect("worker joins");
        }

        // assert — every critical section ran exactly once
        assert_eq!(*counter.lock().unwrap(), 16);
    }
}
