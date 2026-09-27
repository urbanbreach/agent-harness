//! Joined-thread adapter: use Crossterm's parser and wake pipe without EventStream's worker.
use std::io;

use super::{filter::EventFilter, lock_internal_event_reader, poll_internal, sys::Waker};

/// Wakes a blocking poll without consuming or injecting terminal input.
#[derive(Clone, Debug)]
pub struct PollWaker(Waker);

impl PollWaker {
    /// A pending wake remains readable until the event source drains it.
    pub fn wake(&self) -> io::Result<()> {
        match self.0.wake() {
            // A full nonblocking wake pipe already guarantees readiness.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(()),
            result => result,
        }
    }
}

/// Initialize on the reader thread, then pass this handle to its owner.
/// Do not mix this reader with EventStream or another poll/read thread.
pub fn poll_waker() -> io::Result<PollWaker> {
    lock_internal_event_reader().try_waker().map(PollWaker)
}

/// Wait for an event or a wake. `Ok(false)` means the wake pipe was signalled.
/// After `Ok(true)`, the ordinary `read` function returns the queued event.
pub fn poll_blocking() -> io::Result<bool> {
    poll_internal(None, &EventFilter)
}
