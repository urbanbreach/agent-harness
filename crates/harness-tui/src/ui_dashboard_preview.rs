use super::*;
use crate::transcript_blocks::{BlockKind, BlockSnapshot, FoldState};

pub(crate) fn lines(blocks: &[BlockSnapshot], width: u16, theme: &Theme) -> Vec<Line<'static>> {
    blocks
        .iter()
        .flat_map(|block| block_lines(block, width, theme))
        .collect()
}

fn block_lines(block: &BlockSnapshot, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    if block.kind == BlockKind::Thinking {
        return vec![Line::from(Span::styled(
            "Thought",
            Style::default().fg(theme.text.secondary),
        ))];
    }
    if block.fold_state == FoldState::Collapsed {
        let summary =
            ui_tool_output::safe_tool_text(block.content.lines().next().unwrap_or_default());
        return vec![Line::from(Span::styled(
            ui_chrome::truncate_plain_text(&summary, usize::from(width)),
            Style::default().fg(theme.text.secondary),
        ))];
    }
    let mut lines = Vec::new();
    if block.kind == BlockKind::User {
        for (index, row) in wrap_completion_text(
            &ui_tool_output::safe_tool_text(&block.content),
            usize::from(width.saturating_sub(2).max(1)),
        )
        .into_iter()
        .enumerate()
        {
            lines.push(Line::from(Span::styled(
                format!("{} {row}", if index == 0 { "❯" } else { " " }),
                Style::default().fg(theme.text.accent),
            )));
        }
    } else {
        ui_markdown::append_rich_text_block(
            &mut lines,
            &block.content,
            theme.text.primary,
            "",
            theme,
            width,
        );
    }
    lines
}

/// Pin only a fully scrolled-away prompt. The next prompt pushes it out of the
/// preview, sharing the exact body measurement used by scrolling.
pub(crate) fn frame(
    blocks: &[BlockSnapshot],
    width: u16,
    height: u16,
    top: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut body = Vec::new();
    let mut prompts = Vec::new();
    for block in blocks {
        let rows = block_lines(block, width, theme);
        if block.kind == BlockKind::User {
            prompts.push((body.len()..body.len() + rows.len(), rows.clone()));
        }
        body.extend(rows);
    }
    let height = usize::from(height);
    let top = top.min(body.len().saturating_sub(height));
    let pinned = prompts.iter().rev().find(|(range, _)| range.end <= top);
    let mut visible = Vec::new();
    if let Some((_, prompt)) = pinned {
        let distance = prompts
            .iter()
            .find(|(range, _)| range.start >= top)
            .map_or(height, |(range, _)| range.start - top);
        let count = prompt.len().min(4).min(height / 3).min(distance);
        visible.extend(prompt.iter().take(count).cloned());
    }
    visible.extend(
        body.into_iter()
            .skip(top)
            .take(height.saturating_sub(visible.len())),
    );
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript_blocks::BlockLifecycle;
    use crate::transcript_identity::BlockId;

    #[test]
    fn collapsed_tool_output_stays_one_row_in_measurement_and_paint() {
        let block = BlockSnapshot {
            id: BlockId::from_replay(1, 0, 0),
            kind: BlockKind::Tool,
            lifecycle: BlockLifecycle::Completed,
            content: format!("Batch completed\n{}", "full tool output\n".repeat(10_000)),
            fold_state: FoldState::Collapsed,
            raw: None,
        };
        let mut blocks = [block];
        let theme = Theme::default();
        let measured = lines(&blocks, 80, &theme);
        assert_eq!(measured.len(), 1);
        assert_eq!(measured[0].to_string(), "Batch completed");
        assert_eq!(frame(&blocks, 80, 10, 0, &theme), measured);
        blocks[0].kind = BlockKind::Thinking;
        assert_eq!(lines(&blocks, 80, &theme)[0].to_string(), "Thought");
        assert_eq!(frame(&blocks, 80, 10, 0, &theme)[0].to_string(), "Thought");
    }
}
