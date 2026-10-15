fn mime_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}
fn token(byte: u8) -> bool {
    mime_word(byte) || b"!#$%&'*^|~".contains(&byte)
}
fn base64(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
}
fn canonical(payload: &str) -> bool {
    if !payload.len().is_multiple_of(4) {
        return false;
    }
    let unpadded = payload.trim_end_matches('=');
    (match payload.len() - unpadded.len() {
        0 => true,
        1 => unpadded.len() % 4 == 3,
        2 => unpadded.len() % 4 == 2,
        _ => false,
    }) && !unpadded.contains('=')
}

fn marker(text: &str, index: usize) -> Option<(usize, usize)> {
    let tail = &text[index..];
    let (prefix, suffix) = if tail.starts_with("[…") {
        ("[…", "ch elided…]")
    } else {
        ("[...", "ch elided...]")
    };
    let tail = tail.strip_prefix(prefix)?;
    let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let count = tail[..digits].parse::<usize>().ok()?;
    if !tail[digits..].starts_with(suffix) {
        return None;
    }
    Some((index + prefix.len() + digits + suffix.len(), count))
}

/// Whitespace as the specified matcher reads it: the Unicode space separators, line
/// separators and the byte order mark, not only ASCII.
fn whitespace(ch: char) -> bool {
    (ch.is_whitespace() && ch != '\u{85}') || ch == '\u{feff}'
}

/// Byte offset after any run of whitespace starting at `from`.
fn skip_whitespace(text: &str, from: usize) -> usize {
    from + text[from..]
        .char_indices()
        .find(|(_, ch)| !whitespace(*ch))
        .map_or(text.len() - from, |(offset, _)| offset)
}

fn marker_payload(text: &str, index: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let start = skip_whitespace(text, index);
    if start == index {
        return None;
    }
    let (end, omitted) = marker(text, start)?;
    let after = skip_whitespace(text, end);
    let mut after = after;
    if after == end {
        return None;
    }
    let suffix = after;
    while bytes.get(after).is_some_and(|b| base64(*b)) {
        after += 1;
    }
    Some((after, omitted.saturating_add(after - suffix)))
}

fn markdown_opener(before: &str) -> Option<usize> {
    let label = before
        .trim_end_matches(char::is_whitespace)
        .strip_suffix("](")?;
    let boundary = label.rfind(['\n', ']']).map_or(0, |n| n + 1);
    let bracket = boundary + label[boundary..].find('[')?;
    Some(if bracket > 0 && label.as_bytes()[bracket - 1] == b'!' {
        bracket - 1
    } else {
        bracket
    })
}

fn parameter_end(bytes: &[u8], mut index: usize) -> usize {
    loop {
        if bytes
            .get(index..index + 8)
            .is_some_and(|s| s.eq_ignore_ascii_case(b";base64,"))
            || bytes.get(index) != Some(&b';')
        {
            return index;
        }
        let mut parameter = index + 1;
        while bytes.get(parameter).is_some_and(|b| token(*b)) {
            parameter += 1;
        }
        if parameter == index + 1 || bytes.get(parameter) != Some(&b'=') {
            return index;
        }
        let value = parameter + 1;
        parameter = value;
        while bytes.get(parameter).is_some_and(|b| token(*b)) {
            parameter += 1;
        }
        if parameter == value {
            return index;
        }
        index = parameter;
    }
}

/// Replace base64 data URLs atomically; strict mode also heals cut archive prefixes.
pub fn elide_data_urls(text: &str, strict: bool) -> String {
    let bytes = text.as_bytes();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut index = 0;
    while index + 5 <= bytes.len() {
        if !bytes[index..index + 5].eq_ignore_ascii_case(b"data:") {
            index += 1;
            continue;
        }
        let start = index;
        index += 5;
        let mime_start = index;
        if !bytes.get(index).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(|b| mime_word(*b)) {
            index += 1;
        }
        if bytes.get(index) != Some(&b'/') {
            continue;
        }
        index += 1;
        let subtype = index;
        while bytes.get(index).is_some_and(|b| mime_word(*b)) {
            index += 1;
        }
        if index == subtype {
            continue;
        }
        index = parameter_end(bytes, index);
        if !bytes
            .get(index..index + 8)
            .is_some_and(|s| s.eq_ignore_ascii_case(b";base64,"))
        {
            continue;
        }
        let mime_end = index;
        index += 8;
        let payload_start = index;
        while bytes.get(index).is_some_and(|b| base64(*b)) {
            index += 1;
        }
        let mut count = index - payload_start;
        let mut marker_present = false;
        if let Some((end, extra)) = marker_payload(text, index) {
            count = count.saturating_add(extra);
            index = end;
            marker_present = true;
        }
        if !strict && !marker_present && count < 40 && !canonical(&text[payload_start..index]) {
            continue;
        }
        let close = skip_whitespace(text, index);
        let closed = bytes.get(close) == Some(&b')');
        let before = &text[cursor..start];
        let opener = markdown_opener(before);
        if closed {
            if let Some(open) = opener {
                output.push_str(&before[..open]);
            } else {
                output.push_str(before);
            }
        } else {
            output.push_str(before);
        }
        output.push_str(&format!(
            "[data URL omitted: {}, {count} base64 chars]",
            &text[mime_start..mime_end]
        ));
        if closed && opener.is_some() {
            index = close + 1;
        }
        cursor = index;
    }
    output.push_str(&text[cursor..]);
    output
}
