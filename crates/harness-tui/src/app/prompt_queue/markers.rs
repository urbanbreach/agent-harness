//! Image markers inside queued prompt text.
//!
//! The composer renders an attached image as `[attachment:N]`. The numbered display grammar
//! `[Image #N]` and `[Image #N, WxH]` is understood as well, and the optional `, WxH` size
//! metadata is kept verbatim when a marker is renumbered.

/// One rewritten marker, positioned in characters of the original text.
pub(super) struct MarkerEdit {
    pub(super) start: usize,
    pub(super) old_len: usize,
    pub(super) new_len: usize,
}

struct Marker<'a> {
    prefix: &'a str,
    number: u64,
    size: &'a str,
    len: usize,
}

const PREFIXES: [&str; 2] = ["[Image #", "[attachment:"];

fn parse_marker(text: &str) -> Option<Marker<'_>> {
    let prefix = PREFIXES.iter().find(|prefix| text.starts_with(**prefix))?;
    let rest = &text[prefix.len()..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let number = rest[..digits].parse().ok()?;
    let tail = &rest[digits..];
    let size = tail
        .strip_prefix(", ")
        .and_then(|size| {
            let width = size.bytes().take_while(u8::is_ascii_digit).count();
            let size = size[width..].strip_prefix('x')?;
            let height = size.bytes().take_while(u8::is_ascii_digit).count();
            (width > 0 && height > 0).then_some(2 + width + 1 + height)
        })
        .unwrap_or(0);
    let size_text = &tail[..size];
    tail[size..].starts_with(']').then_some(Marker {
        prefix,
        number,
        size: size_text,
        len: prefix.len() + digits + size + 1,
    })
}

/// Rewrites every marker, all occurrences, whose number `remap` answers for. `None` leaves
/// the marker alone, `Some(None)` removes it, `Some(Some(n))` renumbers it.
pub(super) fn rewrite_markers(
    text: &str,
    remap: impl Fn(u64) -> Option<Option<u64>>,
) -> (String, Vec<MarkerEdit>) {
    let mut output = String::with_capacity(text.len());
    let mut edits = Vec::new();
    let mut chars = 0;
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if let Some(marker) = parse_marker(rest)
            && let Some(replacement) = remap(marker.number)
        {
            let start = output.len();
            if let Some(number) = replacement {
                output.push_str(marker.prefix);
                output.push_str(&number.to_string());
                output.push_str(marker.size);
                output.push(']');
            }
            edits.push(MarkerEdit {
                start: chars,
                old_len: marker.len,
                new_len: output.len() - start,
            });
            chars += marker.len;
            index += marker.len;
            continue;
        }
        let Some(character) = rest.chars().next() else {
            break;
        };
        output.push(character);
        chars += 1;
        index += character.len_utf8();
    }
    (output, edits)
}

/// Maps a character position of the original text to the rewritten text.
pub(super) fn shifted_position(edits: &[MarkerEdit], position: usize) -> usize {
    let mut shifted = position;
    for edit in edits {
        if edit.start + edit.old_len <= position {
            shifted = shifted + edit.new_len - edit.old_len;
        }
    }
    shifted
}
