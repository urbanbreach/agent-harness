//! Detect runaway repetition without rescanning the accumulated response.
//! Text and reasoning have independent 16 KiB inspection tails, checked after
//! 512 new bytes (and at completion). Passage loops need six repeats spanning
//! 1500 characters; line loops need twelve identical non-trivial complete lines.
//! Structured output breaks candidate runs; only prose-like words can trigger.
use harness_providers::ProviderStreamEvent;

const TAIL_BYTES: usize = 16 * 1024;
const CHECK_BYTES: usize = 512;
const MIN_PASSAGE_CHARS: usize = 32;
const MIN_REPEATS: usize = 6;
const MIN_SPAN_CHARS: usize = 1500;
const MIN_LINE_REPEATS: usize = 12;

#[derive(Default)]
pub(super) struct StreamGuard {
    text: Tail,
    reasoning: Tail,
}

impl StreamGuard {
    pub(super) fn observe(&mut self, event: &ProviderStreamEvent) -> Option<String> {
        match event {
            ProviderStreamEvent::TextDelta(delta) => self.text.push(delta),
            ProviderStreamEvent::ReasoningDelta(delta) => self.reasoning.push(delta),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                self.text.finish().or_else(|| self.reasoning.finish())
            }
            _ => None,
        }
    }
}

#[derive(Default)]
struct Tail {
    text: String,
    new_bytes: usize,
    matches: Vec<usize>,
    prose: String,
    context: StructuredContext,
}

impl Tail {
    fn push(&mut self, delta: &str) -> Option<String> {
        if delta.is_empty() {
            return None;
        }
        // Keep spare space so tiny deltas do not shift the entire tail each time.
        if delta.len() >= TAIL_BYTES {
            self.context.consume(&self.text);
            self.text.clear();
            let suffix = retained(delta);
            self.context.consume(&delta[..delta.len() - suffix.len()]);
            self.text.push_str(suffix);
        } else {
            if self.text.len() + delta.len() > TAIL_BYTES * 2 {
                let start = self.text.len() - retained(&self.text).len();
                self.context.consume(&self.text[..start]);
                self.text.drain(..start);
            }
            self.text.push_str(delta);
        }
        self.new_bytes = self.new_bytes.saturating_add(delta.len());
        if self.new_bytes < CHECK_BYTES {
            return None;
        }
        self.check()
    }

    fn finish(&mut self) -> Option<String> {
        if self.new_bytes == 0 {
            return None;
        }
        self.check()
    }

    fn check(&mut self) -> Option<String> {
        self.new_bytes = 0;
        self.prose.clear();
        let cutoff = self.text.len() - recent(&self.text).len();
        let mut context = self.context;
        let mut offset = 0;
        for line in self.text.split_inclusive('\n') {
            let blocked = context.line(line);
            if blocked || (!line.trim().is_empty() && !prose_like(line)) {
                self.prose.clear();
            } else if offset + line.len() > cutoff {
                self.prose.push_str(&line[cutoff.saturating_sub(offset)..]);
            }
            offset += line.len();
        }
        if repeated_line(&self.prose) {
            return Some("it began repeating the same line".into());
        }
        repeated_passage(&self.prose, &mut self.matches)
            .map(|chars| format!("it began repeating the same {chars}-character passage"))
    }
}

fn recent(text: &str) -> &str {
    let start = text.ceil_char_boundary(text.len().saturating_sub(TAIL_BYTES));
    &text[start..]
}

fn retained(text: &str) -> &str {
    let tail = recent(text);
    if tail.len() == text.len() {
        return tail;
    }
    // Do not split a nearby fence delimiter when advancing persisted context.
    // Long single-line prose still retains its tail rather than skipping to EOF.
    tail.bytes()
        .take(256)
        .position(|byte| byte == b'\n')
        .map_or(tail, |newline| &tail[newline + 1..])
}

#[derive(Clone, Copy)]
struct StructuredContext {
    fence: Option<(u8, usize)>,
    at_line_start: bool,
    blocked_line: bool,
}

impl Default for StructuredContext {
    fn default() -> Self {
        Self {
            fence: None,
            at_line_start: true,
            blocked_line: false,
        }
    }
}

impl StructuredContext {
    fn consume(&mut self, text: &str) {
        for line in text.split_inclusive('\n') {
            self.line(line);
        }
    }

    fn line(&mut self, line: &str) -> bool {
        if self.at_line_start {
            let trimmed = line.trim_start();
            let marker = trimmed
                .as_bytes()
                .first()
                .copied()
                .filter(|b| matches!(b, b'`' | b'~'));
            let width = marker.map_or(0, |marker| {
                trimmed.bytes().take_while(|b| *b == marker).count()
            });
            let in_fence = self.fence.is_some();
            if let Some((open_marker, open_width)) = self.fence {
                if marker == Some(open_marker)
                    && width >= open_width
                    && trimmed[width..].trim().is_empty()
                {
                    self.fence = None;
                }
            } else if let Some(marker) = marker.filter(|_| width >= 3) {
                self.fence = Some((marker, width));
            }
            self.blocked_line = in_fence
                || self.fence.is_some()
                || trimmed.contains('|')
                || trimmed.starts_with(['{', '}', '[', ']', '"'])
                || trimmed.trim_end().ends_with(';')
                || line.starts_with("    ")
                || line.starts_with('\t');
        }
        let blocked = self.blocked_line;
        self.at_line_start = line.ends_with('\n');
        if self.at_line_start {
            self.blocked_line = false;
        }
        blocked
    }
}

fn prose_like(text: &str) -> bool {
    text.split_whitespace()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .take(2)
        .count()
        == 2
}

fn repeated_line(text: &str) -> bool {
    let mut previous = "";
    let mut repeats = 0;
    // Ignore an unfinished last line, which may still become different.
    for line in text
        .split_inclusive('\n')
        .filter(|line| line.ends_with('\n'))
    {
        let line = line.trim();
        if line == previous {
            repeats += 1;
        } else {
            previous = line;
            repeats = 1;
        }
        if repeats >= MIN_LINE_REPEATS
            && line.chars().filter(|c| !c.is_whitespace()).take(10).count() == 10
            && prose_like(line)
            && line.chars().filter(|c| c.is_alphanumeric()).take(4).count() == 4
        {
            return true;
        }
    }
    false
}

fn repeated_passage(text: &str, matches: &mut Vec<usize>) -> Option<usize> {
    if text.len() < MIN_SPAN_CHARS {
        return None;
    }
    let bytes = text.as_bytes();
    let len = bytes.len();
    matches.resize(len * 2 + 1, 0);
    matches.fill(0);
    let (matches, chars) = matches.split_at_mut(len);
    for i in 0..len {
        chars[i + 1] = chars[i] + usize::from(bytes[len - 1 - i] & 0xc0 != 0x80);
    }
    // Z lengths on reversed bytes identify periodic suffixes in linear work.
    // Reuse the scratch buffer; no reversed copy or per-candidate allocation.
    let (mut left, mut right) = (0, 0);
    for i in 1..len {
        if i < right {
            matches[i] = matches[i - left].min(right - i);
        }
        while i + matches[i] < len && bytes[len - 1 - matches[i]] == bytes[len - 1 - i - matches[i]]
        {
            matches[i] += 1;
        }
        if i + matches[i] > right {
            left = i;
            right = i + matches[i];
        }
    }
    for period in 1..=len / MIN_REPEATS {
        let span = period + matches[period];
        if span < period * MIN_REPEATS || chars[span] < MIN_SPAN_CHARS {
            continue;
        }
        let Some(unit) = text.get(len - period..) else {
            continue;
        };
        // The smallest period must itself be a passage, not a multiple of a
        // short code line, punctuation, or whitespace repeated over and over.
        return (chars[period] >= MIN_PASSAGE_CHARS && prose_like(unit)).then_some(chars[period]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(count: usize, line: impl Fn(usize) -> String) -> String {
        let mut text = String::new();
        for index in 0..count {
            text.push_str(&line(index));
        }
        text
    }

    #[test]
    fn stream_guard_accepts_tables_code_lists_and_long_prose() {
        let table = format!(
            "| Item | State |\n| ---------- | ---------- |\n{}",
            numbered(100, |i| format!("| item {i} | ready |\n"))
        );
        let code = format!("```rust\n{}\n```", "{\n    step();\n}\n".repeat(200));
        let list = numbered(100, |i| format!("- Shared prefix, distinct item {i}\n"));
        let prose = numbered(100, |i| {
            format!("Paragraph {i} discusses a different example and its implications. ")
        });
        for content in [
            table,
            code,
            list,
            prose,
            "| -------------------- | -------------------- |\n".repeat(100),
            "ä".repeat(1200),
            format!("```rust\n{}\n```", "buffer.push(0);\n".repeat(100)),
            "return value;\n".repeat(100),
            "let value = buffer.push(0);\n".repeat(100),
            format!(
                "~~~rust\n{}\n~~~",
                "let value = buffer.push(0);\n".repeat(2000)
            ),
            "| repeated item | ready status |\n".repeat(100),
            format!(
                "[\n{}\n]",
                "  \"same repeated prose with spaces\",\n".repeat(100)
            ),
            "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVow".repeat(50),
            format!(
                "```text\n{}\n```",
                "Repeated prose inside an example is legitimate.\n".repeat(2000)
            ),
        ] {
            let mut guard = StreamGuard::default();
            let mut remaining = content.as_str();
            while !remaining.is_empty() {
                let end = remaining.floor_char_boundary(remaining.len().min(97));
                assert!(guard
                    .observe(&ProviderStreamEvent::TextDelta(remaining[..end].into()))
                    .is_none());
                remaining = &remaining[end..];
            }
            assert!(guard
                .observe(&ProviderStreamEvent::Done { usage: None })
                .is_none());
        }
    }

    #[test]
    fn stream_guard_detects_fragmented_unicode_passages_and_short_line_runs() {
        let passage = "Tämä kappale toistuu uudelleen ilman mitään uutta tietoa. ";
        let mut guard = StreamGuard::default();
        let mut reason = None;
        for c in passage.repeat(80).chars() {
            reason = guard.observe(&ProviderStreamEvent::ReasoningDelta(c.to_string()));
            if reason.is_some() {
                break;
            }
        }
        assert!(reason.is_some());
        let mut guard = StreamGuard::default();
        assert!(guard
            .observe(&ProviderStreamEvent::TextDelta(
                "Repeat this line\n".repeat(12)
            ))
            .is_none());
        assert!(guard
            .observe(&ProviderStreamEvent::Done { usage: None })
            .is_some());
    }

    #[test]
    fn stream_guard_keeps_channels_separate_and_detects_a_loop_after_a_large_prefix() {
        let passage = "The stream continues with a sentence that is long enough to be a passage. ";
        let mut guard = StreamGuard::default();
        for _ in 0..12 {
            assert!(guard
                .observe(&ProviderStreamEvent::TextDelta(passage.into()))
                .is_none());
            assert!(guard
                .observe(&ProviderStreamEvent::ReasoningDelta(passage.into()))
                .is_none());
        }
        assert!(guard
            .observe(&ProviderStreamEvent::Done { usage: None })
            .is_none());
        let prefix = numbered(1000, |i| {
            format!("Ordinary prose with a different numbered point {i}. ")
        });
        assert!(guard
            .observe(&ProviderStreamEvent::TextDelta(prefix))
            .is_none());
        assert!(guard
            .observe(&ProviderStreamEvent::TextDelta(passage.repeat(1000)))
            .is_some());
        let mut prose_after_fence = passage.repeat(300);
        prose_after_fence.truncate(TAIL_BYTES - 2);
        let mut guard = StreamGuard::default();
        assert!(guard
            .observe(&ProviderStreamEvent::TextDelta(format!(
                "```text\n{}\n```\n{prose_after_fence}",
                passage.repeat(1000)
            )))
            .is_some());
    }
}
