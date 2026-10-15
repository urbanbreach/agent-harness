use super::classes::Class;

pub(super) type Revision = (u32, u32, u32);

/// Revision of a lowercase, trimmed, collapsed id for the classes that have a
/// revision grammar; every other class has none.
pub(super) fn parse(class: Class, id: &str) -> Option<Revision> {
    let rest = match class {
        Class::Anthropic => after(id, "claude-")?,
        Class::DeepSeek => after(id, "deepseek-v")?,
        Class::Glm => after(id, "glm-")?,
        Class::Qwen => id.strip_prefix("qwen")?,
        Class::OpenAi => openai_rest(id)?,
        Class::Gemini => after(id, "gemini-")?,
        Class::Xai => after(id, "grok-")?,
        _ => return None,
    };
    components(rest)
}

fn after<'a>(id: &'a str, prefix: &str) -> Option<&'a str> {
    id.find(prefix).map(|at| &id[at + prefix.len()..])
}

/// Leading `chatgpt-`, then `gpt-` anywhere, then a leading `o`; bare `o1`,
/// `o3` and `o4` have no revision.
fn openai_rest(id: &str) -> Option<&str> {
    if let Some(rest) = id.strip_prefix("chatgpt-") {
        Some(rest)
    } else if let Some(rest) = after(id, "gpt-") {
        Some(rest)
    } else if matches!(id, "o1" | "o3" | "o4") {
        None
    } else {
        id.strip_prefix('o')
    }
}

/// Up to three dot or hyphen separated components, each 0..=255, starting at the
/// first ASCII digit. A component immediately followed by a letter is a size
/// token and is not consumed; earlier components are kept.
fn components(rest: &str) -> Option<Revision> {
    let bytes = rest.as_bytes();
    let mut at = bytes.iter().position(u8::is_ascii_digit)?;
    let mut values = [0u32; 3];
    let mut count = 0;
    while count < 3 {
        let end = bytes[at..]
            .iter()
            .position(|b| !b.is_ascii_digit())
            .map_or(bytes.len(), |n| at + n);
        if bytes.get(end).is_some_and(u8::is_ascii_alphabetic) {
            break;
        }
        let Some(value) = rest[at..end]
            .parse::<u32>()
            .ok()
            .filter(|&value| value <= 255)
        else {
            break;
        };
        values[count] = value;
        count += 1;
        let separated = matches!(bytes.get(end), Some(b'.' | b'-'));
        if !separated || !bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
            break;
        }
        at = end + 1;
    }
    (count > 0).then_some(values.into())
}
