//! Named choice lists for settings whose raw value is a number, a list or a free string.
//!
//! Every menu starts with a Default entry that removes the key. The current value is kept
//! selectable: a value outside the list is appended as `Custom: <value>`.

use harness_compaction::CompactionMethod;
use harness_core::config::setting_definition;
use serde_json::Value;

const COMPACTION: &str = "runtime.compaction.";

/// Shape ids offered by the archive shape menu, after `auto`.
const SHAPE_IDS: [&str; 17] = [
    "8x8r-bw",
    "8x8r-sent",
    "8x8u-bw",
    "8x8u-sent",
    "6x6u-bw",
    "6x6u-sent",
    "5x8-bw",
    "5x8-sent",
    "6x12-dim",
    "8x13-bw",
    "8on16-bw",
    "8on22-bw",
    "11on16-bw",
    "silver16-bw",
    "doc-8on16-bw",
    "doc-8on16-sent",
    "doc-8on16-sent-dim",
];

/// One menu entry. `value: None` removes the key so the runtime default applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Preset {
    pub(crate) label: String,
    pub(crate) value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresetMenu {
    pub(crate) presets: Vec<Preset>,
    pub(crate) selected: usize,
}

impl PresetMenu {
    pub(crate) fn labels(&self) -> Vec<String> {
        self.presets
            .iter()
            .map(|preset| preset.label.clone())
            .collect()
    }

    pub(crate) fn selected_preset(&self) -> Option<&Preset> {
        self.presets.get(self.selected)
    }

    pub(crate) fn select(&mut self, index: usize) {
        self.selected = index.min(self.presets.len().saturating_sub(1));
    }

    /// Move by `delta`, wrapping at both ends.
    pub(crate) fn step(&mut self, delta: isize) {
        let len = self.presets.len();
        if len == 0 {
            return;
        }
        let len = isize::try_from(len).unwrap_or(isize::MAX);
        let current = isize::try_from(self.selected).unwrap_or(0);
        self.selected = usize::try_from((current + delta).rem_euclid(len)).unwrap_or(0);
    }
}

fn method_name(method: CompactionMethod) -> &'static str {
    match method {
        CompactionMethod::Remote => "Server compaction",
        CompactionMethod::Archive => "Archive",
        CompactionMethod::Handoff => "Handoff",
        CompactionMethod::Soft => "Soft compaction",
        CompactionMethod::Shake => "Shake",
    }
}

fn shape_available(id: &str) -> bool {
    harness_compaction::archive::resolve_shape("", "", Some(id)).id == id
}

fn method_items() -> Vec<(String, String)> {
    CompactionMethod::DEFAULT_ORDER
        .into_iter()
        .map(|first| {
            let order = std::iter::once(first)
                .chain(
                    CompactionMethod::DEFAULT_ORDER
                        .into_iter()
                        .filter(|method| *method != first),
                )
                .map(|method| Value::from(method.as_str()))
                .collect::<Vec<_>>();
            (
                format!("{} first", method_name(first)),
                Value::Array(order).to_string(),
            )
        })
        .collect()
}

/// `(label, raw value)` pairs for a menu-backed setting.
fn items(id: &str) -> Option<Vec<(String, String)>> {
    let items = match id.strip_prefix(COMPACTION)? {
        "threshold_percent" => [10_u32, 20, 30, 40, 50, 60, 70, 75, 80, 85, 90, 95]
            .into_iter()
            .map(|percent| (format!("{percent}%"), percent.to_string()))
            .collect(),
        "threshold_tokens" => [25_u32, 50, 100, 150, 200, 300, 500]
            .into_iter()
            .map(|thousands| {
                (
                    format!("{thousands}K tokens"),
                    (thousands * 1_000).to_string(),
                )
            })
            .collect(),
        "idle_threshold_tokens" => (1_u32..=9)
            .map(|hundreds| {
                (
                    format!("{hundreds}00K tokens"),
                    (hundreds * 100_000).to_string(),
                )
            })
            .collect(),
        "idle_timeout_seconds" => [60_u32, 120, 300, 600, 1_800, 3_600]
            .into_iter()
            .map(|seconds| (duration_label(seconds.into()), seconds.to_string()))
            .collect(),
        "method_order" => method_items(),
        "archive.system_prompt" => [("None", "none"), ("AGENTS.md", "agents_md"), ("All", "all")]
            .into_iter()
            .map(|(label, value)| (label.to_string(), value.to_string()))
            .collect(),
        "archive.shape" => std::iter::once("auto")
            .chain(SHAPE_IDS)
            .filter(|id| shape_available(id) || *id == "auto")
            .map(|id| (id.to_string(), id.to_string()))
            .collect(),
        _ => return None,
    };
    Some(items)
}

fn is_threshold_override(id: &str) -> bool {
    matches!(
        id.strip_prefix(COMPACTION),
        Some("threshold_percent" | "threshold_tokens")
    )
}

/// Spellings of "no override": absent, `null`, or `-1` for the two threshold overrides.
fn is_unset_marker(id: &str, raw: &str) -> bool {
    raw == "null" || raw.is_empty() || (is_threshold_override(id) && raw == "-1")
}

/// Equal values compare equal regardless of spelling (`80` and `80.0`, reordered whitespace).
fn canonical(id: &str, raw: &str) -> String {
    let raw = raw.trim();
    if id.strip_prefix(COMPACTION) == Some("method_order") {
        return serde_json::from_str::<Value>(raw).map_or_else(
            |_| raw.to_string(),
            |value| {
                CompactionMethod::parse_order(&value)
                    .iter()
                    .map(|method| method.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
    }
    raw.parse::<f64>()
        .map_or_else(|_| raw.to_string(), |number| number.to_string())
}

fn token_label(tokens: u64) -> String {
    if tokens > 0 && tokens.is_multiple_of(1_000_000) {
        format!("{}M tokens", tokens / 1_000_000)
    } else if tokens > 0 && tokens.is_multiple_of(1_000) {
        format!("{}K tokens", tokens / 1_000)
    } else {
        format!("{tokens} tokens")
    }
}

fn duration_label(seconds: u64) -> String {
    let (count, unit) = if seconds > 0 && seconds.is_multiple_of(3_600) {
        (seconds / 3_600, "hour")
    } else if seconds > 0 && seconds.is_multiple_of(60) {
        (seconds / 60, "minute")
    } else {
        (seconds, "second")
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

/// Human display of a stored raw value; unknown settings display verbatim.
pub(crate) fn display_value(id: &str, raw: &str) -> String {
    let raw = raw.trim();
    let Some(suffix) = id.strip_prefix(COMPACTION) else {
        return raw.to_string();
    };
    match suffix {
        "threshold_percent" | "threshold_tokens" if is_unset_marker(id, raw) => "Default".into(),
        "threshold_percent" => raw
            .parse::<f64>()
            .map_or_else(|_| raw.to_string(), |percent| format!("{percent}%")),
        "threshold_tokens" | "idle_threshold_tokens" => raw
            .parse::<u64>()
            .map_or_else(|_| raw.to_string(), token_label),
        "idle_timeout_seconds" => raw
            .parse::<u64>()
            .map_or_else(|_| raw.to_string(), duration_label),
        "method_order" => serde_json::from_str::<Value>(raw).map_or_else(
            |_| raw.to_string(),
            |value| {
                CompactionMethod::parse_order(&value)
                    .into_iter()
                    .map(method_name)
                    .collect::<Vec<_>>()
                    .join(" > ")
            },
        ),
        "archive.system_prompt" => match raw {
            "none" => "None".into(),
            "agents_md" => "AGENTS.md".into(),
            "all" => "All".into(),
            other => other.to_string(),
        },
        _ => raw.to_string(),
    }
}

/// Whether the row opens a preset menu instead of a free text editor.
pub(crate) fn has_menu(id: &str) -> bool {
    items(id).is_some()
}

/// The preset menu for `id`, positioned on the entry matching `current` (the raw stored value).
pub(crate) fn menu(id: &str, current: Option<&str>) -> Option<PresetMenu> {
    let items = items(id)?;
    let default_raw = setting_definition(id).and_then(|definition| definition.default_value);
    let default_canonical = default_raw.map(|raw| canonical(id, raw));
    let mut default_label = None;
    let mut rest = Vec::new();
    for (label, value) in items {
        if default_canonical.as_deref() == Some(canonical(id, &value).as_str()) {
            default_label = Some(format!("{label} (default)"));
        } else {
            rest.push(Preset {
                label,
                value: Some(value),
            });
        }
    }
    let mut presets = vec![Preset {
        label: default_label.unwrap_or_else(|| match default_raw {
            None => "Default".into(),
            Some(raw) => format!("Default ({})", display_value(id, raw)),
        }),
        value: None,
    }];
    presets.extend(rest);

    let current = current
        .map(str::trim)
        .filter(|raw| !is_unset_marker(id, raw));
    let selected = match current {
        None => 0,
        Some(raw) => {
            let wanted = canonical(id, raw);
            if default_canonical.as_deref() == Some(wanted.as_str()) {
                0
            } else if let Some(index) = presets.iter().position(|preset| {
                preset
                    .value
                    .as_deref()
                    .is_some_and(|value| canonical(id, value) == wanted)
            }) {
                index
            } else {
                let shown = if id.strip_prefix(COMPACTION) == Some("method_order") {
                    display_value(id, raw)
                } else {
                    raw.to_string()
                };
                presets.push(Preset {
                    label: format!("Custom: {shown}"),
                    value: Some(raw.to_string()),
                });
                presets.len() - 1
            }
        }
    };
    Some(PresetMenu { presets, selected })
}
