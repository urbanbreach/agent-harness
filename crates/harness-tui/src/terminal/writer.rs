//! Synchronized-output writer: brackets a frame with DEC private mode 2026.
//!
//! Emits the exact byte sequences a compliant terminal recognizes for
//! synchronized updates (`CSI ? 2026 h` to begin, `CSI ? 2026 l` to end). The
//! sink is in-memory so the produced bytes are inspectable in contract tests.
//! Begin is emitted lazily on the first non-empty payload write, so an empty
//! frame (begin + end with no visible content) elides the markers entirely.

use std::io::{self, Write};

/// Begin synchronized update: DECSET private mode 2026.
pub const BEGIN_SYNCHRONIZED_UPDATE: &[u8] = b"\x1b[?2026h";
/// End synchronized update: DECRST private mode 2026.
pub const END_SYNCHRONIZED_UPDATE: &[u8] = b"\x1b[?2026l";

/// A writer that brackets a frame's payload in synchronized-update markers.
///
/// Nested `begin_frame` / `end_frame` calls only affect [`Self::depth`]; a
/// single marker pair wraps the outer frame. Payload written with no open
/// frame is raw pass-through.
#[derive(Debug, Default)]
pub struct SynchronizedWriter<W> {
    sink: W,
    depth: u32,
    begin_emitted: bool,
    bytes_written: usize,
}

impl<W: Write> SynchronizedWriter<W> {
    /// Wrap a sink with no active frame.
    pub fn new(sink: W) -> Self {
        Self {
            sink,
            depth: 0,
            begin_emitted: false,
            bytes_written: 0,
        }
    }

    /// Frames currently open (nested `begin_frame` calls increase depth).
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Whether the outer synchronized-update begin marker has been emitted for
    /// the current frame.
    pub const fn begin_emitted(&self) -> bool {
        self.begin_emitted
    }

    /// Total bytes written to the sink, including any emitted markers.
    pub const fn bytes_written(&self) -> usize {
        self.bytes_written
    }

    /// Open a frame. Nested calls only increase [`Self::depth`].
    pub fn begin_frame(&mut self) -> io::Result<()> {
        self.depth = self
            .depth
            .checked_add(1)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "frame depth overflow"))?;
        Ok(())
    }

    /// Close a frame; emits the end marker only when a begin marker was
    /// emitted.
    ///
    /// # Errors
    /// Returns [`io::ErrorKind::InvalidInput`] when called with no open frame.
    pub fn end_frame(&mut self) -> io::Result<()> {
        let Some(outer) = self.depth.checked_sub(1) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "end_frame called with no open frame",
            ));
        };
        self.depth = outer;
        if outer == 0 && self.begin_emitted {
            self.write_raw(END_SYNCHRONIZED_UPDATE)?;
            self.begin_emitted = false;
        }
        Ok(())
    }

    /// Open a frame for the lifetime of the returned guard (RAII). The guard
    /// calls [`Self::end_frame`] on drop.
    pub fn frame(&mut self) -> io::Result<SyncFrameGuard<'_, W>> {
        self.begin_frame()?;
        Ok(SyncFrameGuard { writer: self })
    }

    /// Write frame payload (or raw bytes outside a frame). When a frame is
    /// open and no begin marker has been emitted yet, the marker is emitted
    /// first. A zero-length write never triggers the begin marker.
    pub fn write_payload(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.depth > 0 && !self.begin_emitted && !bytes.is_empty() {
            self.write_raw(BEGIN_SYNCHRONIZED_UPDATE)?;
            self.begin_emitted = true;
        }
        self.write_raw(bytes)
    }

    /// Consume the writer, returning the underlying sink.
    pub fn into_inner(self) -> W {
        self.sink
    }

    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.sink.write(bytes)?;
        self.bytes_written += written;
        Ok(written)
    }
}

/// RAII guard that closes the frame when dropped.
#[derive(Debug)]
pub struct SyncFrameGuard<'a, W: Write> {
    writer: &'a mut SynchronizedWriter<W>,
}

impl<W: Write> core::ops::Deref for SyncFrameGuard<'_, W> {
    type Target = SynchronizedWriter<W>;

    fn deref(&self) -> &Self::Target {
        self.writer
    }
}

impl<W: Write> core::ops::DerefMut for SyncFrameGuard<'_, W> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.writer
    }
}

impl<W: Write> Drop for SyncFrameGuard<'_, W> {
    fn drop(&mut self) {
        // Best-effort teardown. The in-memory sink is infallible; a real-sink
        // error during teardown is not recoverable from `Drop`.
        let _ = self.writer.end_frame();
    }
}
