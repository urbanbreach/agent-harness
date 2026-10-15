//! Compaction methods, their default order and the manual command grammar.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One way to reduce context. Automatic maintenance walks these in configured order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompactionMethod {
    /// Provider-native compaction (OpenAI Responses compact, Anthropic summarize) or a custom endpoint.
    Remote,
    /// Older history rendered into dense bitmap frames for vision models; no LLM call.
    Archive,
    /// A handoff document written by the live model from its own cached context.
    Handoff,
    /// Mechanical elision of heavy tool output and large blocks into recoverable artifacts.
    Shake,
    /// A local LLM summary of the older history.
    Soft,
}

impl CompactionMethod {
    pub const DEFAULT_ORDER: [Self; 5] = [
        Self::Remote,
        Self::Archive,
        Self::Handoff,
        Self::Shake,
        Self::Soft,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Archive => "archive",
            Self::Handoff => "handoff",
            Self::Shake => "shake",
            Self::Soft => "soft",
        }
    }

    /// The method with this exact wire name. `archive` is the only spelling of the image method.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::DEFAULT_ORDER
            .into_iter()
            .find(|method| method.as_str() == name)
    }

    /// Tolerant `method_order` parse: a non-array yields the default order, unknown or
    /// non-string entries are discarded and the first duplicate is kept.
    pub fn parse_order(value: &serde_json::Value) -> Vec<Self> {
        let Some(entries) = value.as_array() else {
            return Self::DEFAULT_ORDER.to_vec();
        };
        let parsed: Vec<Self> = entries
            .iter()
            .filter_map(|entry| entry.as_str().and_then(Self::from_name))
            .collect();
        dedupe_order(&parsed)
    }

    /// Whether this method calls a model and can therefore run speculatively.
    pub const fn is_llm_backed(self) -> bool {
        matches!(self, Self::Remote | Self::Handoff | Self::Soft)
    }
}

/// Keep the first occurrence of each method, preserving order.
pub fn dedupe_order(order: &[CompactionMethod]) -> Vec<CompactionMethod> {
    let mut out = Vec::with_capacity(order.len());
    for method in order {
        if !out.contains(method) {
            out.push(*method);
        }
    }
    out
}

/// A per-invocation method override for `/compact`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualMode {
    Soft,
    Remote,
    Archive,
}

impl ManualMode {
    pub const ALL: [Self; 3] = [Self::Soft, Self::Remote, Self::Archive];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Remote => "remote",
            Self::Archive => "archive",
        }
    }

    /// The method order this mode forces for one invocation.
    pub fn forced_order(self) -> Vec<CompactionMethod> {
        match self {
            Self::Soft => vec![CompactionMethod::Soft],
            Self::Remote => vec![CompactionMethod::Remote, CompactionMethod::Soft],
            Self::Archive => vec![CompactionMethod::Archive],
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Soft => "Summarize locally with the active model (skip server compaction)",
            Self::Remote => {
                "Use provider-native server compaction, then fall back to a local summary"
            }
            Self::Archive => {
                "Archive history onto dense bitmap frames the model reads back (no LLM call)"
            }
        }
    }
}

/// Parsed `/compact` arguments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactArgs {
    pub mode: Option<ManualMode>,
    pub focus: Option<String>,
}

pub const ARCHIVE_FOCUS_ERROR: &str =
    "/compact archive does not take focus instructions (it archives history without an LLM summary).";

/// Parse `/compact [soft|remote|archive] [focus]`.
///
/// An unknown first word is not an error: the whole trimmed text becomes focus.
pub fn parse_compact_args(args: &str) -> Result<CompactArgs, &'static str> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(CompactArgs::default());
    }
    let (first, rest) = trimmed
        .split_once(char::is_whitespace)
        .map_or((trimmed, ""), |(first, rest)| (first, rest.trim()));
    let mode = ManualMode::ALL
        .into_iter()
        .find(|mode| mode.as_str().eq_ignore_ascii_case(first));
    let Some(mode) = mode else {
        return Ok(CompactArgs {
            mode: None,
            focus: Some(trimmed.to_owned()),
        });
    };
    let focus = (!rest.is_empty()).then(|| rest.to_owned());
    if mode == ManualMode::Archive && focus.is_some() {
        return Err(ARCHIVE_FOCUS_ERROR);
    }
    Ok(CompactArgs {
        mode: Some(mode),
        focus,
    })
}

/// `/shake` modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShakeMode {
    /// Strip heavy tool results and large fenced/XML blocks into a recovery artifact.
    #[default]
    Elide,
    /// Remove image blocks.
    Images,
    /// Remove reasoning blocks.
    Thinking,
}

impl ShakeMode {
    pub const ALL: [Self; 3] = [Self::Elide, Self::Images, Self::Thinking];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Elide => "elide",
            Self::Images => "images",
            Self::Thinking => "thinking",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Elide => "Strip tool results + large blocks (default)",
            Self::Images => "Strip image blocks",
            Self::Thinking => "Drop all thinking blocks",
        }
    }
}

/// Parse `/shake [elide|images|thinking]`; the whole argument is the mode.
pub fn parse_shake_args(args: &str) -> Result<ShakeMode, String> {
    let verb = args.trim().to_lowercase();
    match verb.as_str() {
        "" | "elide" => Ok(ShakeMode::Elide),
        "images" => Ok(ShakeMode::Images),
        "thinking" => Ok(ShakeMode::Thinking),
        _ => Err(format!(
            "Unknown /shake mode \"{verb}\". Use elide, images, or thinking."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_grammar_matches_reference_cases() {
        let cases: [(&str, Result<CompactArgs, &str>); 7] = [
            ("", Ok(CompactArgs::default())),
            (
                "  SOFT  keep the API notes ",
                Ok(CompactArgs {
                    mode: Some(ManualMode::Soft),
                    focus: Some("keep the API notes".into()),
                }),
            ),
            (
                "remote",
                Ok(CompactArgs {
                    mode: Some(ManualMode::Remote),
                    focus: None,
                }),
            ),
            (
                "softly please",
                Ok(CompactArgs {
                    mode: None,
                    focus: Some("softly please".into()),
                }),
            ),
            (
                "focus on tests",
                Ok(CompactArgs {
                    mode: None,
                    focus: Some("focus on tests".into()),
                }),
            ),
            (
                "archive",
                Ok(CompactArgs {
                    mode: Some(ManualMode::Archive),
                    focus: None,
                }),
            ),
            ("archive the tests", Err(ARCHIVE_FOCUS_ERROR)),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_compact_args(input), expected, "{input:?}");
        }
    }

    #[test]
    fn shake_grammar_rejects_unknown_modes() {
        assert_eq!(parse_shake_args(" "), Ok(ShakeMode::Elide));
        assert_eq!(parse_shake_args("Images"), Ok(ShakeMode::Images));
        assert_eq!(
            parse_shake_args("Blocks"),
            Err("Unknown /shake mode \"blocks\". Use elide, images, or thinking.".into())
        );
    }

    #[test]
    fn order_dedupe_keeps_first_occurrence() {
        use CompactionMethod::{Archive, Shake, Soft};
        assert_eq!(
            dedupe_order(&[Soft, Archive, Soft, Shake, Archive]),
            [Soft, Archive, Shake]
        );
    }

    #[test]
    fn method_order_parse_is_tolerant_and_rejects_foreign_names() {
        use CompactionMethod::{Archive, Remote, Soft};
        for (value, expected) in [
            (
                serde_json::json!(["soft", "bogus", 3, "archive", "soft"]),
                vec![Soft, Archive],
            ),
            (
                serde_json::json!(["remote", "image", "ARCHIVE"]),
                vec![Remote],
            ),
            (serde_json::json!([]), vec![]),
            (
                serde_json::json!("soft"),
                CompactionMethod::DEFAULT_ORDER.to_vec(),
            ),
            (
                serde_json::Value::Null,
                CompactionMethod::DEFAULT_ORDER.to_vec(),
            ),
        ] {
            assert_eq!(CompactionMethod::parse_order(&value), expected, "{value}");
        }
    }
}
