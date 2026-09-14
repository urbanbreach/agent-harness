use std::collections::BTreeMap;
use std::io::{self, BufWriter, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use super::capture::FrameOutputWriter;
use super::hyperlinks::{take_frame_hyperlinks, FrameHyperlink};

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;

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
    cells: BTreeMap<(u16, u16), Cell>,
    hyperlinks: Vec<FrameHyperlink>,
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
            cells: BTreeMap::new(),
            hyperlinks: Vec::new(),
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

    fn destination_at<'a>(links: &'a [FrameHyperlink], x: u16, y: u16) -> Option<&'a str> {
        links
            .iter()
            .find(|link| link.row == y && link.start_column <= x && x < link.end_column)
            .map(|link| link.destination.as_str())
            .filter(|destination| crate::transcript_selection::safe_external_url(destination))
    }

    fn write_hyperlink_control(&mut self, destination: Option<&str>) -> io::Result<()> {
        let sequence = destination.map_or_else(
            || "\x1b]8;;\x1b\\".to_string(),
            |url| format!("\x1b]8;;{url}\x1b\\"),
        );
        let sequence = if std::env::var_os("TMUX").is_some() {
            crate::transcript_selection::wrap_tmux(&sequence)
        } else {
            sequence
        };
        self.inner.write_all(sequence.as_bytes())
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
        let current_links = take_frame_hyperlinks();
        let mut changed = content
            .map(|(x, y, cell)| ((y, x), cell.clone()))
            .collect::<BTreeMap<_, _>>();
        if self.hyperlinks != current_links {
            for position in self
                .hyperlinks
                .iter()
                .chain(&current_links)
                .flat_map(|link| (link.start_column..link.end_column).map(move |x| (link.row, x)))
            {
                if let Some(cell) = self.cells.get(&position) {
                    changed.entry(position).or_insert_with(|| cell.clone());
                }
            }
        }
        if !changed.is_empty() {
            self.cursor_position = None;
        }

        // Keep row order and whole runs so Crossterm can reuse cursor/style state.
        let mut pending = changed.iter().peekable();
        while let Some((&(y, x), _)) = pending.peek().copied() {
            let destination = Self::destination_at(&current_links, x, y);
            if destination.is_some() {
                self.write_hyperlink_control(destination)?;
            }
            let result = self.inner.draw(std::iter::from_fn(|| {
                let (&(y, x), cell) = pending.next_if(|(&(y, x), _)| {
                    Self::destination_at(&current_links, x, y) == destination
                })?;
                Some((x, y, cell))
            }));
            let closed = if destination.is_some() {
                self.write_hyperlink_control(None)
            } else {
                Ok(())
            };
            result?;
            closed?;
        }
        Backend::flush(&mut self.inner)?;
        self.metrics.cells_changed = self
            .metrics
            .cells_changed
            .saturating_add(u64::try_from(changed.len()).unwrap_or(u64::MAX));
        self.cells.extend(changed);
        self.hyperlinks = current_links;
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
        Backend::flush(&mut self.inner)?;
        self.cells.clear();
        self.hyperlinks.clear();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        if matches!(clear_type, ClearType::All) {
            return self.clear();
        }
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
