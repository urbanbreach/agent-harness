use super::*;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

impl ViewerState {
    pub(super) fn toggle_child_markdown_mode(&mut self) -> Result<(), ViewerError> {
        let sources = source_lines(self.content.content(), self.mode);
        let tail = self
            .markdown_tail_rebased
            .then(|| tail_source_line(self.content.content()));
        let selected_source = self
            .row_line_ids
            .get(self.cursor.row)
            .and_then(|id| sources.get(*id))
            .map(|source| {
                tail.filter(|start| source >= start)
                    .map_or(*source, |start| source - start)
            });
        let screen_y = self
            .logical_rows(self.cursor.row)
            .start
            .saturating_sub(self.scroll_top());
        self.mode = if self.mode == ViewerMode::Raw {
            ViewerMode::Wrapped
        } else {
            ViewerMode::Raw
        };
        self.rebuild_display()?;
        self.visual_mode = false;
        self.selection = None;
        self.scroll_screen_y = None;
        if !self.following {
            let sources = source_lines(self.content.content(), self.mode);
            let id = selected_source
                .and_then(|target| sources.iter().position(|source| *source >= target))
                .unwrap_or(0);
            self.cursor = CellPoint::new(
                self.row_line_ids
                    .iter()
                    .position(|row_id| *row_id == id)
                    .unwrap_or(0),
                0,
            );
            self.scroll_top = f64::from(
                u32::try_from(self.cursor.row.saturating_sub(screen_y)).unwrap_or(u32::MAX),
            )
            .min(self.layout.max_scroll());
        }
        // Grok's mode change first rebuilds the full source map, then its next
        // render appends the unfrozen tail's map without a source-line offset.
        // Retain that numbering for the next toggle, including its cursor jump.
        self.markdown_tail_rebased = true;
        Ok(())
    }
}

fn source_lines(text: &str, mode: ViewerMode) -> Vec<usize> {
    let newlines = text
        .match_indices('\n')
        .map(|(byte, _)| byte)
        .collect::<Vec<_>>();
    let line_at = |byte| newlines.partition_point(|newline| *newline < byte);
    let mut lines = (0..=newlines.len()).collect::<std::collections::BTreeSet<_>>();
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::SoftBreak
                if !matches!(
                    text.as_bytes().get(range.end),
                    Some(b' ' | b'\t' | b'>' | b'|')
                ) =>
            {
                lines.remove(&line_at(range.start));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
                if mode == ViewerMode::Wrapped =>
            {
                lines.remove(&line_at(range.start));
                let closing = &text[range.clone()];
                if closing.lines().count() > 1
                    && closing
                        .lines()
                        .last()
                        .is_some_and(|line| line.trim_start().starts_with(['`', '~']))
                {
                    lines.remove(&line_at(range.end));
                }
            }
            _ => {}
        }
    }
    lines.into_iter().collect()
}

fn tail_source_line(text: &str) -> usize {
    let mut depth = 0_usize;
    let mut checkpoint = 0;
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::Table(_)) => {
                depth += 1
            }
            Event::End(tag) => {
                if matches!(
                    tag,
                    TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::Table
                ) {
                    depth = depth.saturating_sub(1);
                }
                let block = matches!(
                    tag,
                    TagEnd::Paragraph
                        | TagEnd::Heading(_)
                        | TagEnd::CodeBlock
                        | TagEnd::BlockQuote(_)
                        | TagEnd::List(_)
                        | TagEnd::Table
                        | TagEnd::HtmlBlock
                );
                let blank = text
                    .as_bytes()
                    .get(range.end..)
                    .and_then(|tail| tail.iter().find(|byte| !matches!(byte, b' ' | b'\t')))
                    == Some(&b'\n');
                let code = tag == TagEnd::CodeBlock;
                if depth == 0 && block && (blank || (code && range.end < text.len())) {
                    checkpoint = range.end + usize::from(code && blank);
                }
            }
            Event::Rule if depth == 0 => checkpoint = range.end,
            _ => {}
        }
    }
    if checkpoint > 0
        && text.as_bytes().get(checkpoint - 1) != Some(&b'\n')
        && text.as_bytes().get(checkpoint) == Some(&b'\n')
    {
        checkpoint += 1;
    }
    text[..checkpoint].matches('\n').count()
}
