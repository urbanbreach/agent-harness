use super::{normalize_text, ArchiveError, ArchiveShape, RenderedFrame};

pub fn capacity(shape: &ArchiveShape) -> u32 {
    let cols = shape.edge / shape.cell_width.max(1);
    let rows = shape.edge / shape.cell_height.max(1) / shape.line_repeat.max(1);
    if shape.columns == 2 {
        cols.saturating_sub(3) / 2 * rows * 2
    } else {
        cols * rows
    }
}

pub(super) fn cell_width(ch: char, font: &str) -> u32 {
    if matches!(ch, '\u{e}' | '\u{f}') {
        return 0;
    }
    if font == "silver" {
        return 1;
    }
    super::fonts::cell_width(ch)
}

/// Contiguous source pages; dim controls use no cells and wide glyphs wrap atomically.
pub fn paginate_grid<'a>(text: &'a str, shape: &ArchiveShape) -> Vec<&'a str> {
    let cols = shape.edge / shape.cell_width.max(1);
    let cap = capacity(shape);
    if cap == 0 || text.is_empty() {
        return Vec::new();
    }
    let mut pages = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (offset, ch) in text.char_indices() {
        let width = cell_width(ch, shape.font);
        if width == 0 {
            continue;
        }
        let pad = u32::from(width == 2 && used % cols == cols - 1);
        if used > 0 && used + pad + width > cap {
            pages.push(&text[start..offset]);
            start = offset;
            used = 0;
        } else {
            used += pad;
        }
        used += width;
    }
    if start < text.len() {
        pages.push(&text[start..]);
    }
    pages
}

/// Greedy word wrapping with cell-aware hard splitting for oversized words.
pub fn wrap_doc(text: &str, width: u32, font: &str) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in text.split_whitespace() {
        let word_width: u32 = word.chars().map(|c| cell_width(c, font)).sum();
        if used > 0 && used + 1 + word_width > width {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        if word_width <= width {
            if used > 0 {
                line.push(' ');
                used += 1;
            }
            line.push_str(word);
            used += word_width;
            continue;
        }
        for ch in word.chars() {
            let cells = cell_width(ch, font);
            if used > 0 && used + cells > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(ch);
            used += cells;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

pub(super) fn doc_pages(text: &str, shape: &ArchiveShape) -> Vec<String> {
    let cols = (shape.edge / shape.cell_width.max(1)).saturating_sub(3) / 2;
    let rows = shape.edge / shape.cell_height.max(1) / shape.line_repeat.max(1);
    let lines = wrap_doc(text, cols, shape.font);
    if rows == 0 {
        return Vec::new();
    }
    lines
        .chunks(usize::try_from(rows * 2).unwrap_or(usize::MAX))
        .map(|chunk| chunk.join("\n"))
        .collect()
}

/// General rendering retains oldest pages and allows an explicit zero-frame cap. Pages
/// rasterize concurrently and come back in page order.
pub fn render_many(
    text: &str,
    shape: &ArchiveShape,
    max_frames: Option<u32>,
) -> Result<Vec<RenderedFrame>, ArchiveError> {
    let text = normalize_text(text, shape.font);
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let limit = usize::try_from(max_frames.unwrap_or(u32::MAX)).unwrap_or(usize::MAX);
    let jobs: Vec<(String, ArchiveShape)> = if shape.columns == 2 {
        doc_pages(&text, shape)
            .into_iter()
            .take(limit)
            .map(|page| (page, *shape))
            .collect()
    } else {
        paginate_grid(&text, shape)
            .into_iter()
            .take(limit)
            .map(|page| (page.to_owned(), *shape))
            .collect()
    };
    super::pages::render_all(&jobs)
}
