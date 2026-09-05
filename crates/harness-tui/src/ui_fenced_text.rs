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
    let mut plain_lines = Vec::new();
    let mut code_lines = Vec::new();
    let mut raw_lines = Vec::new();
    let mut language = None;
    let mut in_code = false;

    for line in text.lines().map(normalize_fenced_line) {
        if !in_code {
            if let Some(block_language) = opening_fence_language(line) {
                if !plain_lines.is_empty() {
                    blocks.push(ParsedTextBlock::Plain(plain_lines.join("\n")));
                    plain_lines.clear();
                }
                in_code = true;
                language = block_language.map(str::to_string);
                raw_lines.push(line.to_string());
                continue;
            }
            plain_lines.push(line.to_string());
            continue;
        }

        raw_lines.push(line.to_string());
        if is_closing_fence(line) {
            blocks.push(ParsedTextBlock::Code {
                language: language.take(),
                body: code_lines.join("\n"),
                raw: raw_lines.join("\n"),
            });
            code_lines.clear();
            raw_lines.clear();
            in_code = false;
        } else {
            code_lines.push(line.to_string());
        }
    }

    if in_code {
        if !include_open_fence {
            return None;
        }
        blocks.push(ParsedTextBlock::Code {
            language,
            body: code_lines.join("\n"),
            raw: raw_lines.join("\n"),
        });
    }

    if !plain_lines.is_empty() {
        blocks.push(ParsedTextBlock::Plain(plain_lines.join("\n")));
    }

    Some(blocks)
}

fn opening_fence_language(line: &str) -> Option<Option<&str>> {
    let trimmed = line.trim_start();
    let suffix = trimmed.strip_prefix("```")?;
    let language = suffix
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty());
    Some(language)
}

fn is_closing_fence(line: &str) -> bool {
    line.trim_start().starts_with("```")
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
}
