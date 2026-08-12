use std::io::{self, Write};

use ratatui::backend::{Backend, ClearType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::layout::{Position, Size};

use super::queue::FrameOutputWriter;

#[derive(Debug)]
pub struct FrameOutputBackend {
    inner: CrosstermBackend<FrameOutputWriter>,
    cursor_position: Option<Position>,
    cursor_visible: Option<bool>,
}

impl FrameOutputBackend {
    pub fn new(writer: FrameOutputWriter) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
            cursor_position: None,
            cursor_visible: None,
        }
    }

    pub fn invalidate_cursor_state(&mut self) {
        self.cursor_position = None;
        self.cursor_visible = None;
    }
}

impl Write for FrameOutputBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.write(bytes)
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
        let mut content = content.peekable();
        if content.peek().is_some() {
            self.cursor_position = None;
        }
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        if self.cursor_visible == Some(false) {
            return Ok(());
        }
        self.inner.hide_cursor()?;
        self.cursor_visible = Some(false);
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        if self.cursor_visible == Some(true) {
            return Ok(());
        }
        self.inner.show_cursor()?;
        self.cursor_visible = Some(true);
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        let position = self.inner.get_cursor_position()?;
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
        self.cursor_position = Some(position);
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
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
