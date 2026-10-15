use super::pages::{
    dim_state, frame_meta, grid_plan, page_columns, page_rows, render_all, utf16_len, utf16_prefix,
    utf16_suffix,
};
use super::{
    capacity, elide_data_urls, normalize_text, ArchiveError, ArchiveOutput, ArchivePlanInput,
    ArchiveSource,
};
use super::{layout::doc_pages, summary};

/// Re-render the complete retained source rather than carrying earlier image bytes.
///
/// A zero `frame_budget` is a caller error: the host skips the method instead. A zero
/// `max_frames` clamps to one, as every public compaction option does.
pub fn build_archive(input: &ArchivePlanInput) -> Result<ArchiveOutput, ArchiveError> {
    if input.frame_budget == 0 {
        return Err(ArchiveError::EmptyFrameBudget);
    }
    let budget =
        usize::try_from(input.frame_budget.min(input.max_frames.clamp(1, 80))).unwrap_or(80);
    plan(input, budget)
}

/// The planner proper; `budget == 0` discards the whole imaged middle.
fn plan(input: &ArchivePlanInput, budget: usize) -> Result<ArchiveOutput, ArchiveError> {
    let high = input.shape;
    if high.edge == 0 || high.edge > 16384 || capacity(&high) == 0 {
        return Err(ArchiveError::InvalidGeometry);
    }
    let previous = input.previous.as_ref();
    let prior = previous.map_or_else(String::new, |p| elide_data_urls(&p.text, true));
    let fresh = normalize_text(&elide_data_urls(&input.new_text, false), high.font);
    let source = if prior.is_empty() {
        fresh
    } else if fresh.is_empty() {
        prior
    } else {
        format!("{prior}█{fresh}")
    };
    let edge = usize::try_from(capacity(&high)).unwrap_or(usize::MAX);
    let prior_dropped = previous.map_or(0, |p| p.truncated_chars);
    let mut output = ArchiveOutput::empty(prior_dropped);
    let mut shapes = Vec::new();
    if utf16_len(&source) <= u64::try_from(edge.saturating_mul(2)).unwrap_or(u64::MAX) {
        output.text_head.clone_from(&source);
        output.source.text = source;
    } else {
        let head = utf16_prefix(&source, edge);
        let tail = utf16_suffix(&source, edge);
        let middle = &source[head.len()..source.len() - tail.len()];
        output.text_head = head.to_owned();
        output.text_tail = tail.to_owned();
        let mut dim = dim_state(head, false);
        let mut retained = String::new();
        let mut dropped = 0;
        let mut jobs = Vec::new();
        let mut frame = |page: &str, shape: &super::ArchiveShape, dim: &mut bool| {
            let framed = if *dim {
                format!("\u{e}{page}")
            } else {
                page.to_owned()
            };
            *dim = dim_state(page, *dim);
            jobs.push((framed, *shape));
            shapes.push(*shape);
        };
        if budget == 0 {
            dropped = utf16_len(middle);
        } else if high.columns == 2 {
            let pages = doc_pages(middle, &high);
            let drop = pages.len().saturating_sub(budget);
            for (index, page) in pages.iter().enumerate() {
                if drop > 0 && index > 0 && index <= drop {
                    dropped += utf16_len(page);
                    continue;
                }
                frame(page, &high, &mut dim);
                if !retained.is_empty() {
                    retained.push(' ');
                }
                retained.push_str(&page.replace('\n', " "));
            }
        } else {
            let (pages, removed) = grid_plan(middle, &high, budget);
            dropped = removed;
            for page in pages {
                frame(page.text, &page.shape, &mut dim);
                retained.push_str(page.text);
            }
        }
        output.frames = render_all(&jobs)?;
        if dim && !output.text_tail.starts_with('\u{e}') {
            output.text_tail.insert(0, '\u{e}');
        }
        output.source.text.reserve(source.len());
        output.source.text.push_str(&output.text_head);
        output.source.text.push_str(&retained);
        output.source.text.push_str(&output.text_tail);
        output.source.truncated_chars = prior_dropped.saturating_add(dropped);
    }
    output.frame_meta = shapes.iter().map(frame_meta).collect();
    let text_chars = utf16_len(&output.text_head) + utf16_len(&output.text_tail);
    output.total_chars = output.frames.iter().map(|f| f.chars).sum::<u64>() + text_chars;
    output.short_summary =
        summary::short_summary(output.total_chars, output.frames.len(), text_chars);
    let mut widths = Vec::new();
    for shape in &shapes {
        let width = page_columns(shape);
        if !widths.contains(&width) {
            widths.push(width);
        }
    }
    let rows = shapes.iter().map(page_rows).max().unwrap_or(0);
    output.lead_in = summary::lead_in(&output, &high, &widths, rows);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::resolve_shape;

    #[test]
    fn zero_budget_is_rejected_publicly_and_discards_the_middle_internally() {
        let shape = resolve_shape("", "", Some("8on16-bw")).with_frame_size(64);
        let input = ArchivePlanInput {
            previous: None,
            new_text: "x ".repeat(400),
            frame_budget: 0,
            shape,
            max_frames: 0,
        };
        assert!(matches!(
            build_archive(&input),
            Err(ArchiveError::EmptyFrameBudget)
        ));
        let none = plan(&input, 0).map(|o| (o.frames.len(), o.source.truncated_chars));
        assert!(matches!(none, Ok((0, dropped)) if dropped > 0));
        let one = ArchivePlanInput {
            frame_budget: 3,
            ..input
        };
        assert!(build_archive(&one).is_ok_and(|o| o.frames.len() == 1));
    }

    #[test]
    fn edges_never_split_a_surrogate_pair_and_totals_include_text() -> Result<(), ArchiveError> {
        let shape = resolve_shape("", "", Some("8on16-bw")).with_frame_size(64);
        let text = "😀".repeat(500);
        let output = build_archive(&ArchivePlanInput {
            previous: None,
            new_text: text,
            frame_budget: 2,
            shape,
            max_frames: 2,
        })?;
        assert!(!output.text_head.contains('\u{fffd}') && !output.text_tail.contains('\u{fffd}'));
        let edges = utf16_len(&output.text_head) + utf16_len(&output.text_tail);
        let framed: u64 = output.frames.iter().map(|f| f.chars).sum();
        assert_eq!(output.total_chars, framed + edges);
        assert_eq!(output.frame_meta.len(), output.frames.len());
        Ok(())
    }
}
