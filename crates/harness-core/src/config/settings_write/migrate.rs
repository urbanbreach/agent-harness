//! Rewrites of persisted values whose meaning changed.
use super::*;

/// Bare numbers in the threshold maps used to be percentages. They now mean base-window tokens
/// in the model map and trigger tokens in the agent map, so integers from 1 to 100 become
/// `"N%"` strings before the file is saved. Larger numbers are already token counts.
pub(super) fn threshold_numbers(root: &mut Value) {
    for map in ["model_thresholds", "agent_thresholds"] {
        let Some(entries) = root
            .pointer_mut(&format!("/runtime/compaction/{map}"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        for value in entries.values_mut() {
            if let Some(percent) = threshold::legacy_percent(value) {
                *value = Value::String(format!("{percent}%"));
            }
        }
    }
}
