use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParsedTextBlock {
    Plain(String),
    Code {
        language: Option<String>,
        body: String,
        raw: String,
    },
}

pub(super) fn parse_fenced_text_blocks(text: &str) -> Option<Vec<ParsedTextBlock>> {
    parse_fenced_text_blocks_inner(text, false)
}

pub(super) fn parse_streaming_fenced_text_blocks(text: &str) -> Vec<ParsedTextBlock> {
    parse_fenced_text_blocks_inner(text, true).unwrap_or_default()
}

fn parse_fenced_text_blocks_inner(
    text: &str,
    include_open_fence: bool,
) -> Option<Vec<ParsedTextBlock>> {
    let mut blocks = Vec::new();
    let mut cursor = 0;
    let mut events = Parser::new(text).into_offset_iter();
    while let Some((event, range)) = events.next() {
        let Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) = event else {
            continue;
        };
        // The parser range starts at the marker, not its permitted indentation.
        let start = text[..range.start].rfind('\n').map_or(0, |index| index + 1);
        if cursor < start {
            blocks.push(ParsedTextBlock::Plain(
                text[cursor..start]
                    .lines()
                    .map(normalize_fenced_line)
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
        let mut body = String::new();
        let mut body_end = text[range.start..]
            .find('\n')
            .map_or(text.len(), |index| range.start + index + 1);
        for (event, body_range) in events.by_ref() {
            match event {
                Event::Text(value) => {
                    body.push_str(&value);
                    body_end = body_range.end;
                }
                Event::End(TagEnd::CodeBlock) => break,
                _ => {}
            }
        }
        // An EOF-synthesized end has no closing-marker bytes after the body.
        // Retain the existing settled/open API contract while streaming exposes
        // the structurally parsed open body.
        if !include_open_fence && body_end >= range.end {
            return None;
        }
        let mut block_end = range.end;
        if body_end < range.end {
            // Pulldown excludes closing-line whitespace from the block range.
            // Consume that line, but leave subsequent blank lines as prose.
            block_end += text[block_end..]
                .bytes()
                .take_while(|byte| matches!(*byte, b' ' | b'\t'))
                .count();
            if text[block_end..].starts_with("\r\n") {
                block_end += 2;
            } else if matches!(text.as_bytes().get(block_end), Some(b'\r' | b'\n')) {
                block_end += 1;
            }
        }
        blocks.push(ParsedTextBlock::Code {
            language: info.split_whitespace().next().map(str::to_string),
            body: body.strip_suffix('\n').unwrap_or(&body).to_string(),
            raw: text[start..block_end]
                .lines()
                .map(normalize_fenced_line)
                .collect::<Vec<_>>()
                .join("\n"),
        });
        cursor = block_end;
    }
    if cursor < text.len() {
        blocks.push(ParsedTextBlock::Plain(
            text[cursor..]
                .lines()
                .map(normalize_fenced_line)
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    Some(blocks)
}

fn normalize_fenced_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
mod tests {
    use super::{parse_fenced_text_blocks, parse_streaming_fenced_text_blocks, ParsedTextBlock};

    #[test]
    fn parses_plain_and_fenced_code_blocks() {
        assert_eq!(
            parse_fenced_text_blocks("Before\n```rust\nfn main() {}\n```\nAfter"),
            Some(vec![
                ParsedTextBlock::Plain("Before".to_string()),
                ParsedTextBlock::Code {
                    language: Some("rust".to_string()),
                    body: "fn main() {}".to_string(),
                    raw: "```rust\nfn main() {}\n```".to_string(),
                },
                ParsedTextBlock::Plain("After".to_string()),
            ])
        );
    }

    #[test]
    fn parses_fence_without_language_and_normalizes_crlf() {
        assert_eq!(
            parse_fenced_text_blocks("```\r\nline\r\n```\r"),
            Some(vec![ParsedTextBlock::Code {
                language: None,
                body: "line".to_string(),
                raw: "```\nline\n```".to_string(),
            }])
        );
    }

    #[test]
    fn returns_none_for_unclosed_fence() {
        assert_eq!(
            parse_fenced_text_blocks("Before\n```rust\nfn main() {}"),
            None
        );
    }

    #[test]
    fn indented_fences_are_recognized() {
        assert_eq!(
            parse_fenced_text_blocks("  ```diff\n+added\n  ```"),
            Some(vec![ParsedTextBlock::Code {
                language: Some("diff".to_string()),
                body: "+added".to_string(),
                raw: "  ```diff\n+added\n  ```".to_string(),
            }])
        );
    }

    #[test]
    fn streaming_parser_exposes_open_fence_body() {
        assert_eq!(
            parse_streaming_fenced_text_blocks("Before\n```rust\nfn main() {}"),
            vec![
                ParsedTextBlock::Plain("Before".to_string()),
                ParsedTextBlock::Code {
                    language: Some("rust".to_string()),
                    body: "fn main() {}".to_string(),
                    raw: "```rust\nfn main() {}".to_string(),
                },
            ]
        );
    }

    #[test]
    fn code_body_keeps_source_indentation_while_streaming_and_settled() {
        let source = "```python\ndef f():\n    return 1\n```";
        let expected = "def f():\n    return 1";

        for blocks in [
            parse_fenced_text_blocks(source).expect("closed fence should parse"),
            parse_streaming_fenced_text_blocks(source),
        ] {
            assert_eq!(
                blocks,
                vec![ParsedTextBlock::Code {
                    language: Some("python".to_string()),
                    body: expected.to_string(),
                    raw: source.to_string(),
                }]
            );
        }
    }
}
