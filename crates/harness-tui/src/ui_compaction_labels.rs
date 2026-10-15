//! Loader text of a running maintenance pass, shared by the status row and the live card.
use crate::app::AppState;

/// Spec label of the running pass with the configured interrupt key. Without lifecycle
/// knowledge of the pass (older coordinator) the pass is a plain compaction.
pub(super) fn running_label(app: &AppState, ascii: bool) -> String {
    app.maintenance_loader_label(ascii).unwrap_or_else(|| {
        let ellipsis = if ascii { "..." } else { "…" };
        if app.current_subagent_session_present() {
            format!("Compacting context{ellipsis}")
        } else {
            format!(
                "Compacting context{ellipsis} ({} to cancel)",
                app.interrupt_key_hint()
            )
        }
    })
}

/// Short form for narrow rows or rows that also carry a preview.
pub(super) fn running_label_short(app: &AppState) -> String {
    if app.current_subagent_session_present() {
        "Compacting".to_owned()
    } else {
        format!("Compacting ({} to cancel)", app.interrupt_key_hint())
    }
}
