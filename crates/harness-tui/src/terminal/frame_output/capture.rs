use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default)]
pub(super) struct CaptureState {
    pub(super) active: bool,
    pub(super) bytes: Vec<u8>,
    pub(super) write_calls: u64,
}

pub(super) fn capture_lock(state: &Mutex<CaptureState>) -> MutexGuard<'_, CaptureState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) fn synchronized_bytes(
    payload: Vec<u8>,
    begin_marker: &[u8],
    end_marker: &[u8],
) -> io::Result<Vec<u8>> {
    let capacity = begin_marker
        .len()
        .checked_add(payload.len())
        .and_then(|len| len.checked_add(end_marker.len()))
        .ok_or_else(|| io::Error::other("terminal frame size overflow"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| io::Error::other("terminal frame allocation failed"))?;
    bytes.extend_from_slice(begin_marker);
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(end_marker);
    Ok(bytes)
}

#[derive(Clone, Debug)]
pub struct FrameOutputWriter {
    pub(super) capture: Arc<Mutex<CaptureState>>,
}

impl Write for FrameOutputWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut capture = capture_lock(&self.capture);
        if !capture.active {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal command emitted outside an active frame",
            ));
        }
        capture
            .bytes
            .try_reserve(bytes.len())
            .map_err(|_| io::Error::other("terminal frame allocation failed"))?;
        capture.bytes.extend_from_slice(bytes);
        capture.write_calls = capture.write_calls.saturating_add(1);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
