use crate::UnwrapOrAbort;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::Terminal;

pub fn render_to_string<ViewModel>(
    view_model: &ViewModel,
    area: Rect,
    render: impl FnOnce(&ViewModel, &mut ratatui::Frame<'_>, Rect),
) -> String {
    let buffer = render_to_buffer(view_model, area, render);
    buffer_to_string(&buffer, area.width)
}

pub fn render_to_buffer<ViewModel>(
    view_model: &ViewModel,
    area: Rect,
    render: impl FnOnce(&ViewModel, &mut ratatui::Frame<'_>, Rect),
) -> Buffer {
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render(view_model, frame, area))
        .unwrap_or_abort();
    terminal.backend().buffer().clone()
}

pub fn buffer_to_string(buffer: &Buffer, width: u16) -> String {
    buffer
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
