use std::{collections::BTreeMap, ops::Range};

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::theme::{MarkdownColors, Theme};

// Raw mode retains Markdown punctuation, but uses the same semantic colors as
// rendered mode. Soft breaks collapse in both modes in the reference viewer.
pub(super) fn render(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let base = Style::default().fg(theme.markdown.text);
    let mut paint = Paint(BTreeMap::from([
        (0, Style::default()),
        (text.len(), Style::default()),
    ]));
    let mut source = text.to_owned();
    let mut stack = Vec::new();
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                paint.tag(text, range, &tag, theme);
                stack.push(tag);
            }
            Event::End(_) => {
                stack.pop();
            }
            Event::Text(_) => {
                if let Some(Tag::CodeBlock(kind)) = stack.last() {
                    paint.code(text, range, kind, theme);
                } else {
                    paint.set(range, inner_style(&stack, theme));
                }
            }
            Event::Code(code) => {
                let outer = Style::default()
                    .fg(theme.markdown.code)
                    .add_modifier(Modifier::DIM);
                paint.set(range.clone(), outer);
                if let Some(start) = text[range.clone()].find(code.as_ref()) {
                    paint.set(
                        range.start + start..range.start + start + code.len(),
                        Style::default()
                            .fg(theme.markdown.code)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                paint.set(range, base.add_modifier(Modifier::ITALIC));
            }
            Event::SoftBreak
                if !matches!(
                    text.as_bytes().get(range.end),
                    Some(b' ' | b'\t' | b'>' | b'|')
                ) =>
            {
                source.replace_range(range.clone(), &" ".repeat(range.len()));
                paint.set(range, Style::default().fg(ratatui::style::Color::Reset));
            }
            Event::TaskListMarker(checked) => {
                let style = if checked {
                    Style::default().fg(theme.markdown.task_checked)
                } else {
                    Style::default()
                        .fg(theme.markdown.text)
                        .add_modifier(Modifier::DIM)
                };
                paint.set(range, style);
            }
            Event::Rule => paint.set(range, Style::default().fg(theme.markdown.rule)),
            _ => {}
        }
    }
    paint.lines(&source)
}

fn inner_style(stack: &[Tag<'_>], theme: &Theme) -> Style {
    let mut style = Style::default().fg(theme.markdown.text);
    let link = stack
        .iter()
        .any(|tag| matches!(tag, Tag::Link { .. } | Tag::Image { .. }));
    for tag in stack {
        style = match tag {
            Tag::Heading { level, .. } => style
                .fg(theme.markdown.heading(*level as usize))
                .add_modifier(MarkdownColors::heading_modifier(*level as usize)),
            Tag::Strong => style.fg(theme.markdown.text).add_modifier(Modifier::BOLD),
            Tag::Emphasis => style.fg(theme.markdown.text).add_modifier(Modifier::ITALIC),
            Tag::Strikethrough => style
                .fg(theme.markdown.text)
                .add_modifier(Modifier::CROSSED_OUT),
            _ => style,
        };
    }
    if link {
        style = style
            .fg(theme.markdown.link)
            .add_modifier(Modifier::UNDERLINED);
    }
    style
}

struct Paint(BTreeMap<usize, Style>);

impl Paint {
    fn set(&mut self, range: Range<usize>, style: Style) {
        if range.is_empty() {
            return;
        }
        let after = self
            .0
            .range(..=range.end)
            .next_back()
            .map_or_default(|(_, style)| *style);
        let keys: Vec<_> = self.0.range(range.clone()).map(|(key, _)| *key).collect();
        for key in keys {
            self.0.remove(&key);
        }
        self.0.insert(range.start, style);
        self.0.insert(range.end, after);
    }

    fn tag(&mut self, text: &str, range: Range<usize>, tag: &Tag<'_>, theme: &Theme) {
        let dim = Style::default().add_modifier(Modifier::DIM);
        let muted = Style::default().fg(theme.markdown.muted);
        match tag {
            Tag::Heading { level, .. } => {
                let marker = text[range.clone()]
                    .find(|ch| ch != '#' && ch != ' ')
                    .unwrap_or(range.len());
                self.set(
                    range.start..range.start + marker,
                    dim.fg(theme.markdown.heading(*level as usize)),
                );
            }
            Tag::Strong | Tag::Emphasis | Tag::Strikethrough => self.set(range, dim),
            Tag::CodeBlock(kind) => {
                self.set(range.clone(), dim.fg(theme.markdown.code));
                if let CodeBlockKind::Fenced(language) = kind
                    && let Some(offset) = text[range.clone()]
                        .find(language.as_ref())
                        .filter(|_| !language.is_empty())
                {
                    self.set(
                        range.start + offset..range.start + offset + language.len(),
                        Style::default().fg(theme.markdown.heading_h3),
                    );
                }
            }
            Tag::BlockQuote(_) => {
                let mut start = range.start;
                for line in text[range].split_inclusive('\n') {
                    for (offset, ch) in line
                        .char_indices()
                        .take_while(|(_, ch)| matches!(ch, '>' | ' ' | '\t'))
                        .filter(|(_, ch)| *ch == '>')
                    {
                        self.set(
                            start + offset..start + offset + ch.len_utf8(),
                            muted.add_modifier(Modifier::DIM),
                        );
                    }
                    start += line.len();
                }
            }
            Tag::Item => {
                let source = &text[range.clone()];
                let body = source.trim_start();
                let length = if body.starts_with("- ") || body.starts_with("* ") {
                    2
                } else {
                    body.find(['.', ')'])
                        .filter(|end| body[..*end].bytes().all(|ch| ch.is_ascii_digit()))
                        .map_or(0, |end| end + 2)
                };
                let start = range.start + source.len() - body.len();
                self.set(start..(start + length).min(range.end), muted);
            }
            Tag::Table(_) | Tag::TableHead | Tag::TableRow => {
                self.set(range, Style::default().fg(theme.markdown.heading_h2))
            }
            Tag::TableCell => self.set(range, Style::default()),
            Tag::Link { title, .. } | Tag::Image { title, .. } => {
                self.set(range.clone(), muted);
                for (offset, ch) in text[range.clone()]
                    .char_indices()
                    .filter(|(_, ch)| ch.is_whitespace())
                {
                    self.set(
                        range.start + offset..range.start + offset + ch.len_utf8(),
                        Style::default(),
                    );
                }
                if !title.is_empty()
                    && let Some(offset) = text[range.clone()].rfind(title.as_ref())
                {
                    self.set(
                        range.start + offset.saturating_sub(1)
                            ..(range.start + offset + title.len() + 1).min(range.end),
                        Style::default().fg(theme.markdown.heading_h5),
                    );
                }
            }
            _ => {}
        }
    }

    fn code(&mut self, text: &str, range: Range<usize>, kind: &CodeBlockKind<'_>, theme: &Theme) {
        let language = match kind {
            CodeBlockKind::Fenced(language) => Some(language.as_ref()),
            _ => None,
        };
        let body = &text[range.clone()];
        self.set(
            range.clone(),
            Style::default()
                .fg(theme.markdown.text)
                .bg(theme.markdown.code_background),
        );
        let highlighted = super::super::ui_syntax_highlight::render_highlighted_code_block(
            language,
            body,
            body,
            "",
            theme.markdown.text,
            theme,
        );
        let mut offset = range.start;
        for (line, source) in highlighted.into_iter().zip(body.split_inclusive('\n')) {
            let mut column = offset;
            for span in line.spans {
                let end = (column + span.content.len()).min(range.end);
                self.set(column..end, span.style.bg(theme.markdown.code_background));
                column = end;
            }
            offset += source.len();
        }
    }

    fn lines(&self, text: &str) -> Vec<Line<'static>> {
        let mut result = Vec::new();
        let mut start = 0;
        for line in text.split('\n') {
            let end = start + line.len();
            let mut spans = Vec::new();
            let mut position = start;
            let mut style = self
                .0
                .range(..=start)
                .next_back()
                .map_or_default(|(_, style)| *style);
            for (&boundary, &next) in self
                .0
                .range(start..end)
                .filter(|(boundary, _)| **boundary > start)
            {
                spans.push(Span::styled(text[position..boundary].to_owned(), style));
                position = boundary;
                style = next;
            }
            spans.push(Span::styled(text[position..end].to_owned(), style));
            let background = spans.iter().find_map(|span| span.style.bg);
            let mut line = Line::from(spans);
            line.style.bg = background;
            result.push(line);
            start = end + 1;
        }
        result
    }
}
