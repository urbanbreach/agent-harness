//! Byte-level identity-span scanners for truthful substitution.
//!
//! Pure string-scanning primitives: locate the next identity span of a given
//! shape at or after a byte offset, refusing to match spans embedded in
//! larger words or in larger dotted numeric runs. Every returned offset is a
//! byte offset that starts and ends on an ASCII character, so the caller may
//! slice the original UTF-8 text without further boundary checks.

/// True when a byte is ASCII alphanumeric or underscore (a "word" byte).
pub(super) fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Locate the next exact `literal` at or after `from`, with no boundary
/// guard. Reserved for distinctive logo glyph sequences whose literal is
/// itself unique identity text and cannot collide with functional content.
pub(super) fn find_exact(text: &str, from: usize, literal: &str) -> Option<(usize, usize)> {
    if literal.is_empty() {
        return None;
    }
    let rel = text.get(from..)?.find(literal)?;
    let start = from + rel;
    Some((start, start + literal.len()))
}

/// Locate the next whole-word occurrence of `literal` at or after `from`.
///
/// An occurrence counts only when neither neighbour is a word byte, so a
/// title such as `Harness` inside `Harnesses`, or a provider/account token
/// embedded in a larger identifier, is never matched. Truthful substitution
/// rewrites only the standalone identity word.
pub(super) fn find_word(text: &str, from: usize, literal: &str) -> Option<(usize, usize)> {
    if literal.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let step = literal.chars().next().map_or(1, char::len_utf8);
    let mut search = from;
    while let Some(slice) = text.get(search..) {
        let rel = slice.find(literal)?;
        let start = search + rel;
        let end = start + literal.len();
        let left_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let right_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if left_ok && right_ok {
            return Some((start, end));
        }
        search = start + step;
    }
    None
}

/// Locate the next SemVer-ish version span at or after `from`.
///
/// Grammar: an optional ASCII `v`/`V`, then `MAJOR.MINOR.PATCH` as runs of
/// ASCII digits, with optional `-prerelease` and `+build` tails drawn from
/// `[0-9A-Za-z.-]`. Dotted-numeric guards on both sides reject spans that
/// sit inside a longer numeric run, so functional values such as IP
/// addresses (`10.0.0.1`), four-component versions (`1.2.3.4`), and token
/// counts (`1.7K`) are never rewritten.
pub(super) fn find_version(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut index = from;
    while index < bytes.len() {
        let byte = bytes[index];
        let version_lead =
            (byte == b'v' || byte == b'V') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit);
        if !byte.is_ascii_digit() && !version_lead {
            index += 1;
            continue;
        }
        if version_left_ok(bytes, index) {
            match parse_semver(bytes, index) {
                Some(end) if version_right_ok(bytes, end) => return Some((index, end)),
                _ => {}
            }
        }
        index += 1;
    }
    None
}

/// Left-boundary guard for a version candidate starting at `start`.
fn version_left_ok(bytes: &[u8], start: usize) -> bool {
    if start == 0 {
        return true;
    }
    let prev = bytes[start - 1];
    if is_word_byte(prev) {
        return false;
    }
    // Reject a dotted numeric run continuing on the left (`10.1.2.3`).
    !(prev == b'.' && start >= 2 && bytes[start - 2].is_ascii_digit())
}

/// Right-boundary guard for a version candidate ending at `end`.
fn version_right_ok(bytes: &[u8], end: usize) -> bool {
    let Some(next) = bytes.get(end).copied() else {
        return true;
    };
    if is_word_byte(next) {
        return false;
    }
    // Reject a dotted numeric run continuing on the right (`1.2.3.4`).
    !(next == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_digit))
}

/// Parse `v?MAJOR.MINOR.PATCH[-prerelease][+build]` from `start`, returning
/// the byte offset immediately past the matched span, if any.
fn parse_semver(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    if bytes
        .get(index)
        .is_some_and(|&byte| byte == b'v' || byte == b'V')
    {
        index += 1;
    }
    index = parse_digits(bytes, index)?;
    index = expect_byte(bytes, index, b'.')?;
    index = parse_digits(bytes, index)?;
    index = expect_byte(bytes, index, b'.')?;
    index = parse_digits(bytes, index)?;
    if bytes.get(index) == Some(&b'-') {
        index = parse_ident(bytes, index + 1)?;
    }
    if bytes.get(index) == Some(&b'+') {
        index = parse_ident(bytes, index + 1)?;
    }
    Some(index)
}

/// Parse a non-empty run of ASCII digits.
fn parse_digits(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    (index > start).then_some(index)
}

/// Consume exactly one expected byte.
fn expect_byte(bytes: &[u8], index: usize, wanted: u8) -> Option<usize> {
    (bytes.get(index) == Some(&wanted)).then_some(index + 1)
}

/// Parse a non-empty prerelease/build body drawn from `[0-9A-Za-z.-]`.
fn parse_ident(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    while bytes
        .get(index)
        .is_some_and(|&byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        index += 1;
    }
    (index > start).then_some(index)
}
