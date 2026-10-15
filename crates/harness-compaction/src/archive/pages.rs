//! Page planning (foveation) and ordered concurrent rasterization.
use super::{
    capacity, paginate_grid, render_frame, ArchiveError, ArchiveShape, FrameMeta, Ink,
    RenderedFrame,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

pub(super) fn utf16_len(text: &str) -> u64 {
    u64::try_from(text.encode_utf16().count()).unwrap_or(u64::MAX)
}

/// The dim-output state after `text`, starting from `dim`.
pub(super) fn dim_state(text: &str, mut dim: bool) -> bool {
    for c in text.chars() {
        match c {
            '\u{e}' => dim = true,
            '\u{f}' => dim = false,
            _ => {}
        }
    }
    dim
}

/// The longest prefix of at most `units` UTF-16 code units. A cut inside a surrogate pair
/// retreats to the previous scalar; Rust strings cannot hold a lone surrogate.
pub(super) fn utf16_prefix(text: &str, units: usize) -> &str {
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        used += ch.len_utf16();
        if used > units {
            return &text[..index];
        }
    }
    text
}

/// The longest suffix of at most `units` UTF-16 code units, retreating like the prefix.
pub(super) fn utf16_suffix(text: &str, units: usize) -> &str {
    let mut used = 0;
    for (index, ch) in text.char_indices().rev() {
        used += ch.len_utf16();
        if used > units {
            return &text[index + ch.len_utf8()..];
        }
    }
    text
}

#[derive(Debug)]
pub(super) struct Page<'a> {
    pub text: &'a str,
    pub shape: ArchiveShape,
}

/// The dense companion of a high-quality shape: same edge, price and detail, plain ink.
fn dense_companion(high: &ArchiveShape) -> ArchiveShape {
    let mut dense = *high;
    dense.id = "8on16-bw";
    dense.font = "8x13";
    dense.cell_width = 8;
    dense.cell_height = 16;
    dense.line_repeat = 1;
    dense.ink = Ink::Black;
    dense.columns = 1;
    dense.stopword_dim = false;
    dense.stretch = false;
    // The companion never inherits the high shape's verification.
    dense.verified = false;
    dense.frame_token_estimate = dense.frame_token_estimate.max(5024);
    dense
}

/// Pages for a grid shape. When the high-quality pages exceed `budget`, the first and last
/// `k` stay high quality, the middle is repaginated densely and only its newest pages
/// survive. Returns the pages and the UTF-16 length of the discarded oldest dense pages.
pub(super) fn grid_plan<'a>(
    middle: &'a str,
    high: &ArchiveShape,
    budget: usize,
) -> (Vec<Page<'a>>, u64) {
    let high_pages = paginate_grid(middle, high);
    if high_pages.len() <= budget {
        let pages = high_pages
            .into_iter()
            .map(|text| Page { text, shape: *high })
            .collect();
        return (pages, 0);
    }
    let dense = dense_companion(high);
    let foveate = high.font != "silver" && capacity(&dense) > capacity(high);
    let k = if foveate {
        3.min(budget.saturating_sub(1) / 2)
    } else {
        0
    };
    let start: usize = high_pages.iter().take(k).map(|s| s.len()).sum();
    let end = middle.len()
        - high_pages
            .iter()
            .rev()
            .take(k)
            .map(|s| s.len())
            .sum::<usize>();
    let mid_shape = if foveate { dense } else { *high };
    let dense_pages = paginate_grid(&middle[start..end], &mid_shape);
    let drop = dense_pages
        .len()
        .saturating_sub(budget.saturating_sub(k * 2));
    let dropped = dense_pages.iter().take(drop).map(|s| utf16_len(s)).sum();
    let pages = high_pages
        .iter()
        .take(k)
        .map(|text| Page { text, shape: *high })
        .chain(dense_pages.iter().skip(drop).map(|text| Page {
            text,
            shape: mid_shape,
        }))
        .chain(
            high_pages
                .iter()
                .skip(high_pages.len() - k)
                .map(|text| Page { text, shape: *high }),
        )
        .collect();
    (pages, dropped)
}

/// Cell columns of one page grid; a document column is narrower than the frame.
pub(super) fn page_columns(shape: &ArchiveShape) -> u32 {
    let columns = shape.edge / shape.cell_width.max(1);
    if shape.columns == 2 {
        columns.saturating_sub(3) / 2
    } else {
        columns
    }
}

/// Unique-line rows of one page grid.
pub(super) fn page_rows(shape: &ArchiveShape) -> u32 {
    shape.edge / shape.cell_height.max(1) / shape.line_repeat.max(1)
}

pub(super) fn frame_meta(shape: &ArchiveShape) -> FrameMeta {
    FrameMeta {
        columns: page_columns(shape),
        rows: page_rows(shape),
        shape: shape.id,
        font: shape.font,
        variant: if shape.ink == Ink::Sentence {
            "sent"
        } else {
            "bw"
        },
        detail: shape.detail,
    }
}

/// Rasterize pages on a bounded set of worker threads; the result keeps plan order.
pub(super) fn render_all(
    jobs: &[(String, ArchiveShape)],
) -> Result<Vec<RenderedFrame>, ArchiveError> {
    if jobs.len() <= 1 {
        return jobs
            .iter()
            .map(|(text, shape)| render_frame(text, shape))
            .collect();
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(jobs.len());
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Result<RenderedFrame, ArchiveError>>>> =
        jobs.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let (Some((text, shape)), Some(slot)) = (jobs.get(index), slots.get(index)) else {
                    break;
                };
                let rendered = render_frame(text, shape);
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(rendered);
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .ok()
                .flatten()
                .unwrap_or_else(|| Err(ArchiveError::Encoding("render worker stopped".into())))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_edges_retreat_from_a_surrogate_pair() {
        for (text, units, prefix, suffix) in [
            ("ab", 5, "ab", "ab"),
            ("a😀b", 2, "a", "b"),
            ("a😀b", 3, "a😀", "😀b"),
            ("😀😀", 3, "😀", "😀"),
        ] {
            assert_eq!(utf16_prefix(text, units), prefix, "{text} prefix");
            assert_eq!(utf16_suffix(text, units), suffix, "{text} suffix");
        }
    }
}
