//! Protection matchers and tool-call pairing shared by every reducer.
use crate::item::{Item, ItemKind};
use harness_providers::AssistantToolCall;
use std::{collections::BTreeMap, ops::Range, sync::Arc};

type Predicate = dyn Fn(&Item<'_>, Option<&AssistantToolCall>) -> bool + Send + Sync;

/// One way to protect a tool result from reduction.
///
/// Either an exact tool name, or a predicate over the result and its paired call (absent
/// when the call is outside the known history). A result is protected when any matcher in
/// the list matches.
#[derive(Clone)]
pub struct ToolMatcher(Matcher);

#[derive(Clone)]
enum Matcher {
    Name(Arc<str>),
    Predicate(Arc<Predicate>),
}

impl std::fmt::Debug for ToolMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Matcher::Name(name) => write!(f, "ToolMatcher::Name({name:?})"),
            Matcher::Predicate(_) => f.write_str("ToolMatcher::Predicate"),
        }
    }
}

impl ToolMatcher {
    /// Matches results whose tool name equals `name` exactly.
    pub fn name(name: impl Into<Arc<str>>) -> Self {
        Self(Matcher::Name(name.into()))
    }

    pub fn predicate(
        predicate: impl Fn(&Item<'_>, Option<&AssistantToolCall>) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self(Matcher::Predicate(Arc::new(predicate)))
    }

    /// Results of the `skill` tool.
    pub fn skill_tool() -> Self {
        Self::name("skill")
    }

    /// `read` results whose paired call targets a `skill://` location.
    pub fn skill_read() -> Self {
        Self::predicate(|item, call| read_path(item, call).as_deref().is_some_and(is_skill_path))
    }

    /// Recovery reads: a `read` of an `artifact://` location, or any result the tool marked
    /// as an internal recovery artifact.
    pub fn artifact_recovery() -> Self {
        Self::predicate(|item, call| {
            item.tool.internal_artifact
                || read_path(item, call).is_some_and(|path| path.starts_with("artifact://"))
        })
    }

    pub fn matches(&self, item: &Item<'_>, call: Option<&AssistantToolCall>) -> bool {
        match &self.0 {
            Matcher::Name(name) => item.message.name.as_deref() == Some(&**name),
            Matcher::Predicate(predicate) => predicate(item, call),
        }
    }
}

fn is_skill_path(path: &str) -> bool {
    path.starts_with("skill://")
}

/// Whether any matcher protects the result.
pub fn protects(
    matchers: &[ToolMatcher],
    item: &Item<'_>,
    call: Option<&AssistantToolCall>,
) -> bool {
    matchers.iter().any(|matcher| matcher.matches(item, call))
}

/// The protection every reducer preset starts from: `skill` results and skill reads.
pub fn skill_protection() -> Vec<ToolMatcher> {
    vec![ToolMatcher::skill_tool(), ToolMatcher::skill_read()]
}

/// The path a `read` result's paired call asked for. Requires the result tool and the call
/// to both be `read`; accepts the argument spellings the read tool accepts.
pub fn read_path(item: &Item<'_>, call: Option<&AssistantToolCall>) -> Option<String> {
    if item.message.name.as_deref() != Some("read") {
        return None;
    }
    read_target(call)
}

pub(crate) fn read_target(call: Option<&AssistantToolCall>) -> Option<String> {
    let call = call.filter(|call| call.function_name == "read")?;
    let args: serde_json::Value = serde_json::from_str(&call.arguments_json).ok()?;
    ["filePath", "file_path", "path"].iter().find_map(|key| {
        args.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    })
}

/// Tool calls of the assistant entries in `range`; a later duplicate id replaces an earlier
/// one. The half-open interval keeps calls outside it from pairing.
pub fn call_index<'a>(
    items: &'a [Item<'a>],
    range: Range<usize>,
) -> BTreeMap<&'a str, &'a AssistantToolCall> {
    let mut map = BTreeMap::new();
    for item in items.iter().take(range.end).skip(range.start) {
        if item.is_assistant() {
            for call in item.message.assistant_tool_calls.iter().flatten() {
                map.insert(call.tool_call_id.as_str(), call);
            }
        }
    }
    map
}

/// Call lookup for a reducer that considers `items[boundary..]`. Calls inside the sent
/// region win; calls in the excluded prefix resolve only when the sent region has none.
pub(crate) struct Calls<'a> {
    sent: BTreeMap<&'a str, &'a AssistantToolCall>,
    excluded: BTreeMap<&'a str, &'a AssistantToolCall>,
}

impl<'a> Calls<'a> {
    pub(crate) fn new(items: &'a [Item<'a>], boundary: usize) -> Self {
        Self {
            sent: call_index(items, boundary..items.len()),
            excluded: call_index(items, 0..boundary),
        }
    }

    /// Every call across the whole history, later duplicates winning.
    pub(crate) fn whole(items: &'a [Item<'a>]) -> Self {
        Self {
            sent: call_index(items, 0..items.len()),
            excluded: BTreeMap::new(),
        }
    }

    pub(crate) fn paired(&self, item: &Item<'_>) -> Option<&'a AssistantToolCall> {
        let id = item.message.tool_call_id.as_deref()?;
        self.sent.get(id).or_else(|| self.excluded.get(id)).copied()
    }
}

/// Whether a result may be mutated at all.
pub(crate) fn eligible(
    item: &Item<'_>,
    call: Option<&AssistantToolCall>,
    protected: &[ToolMatcher],
) -> bool {
    item.is_tool_result() && !item.tool.pruned && !protects(protected, item, call)
}

/// Whether the entry counts toward whole-message suffix budgets (message entries only).
pub(crate) fn counts_toward_suffix(item: &Item<'_>) -> bool {
    item.kind == ItemKind::Message
}
