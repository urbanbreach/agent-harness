//! Output caps, chronological folding and overflow budget reduction.
use crate::{
    serialize::{serialize_for_summary, truncate_units, Dialect},
    tokenizer::Counter,
    Item,
};
use std::ops::Range;

/// Fallback text for an empty history without a carried summary.
pub const NO_PRIOR_HISTORY: &str = "No prior history.";
/// Main summary output cap, eighty percent of the reserve up to 16384.
pub fn summary_max_tokens(reserve: u32) -> u32 {
    (reserve / 5 * 4 + reserve % 5 * 4 / 5).min(16_384)
}
/// Split-turn prefix output cap, half the reserve up to 16384.
pub fn turn_prefix_max_tokens(reserve: u32) -> u32 {
    (reserve / 2).min(16_384)
}
/// Display-summary cap, one fifth of the reserve up to 512.
pub fn short_summary_max_tokens(reserve: u32) -> u32 {
    (reserve / 5).min(512)
}
fn window_tokens(window: Option<u32>) -> u32 {
    window.filter(|window| *window > 0).unwrap_or(200_000)
}
/// Scaled minimum conversation budget; unknown windows use 200000.
pub fn min_summary_input(window: Option<u32>) -> u32 {
    (window_tokens(window) / 8).clamp(1024, 16_384)
}
/// Conversation budget after fixed prompt and output reserves.
pub fn summary_input_budget(window: Option<u32>, max_tokens: u32) -> u32 {
    let window = window_tokens(window);
    let available = (window / 5 * 4 + window % 5 * 4 / 5)
        .saturating_sub(max_tokens)
        .saturating_sub(16_384);
    available.max(min_summary_input(Some(window)))
}
/// Chronological whole-message windows over a history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub windows: Vec<Range<usize>>,
    /// The serialization of the whole history when it fits one window, so the caller does
    /// not serialize it a second time.
    pub whole: Option<String>,
}
/// Plan chronological whole-message windows, retaining oversized messages intact.
///
/// A history the tokenizer budget probe proves fits becomes one window carrying its
/// serialization; an empty history is one empty window (a previous summary can still need
/// updating). Otherwise each message is counted independently and a window closes before
/// the message that would exceed the budget. Empty and zero-token messages never force a
/// cut.
pub fn plan_windows(items: &[Item<'_>], dialect: Dialect, budget: u32, counter: &Counter) -> Plan {
    let whole = serialize_for_summary(items, dialect);
    if counter
        .probe_budget([whole.as_str()], i64::from(budget))
        .fits
    {
        return Plan {
            windows: std::iter::once(0..items.len()).collect(),
            whole: Some(whole),
        };
    }
    let mut windows = Vec::new();
    let mut start = 0;
    let mut used = 0_u32;
    for (index, item) in items.iter().enumerate() {
        let text = serialize_for_summary(std::slice::from_ref(item), dialect);
        let tokens = counter.count_text(&text);
        if used > 0 && used.saturating_add(tokens) > budget {
            windows.push(start..index);
            start = index;
            used = 0;
        }
        used = used.saturating_add(tokens);
    }
    windows.push(start..items.len());
    Plan {
        windows,
        whole: None,
    }
}
/// Keep a proportional UTF-16 prefix with the exact truncation notice.
pub fn clamp_to_budget(text: &str, budget: u32, tokens: u32) -> String {
    if tokens <= budget || tokens == 0 {
        return text.to_owned();
    }
    let length = text.encode_utf16().count();
    // u128 keeps the proportional prefix calculation exact for large transcripts.
    let keep = (length as u128 * u128::from(budget) * 95 / (u128::from(tokens) * 100)).max(1024);
    truncate_units(text, usize::try_from(keep).unwrap_or(usize::MAX))
}
/// Reduce only the failed window budget, stopping below the minimum.
pub fn halved_budget(window_budget: u32, sent_tokens: u32, min: u32) -> Option<u32> {
    let half = window_budget.min(sent_tokens) / 2;
    (half >= min && half < window_budget).then_some(half)
}
/// Join history and prefix summaries with the split-turn context divider.
pub fn merge_split(history: &str, prefix: &str) -> String {
    format!("{history}\n\n---\n\n**Turn Context (split turn):**\n\n{prefix}")
}
/// Allow the auto tool-choice lane only for the specific HTTP 400 rejection.
pub fn handoff_tool_choice_fallback(status: Option<u16>, message: &str) -> bool {
    status == Some(400)
        && ["tool_choice", "auto", "supported"].iter().all(|required| {
            message
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|word| word.eq_ignore_ascii_case(required))
        })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Canonical reasoning effort order used when clamping model allowances.
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}
impl Effort {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }
}
/// Resolve off/inherit/default effort against the declared supported ladder.
pub fn resolve_effort(
    requested: Option<&str>,
    supported: &[String],
    reasoning_capable: bool,
) -> Option<String> {
    if !reasoning_capable || supported.is_empty() {
        return None;
    }
    let request = requested.unwrap_or("high").to_ascii_lowercase();
    if request == "off" {
        return None;
    }
    let request = if request == "inherit" {
        "high"
    } else {
        request.as_str()
    };
    if let Some(exact) = supported.iter().find(|level| level.as_str() == request) {
        return Some(exact.clone());
    }
    let requested = Effort::parse(request)?;
    supported
        .iter()
        .rev()
        .find(|level| Effort::parse(level).is_some_and(|effort| effort <= requested))
        .or_else(|| supported.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_and_scaled_input_budgets() {
        for (reserve, expected) in [
            (150_000, (16_384, 16_384, 512)),
            (10_000, (8000, 5000, 512)),
            (3, (2, 1, 0)),
            (u32::MAX, (16_384, 16_384, 512)),
        ] {
            assert_eq!(
                (
                    summary_max_tokens(reserve),
                    turn_prefix_max_tokens(reserve),
                    short_summary_max_tokens(reserve)
                ),
                expected
            );
        }
        for (window, minimum, budget) in [
            (None, 16_384, 127_232),
            (Some(40_000), 5000, 5000),
            (Some(0), 16_384, 127_232),
            (Some(4096), 1024, 1024),
        ] {
            assert_eq!(
                (
                    min_summary_input(window),
                    summary_input_budget(window, 16_384)
                ),
                (minimum, budget)
            );
        }
    }
    #[test]
    fn overflow_clamp_and_halving_stop_at_minimum() {
        let text = "x".repeat(4000);
        assert_eq!(
            clamp_to_budget(&text, 500, 1000),
            format!(
                "{}\n\n[... 2100 more characters truncated]",
                "x".repeat(1900)
            )
        );
        assert_eq!(clamp_to_budget("short", 1, 10), "short");
        for (window, sent, min, expected) in [
            (4000, 3000, 1024, Some(1500)),
            (2000, 9000, 1024, None),
            (4096, 2048, 1024, Some(1024)),
        ] {
            assert_eq!(halved_budget(window, sent, min), expected);
        }
    }
    #[test]
    fn fallback_requires_status_and_whole_words() {
        for (status, text, expected) in [
            (400, "TOOL_CHOICE: only auto is supported", true),
            (403, "tool_choice auto supported", false),
            (400, "tool_choice automatic supported", false),
            (400, "tool_choice auto unsupported", false),
        ] {
            assert_eq!(handoff_tool_choice_fallback(Some(status), text), expected);
        }
    }
    #[test]
    fn effort_defaults_and_clamps_in_declared_order() {
        let supported = vec!["low".to_owned(), "high".to_owned()];
        for (request, capable, expected) in [
            (None, true, Some("high")),
            (Some("Inherit"), true, Some("high")),
            (Some("Off"), true, None),
            (Some("Low"), true, Some("low")),
            (Some("medium"), true, Some("low")),
            (Some("minimal"), true, Some("low")),
            (Some("High"), false, None),
        ] {
            assert_eq!(
                resolve_effort(request, &supported, capable).as_deref(),
                expected
            );
        }
    }

    #[test]
    fn windows_keep_message_boundaries_and_oversized_messages() {
        use crate::serialize::tests::item;
        use harness_providers::{CompletionMessage, MessageRole};
        let messages = [
            CompletionMessage::text(MessageRole::User, "x".repeat(100)),
            CompletionMessage::text(MessageRole::System, ""),
            CompletionMessage::text(MessageRole::User, "y".repeat(400)),
            CompletionMessage::text(MessageRole::User, "z".repeat(100)),
        ];
        let items: Vec<_> = messages.iter().map(item).collect();
        let counter = Counter::new(None);
        let one = |end: usize| -> Vec<Range<usize>> { std::iter::once(0..end).collect() };
        for (budget, windows, whole) in [(1000, one(4), true), (50, vec![0..2, 2..3, 3..4], false)]
        {
            let plan = plan_windows(&items, Dialect::Default, budget, &counter);
            assert_eq!((plan.windows, plan.whole.is_some()), (windows, whole));
        }
        // An empty history is one empty window so a previous summary can still be updated.
        let empty = plan_windows(&[], Dialect::Default, 10, &counter);
        assert_eq!((empty.windows, empty.whole.as_deref()), (one(0), Some("")));
        assert_eq!(
            merge_split("history", "prefix"),
            "history\n\n---\n\n**Turn Context (split turn):**\n\nprefix"
        );
    }
}
