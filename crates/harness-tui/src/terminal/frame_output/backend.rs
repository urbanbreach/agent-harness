use std::io::{self, BufWriter, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use super::capture::FrameOutputWriter;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameBackendMetrics {
    pub draw_calls: u64,
    pub cells_changed: u64,
    pub cursor_commands: u64,
    pub clears: u64,
}

#[derive(Debug)]
pub struct FrameOutputBackend {
    inner: CrosstermBackend<BufWriter<FrameOutputWriter>>,
    cursor_position: Option<Position>,
    cursor_visible: Option<bool>,
    metrics: FrameBackendMetrics,
}

impl FrameOutputBackend {
    const FRAME_BUFFER_CAPACITY: usize = 16 * 1024;

    pub fn new(writer: FrameOutputWriter) -> Self {
        Self {
            inner: CrosstermBackend::new(BufWriter::with_capacity(
                Self::FRAME_BUFFER_CAPACITY,
                writer,
            )),
            cursor_position: None,
            cursor_visible: None,
            metrics: FrameBackendMetrics::default(),
        }
    }

    pub fn invalidate_cursor_state(&mut self) {
        self.cursor_position = None;
        self.cursor_visible = None;
    }

    pub fn prepare_for_terminal_drop(&mut self) {
        self.cursor_visible = Some(true);
    }

    pub const fn metrics(&self) -> FrameBackendMetrics {
        self.metrics
    }
}

impl Write for FrameOutputBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        Write::flush(&mut self.inner)?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

impl Backend for FrameOutputBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.metrics.draw_calls = self.metrics.draw_calls.saturating_add(1);
        let mut changed = 0u64;
        let mut content = content
            .inspect(|_| changed = changed.saturating_add(1))
            .peekable();
        if content.peek().is_some() {
            self.cursor_position = None;
        }
        self.inner.draw(content)?;
        Backend::flush(&mut self.inner)?;
        self.metrics.cells_changed = self.metrics.cells_changed.saturating_add(changed);
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        if self.cursor_visible == Some(false) {
            return Ok(());
        }
        self.inner.hide_cursor()?;
        Backend::flush(&mut self.inner)?;
        self.metrics.cursor_commands = self.metrics.cursor_commands.saturating_add(1);
        self.cursor_visible = Some(false);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        if self.cursor_visible == Some(true) {
            return Ok(());
        }
        self.inner.show_cursor()?;
        Backend::flush(&mut self.inner)?;
        self.metrics.cursor_commands = self.metrics.cursor_commands.saturating_add(1);
        self.cursor_visible = Some(true);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let position = self.cursor_position.unwrap_or(Position::ORIGIN);
        self.cursor_position = Some(position);
        Ok(position)
    }

    fn set_cursor_position<P>(&mut self, position: P) -> Result<(), Self::Error>
    where
        P: Into<Position>,
    {
        let position = position.into();
        if self.cursor_position == Some(position) {
            return Ok(());
        }
        self.inner.set_cursor_position(position)?;
        Backend::flush(&mut self.inner)?;
        self.metrics.cursor_commands = self.metrics.cursor_commands.saturating_add(1);
        self.cursor_position = Some(position);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.metrics.clears = self.metrics.clears.saturating_add(1);
        self.inner.clear()?;
        Backend::flush(&mut self.inner)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.metrics.clears = self.metrics.clears.saturating_add(1);
        self.inner.clear_region(clear_type)?;
        Backend::flush(&mut self.inner)
    }

    fn size(&self) -> Result<Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Backend::flush(&mut self.inner)
    }
}
