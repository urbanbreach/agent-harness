use super::*;
use std::collections::BTreeSet;

pub(in crate::coord) struct Plan {
    pub start: usize,
    pub cut: usize,
    pub read_files: Vec<String>,
    pub modified_files: Vec<String>,
}
impl Plan {
    pub fn new(
        context: &Context,
        config: &crate::config::CompactionSettings,
        keep: u32,
        through: Option<&str>,
    ) -> Result<Option<Self>, CoordinatorError> {
        let entries = &context.entries;
        let start = entries
            .iter()
            .position(|e| e.message.role != MessageRole::System)
            .unwrap_or(entries.len());
        let mut boundaries: Vec<_> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                let prior = index.checked_sub(1).and_then(|i| entries.get(i));
                (entry.turn.is_some()
                    && (prior.is_none_or(|e| e.turn != entry.turn)
                        || (config.split_oversized_turns
                            && entry.message.role == MessageRole::Assistant)))
                    .then_some(index)
            })
            .collect();
        let Some(mut cut) = boundaries.pop() else {
            return Ok(None);
        };
        // A newly queued prompt is not a completed turn. Preserve the preceding turn too.
        let pending = entries
            .last()
            .is_some_and(|e| e.message.role == MessageRole::User);
        if !config.split_oversized_turns && pending {
            cut = boundaries.pop().unwrap_or(cut);
        }
        let end = entries.len() - usize::from(pending);
        let mut retained = super::super::context::tokens(&entries[cut..end]);
        while let Some(previous) = boundaries.pop() {
            let next =
                retained.saturating_add(super::super::context::tokens(&entries[previous..cut]));
            if next > keep {
                break;
            }
            retained = next;
            cut = previous;
        }
        if let Some(through) = through {
            let last = entries
                .iter()
                .rposition(|e| e.turn.as_deref() == Some(through))
                .ok_or_else(|| {
                    CoordinatorError::Invalid(
                        "compaction request boundary is not in context".into(),
                    )
                })?;
            cut = cut.min(last + 1);
        }
        if cut <= start || !entries[start..cut].iter().any(|e| e.turn.is_some()) {
            return Ok(None);
        }
        let (mut reads, mut modified) = (BTreeSet::new(), BTreeSet::new());
        for call in entries[start..cut]
            .iter()
            .flat_map(|e| e.message.assistant_tool_calls.iter().flatten())
        {
            let args: serde_json::Value = serde_json::from_str(&call.arguments_json)?;
            let path = ["file_path", "filePath", "path"]
                .iter()
                .find_map(|key| args.get(key).and_then(serde_json::Value::as_str));
            if let Some(path) = path {
                match call.function_name.as_str() {
                    "read" => {
                        reads.insert(path.to_owned());
                    }
                    "write" | "edit" => {
                        modified.insert(path.to_owned());
                    }
                    _ => {}
                }
            }
        }
        Ok(Some(Self {
            start,
            cut,
            read_files: reads.into_iter().collect(),
            modified_files: modified.into_iter().collect(),
        }))
    }
}
