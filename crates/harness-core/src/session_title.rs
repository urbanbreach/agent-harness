use std::time::{Duration, UNIX_EPOCH};

use crate::clock::Clock;

const PARENT_TITLE_PREFIX: &str = "New session - ";
const CHILD_TITLE_PREFIX: &str = "Child session - ";

pub const TITLE_OPERATION_TEMPERATURE: f32 = 0.5;
pub const TITLE_GENERATION_USER_PROMPT: &str = "Generate a title for this conversation:\n";
pub const TITLE_OPERATION_SYSTEM_PROMPT: &str = r#"You are a title generator. You output ONLY a thread title. Nothing else.

<task>
Generate a brief title that would help the user find this conversation later.

Follow all rules in <rules>
Use the <examples> so you know what a good title looks like.
Your output must be:
- A single line
- ≤50 characters
- No explanations
</task>

<rules>
- you MUST use the same language as the user message you are summarizing
- Title must be grammatically correct and read naturally - no word salad
- Never include tool names in the title (e.g. "read tool", "bash tool", "edit tool")
- Focus on the main topic or question the user needs to retrieve
- Vary your phrasing - avoid repetitive patterns like always starting with "Analyzing"
- When a file is mentioned, focus on WHAT the user wants to do WITH the file, not just that they shared it
- Keep exact: technical terms, numbers, filenames, HTTP codes
- Remove: the, this, my, a, an
- Never assume tech stack
- Never use tools
- NEVER respond to questions, just generate a title for the conversation
- The title should NEVER include "summarizing" or "generating" when generating a title
- DO NOT SAY YOU CANNOT GENERATE A TITLE OR COMPLAIN ABOUT THE INPUT
- Always output something meaningful, even if the input is minimal.
- If the user message is short or conversational (e.g. "hello", "lol", "what's up", "hey"):
  → create a title that reflects the user's tone or intent (such as Greeting, Quick check-in, Light chat, Intro message, etc.)
</rules>

<examples>
"debug 500 errors in production" → Debugging production 500 errors
"refactor user service" → Refactoring user service
"why is app.js failing" → app.js failure investigation
"implement rate limiting" → Rate limiting implementation
"how do I connect postgres to my API" → Postgres API connection
"best practices for React hooks" → React hooks best practices
"@src/auth.ts can you add refresh token support" → Auth refresh token support
"@utils/parser.ts this is broken" → Parser bug fix
"look at @config.json" → Config review
"@App.tsx add dark mode toggle" → Dark mode toggle in App
</examples>"#;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionTitleOperationSpec {
    pub(crate) model_ref: String,
    pub(crate) temperature: f32,
}

impl SessionTitleOperationSpec {
    pub(crate) fn for_model(model_ref: &str) -> Self {
        Self {
            model_ref: model_ref.to_string(),
            temperature: TITLE_OPERATION_TEMPERATURE,
        }
    }
}

pub fn create_default_title(clock: &(impl Clock + ?Sized), is_child: bool) -> String {
    let prefix = if is_child {
        CHILD_TITLE_PREFIX
    } else {
        PARENT_TITLE_PREFIX
    };
    format!("{prefix}{}", clock_millis_iso(clock))
}

pub fn is_default_title(title: &str) -> bool {
    default_title_timestamp(title, PARENT_TITLE_PREFIX).is_some()
        || default_title_timestamp(title, CHILD_TITLE_PREFIX).is_some()
}

pub fn is_parent_default_title(title: &str) -> bool {
    default_title_timestamp(title, PARENT_TITLE_PREFIX).is_some()
}

pub fn clean_generated_title(text: &str) -> Option<String> {
    let without_think = remove_think_blocks(text);
    let cleaned = without_think
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;

    if cleaned.chars().count() > 100 {
        let mut truncated = cleaned.chars().take(97).collect::<String>();
        truncated.push_str("...");
        Some(truncated)
    } else {
        Some(cleaned.to_string())
    }
}

fn default_title_timestamp<'a>(title: &'a str, prefix: &str) -> Option<&'a str> {
    let timestamp = title.strip_prefix(prefix)?;
    (timestamp.len() == "0000-00-00T00:00:00.000Z".len()
        && timestamp.as_bytes().get(4) == Some(&b'-')
        && timestamp.as_bytes().get(7) == Some(&b'-')
        && timestamp.as_bytes().get(10) == Some(&b'T')
        && timestamp.as_bytes().get(13) == Some(&b':')
        && timestamp.as_bytes().get(16) == Some(&b':')
        && timestamp.as_bytes().get(19) == Some(&b'.')
        && timestamp.as_bytes().get(23) == Some(&b'Z')
        && timestamp
            .chars()
            .enumerate()
            .all(|(idx, ch)| matches!(idx, 4 | 7 | 10 | 13 | 16 | 19 | 23) || ch.is_ascii_digit()))
    .then_some(timestamp)
}

fn remove_think_blocks(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<think>") {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + "<think>".len()..];
        let Some(end) = after_start.find("</think>") else {
            rest = "";
            break;
        };
        rest = after_start[end + "</think>".len()..].trim_start_matches(char::is_whitespace);
    }
    output.push_str(rest);
    output
}

fn clock_millis_iso(clock: &(impl Clock + ?Sized)) -> String {
    let Some(timestamp) = clock.system_time_rfc3339_millis() else {
        return "1970-01-01T00:00:00.000Z".to_string();
    };
    timestamp
}

pub(crate) fn system_time_millis_iso(time: std::time::SystemTime) -> String {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    let total_millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    let millis = total_millis % 1_000;
    let secs = UNIX_EPOCH + Duration::from_secs(total_millis / 1_000);
    let seconds = humantime::format_rfc3339(secs).to_string();
    match seconds.strip_suffix('Z') {
        Some(prefix) => format!("{prefix}.{millis:03}Z"),
        None => seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_generated_title, is_default_title};

    #[test]
    fn recognizes_harness_default_titles() {
        // arrange
        // act
        // assert
        assert!(is_default_title("New session - 2026-05-07T12:34:56.789Z"));
        assert!(is_default_title("Child session - 2026-05-07T12:34:56.789Z"));
        assert!(!is_default_title("New session - 2026-05-07T12:34:56Z"));
        assert!(!is_default_title("interactive"));
    }

    #[test]
    fn cleans_harness_generated_title() {
        // arrange
        // act
        // assert
        assert_eq!(
            clean_generated_title(
                "<think>hidden</think>\n\n  Debugging production 500 errors\nextra"
            ),
            Some("Debugging production 500 errors".to_string())
        );
        assert_eq!(clean_generated_title("\n\n"), None);
    }

    #[test]
    fn truncates_harness_generated_title() {
        // arrange
        // act
        // assert
        let input = "x".repeat(101);
        assert_eq!(
            clean_generated_title(&input),
            Some(format!("{}...", "x".repeat(97)))
        );
    }
}
