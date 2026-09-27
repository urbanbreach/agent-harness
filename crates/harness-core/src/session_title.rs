use crate::clock::Clock;

pub const TITLE_OPERATION_TEMPERATURE: f32 = 0.5;
pub const TITLE_GENERATION_USER_PROMPT: &str = "Generate a title for this conversation:\n";
pub const TITLE_OPERATION_SYSTEM_PROMPT: &str = "Return only a brief, single-line conversation title in the user's language. Describe the main task, preserve useful technical names, and use no more than 50 characters. Do not answer the request or call tools.";
const PARENT: &str = "New session - ";
const CHILD: &str = "Child session - ";
pub fn create_default_title(clock: &(impl Clock + ?Sized), is_child: bool) -> String {
    let time = clock
        .system_time_rfc3339_millis()
        .and_then(|s| humantime::parse_rfc3339(&s).ok())
        .unwrap_or(std::time::UNIX_EPOCH);
    format!(
        "{}{}",
        if is_child { CHILD } else { PARENT },
        humantime::format_rfc3339_millis(time)
    )
}
pub fn is_default_title(title: &str) -> bool {
    default_with_prefix(title, PARENT) || default_with_prefix(title, CHILD)
}
pub fn is_parent_default_title(title: &str) -> bool {
    default_with_prefix(title, PARENT)
}
fn default_with_prefix(title: &str, prefix: &str) -> bool {
    title.strip_prefix(prefix).is_some_and(|time| {
        time.len() == 24
            && time.as_bytes().get(19) == Some(&b'.')
            && time.ends_with('Z')
            && humantime::parse_rfc3339(time).is_ok()
    })
}
pub fn clean_generated_title(text: &str) -> Option<String> {
    let text = if text.contains("<think>") {
        text.rsplit_once("</think>")?.1
    } else {
        text
    };
    if text.contains("<think>") || text.contains("</think>") {
        return None;
    }
    let line = text.lines().map(str::trim).find(|s| !s.is_empty())?;
    if line.chars().any(char::is_control) {
        return None;
    }
    if line.chars().count() > 100 {
        Some(format!("{}...", line.chars().take(97).collect::<String>()))
    } else {
        Some(line.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn title_cleanup_keeps_reasoning_and_controls_out_of_session_metadata() {
        let title = create_default_title(&crate::clock::FakeClock::new(), false);
        assert_eq!(title, "New session - 1970-01-01T00:00:00.000Z");
        assert!(is_parent_default_title(&title));
        assert!(!is_parent_default_title("New session - not a timestamp"));
        assert!(!is_parent_default_title(
            "New session - 2026-09-26T12:00:00Z"
        ));
        assert_eq!(
            clean_generated_title("<think>private\nreasoning</think>\n Fix the parser\nextra text"),
            Some("Fix the parser".into())
        );
        assert_eq!(clean_generated_title("<think>unfinished reasoning"), None);
        assert_eq!(clean_generated_title("\u{1b}[31munsafe"), None);
        assert_eq!(
            clean_generated_title(&"文".repeat(120)).map(|s| s.chars().count()),
            Some(100)
        );
    }
}
