use super::{CompletionSource, CompletionTrigger};

/// Active trigger precedence is explicit and stable: slash, file, shell, history.
const PRECEDENCE: [(CompletionSource, u8); 4] = [
    (CompletionSource::Slash, 4),
    (CompletionSource::File, 3),
    (CompletionSource::Shell, 2),
    (CompletionSource::History, 1),
];

/// Returns the source order used when multiple triggers are active at once.
pub const fn precedence_table() -> &'static [(CompletionSource, u8); 4] {
    &PRECEDENCE
}

/// Chooses the highest-precedence trigger from simultaneously detected candidates.
pub fn choose_preferred_trigger(triggers: &[CompletionTrigger]) -> Option<CompletionTrigger> {
    triggers
        .iter()
        .max_by_key(|trigger| source_rank(trigger.source))
        .cloned()
}

const fn source_rank(source: CompletionSource) -> u8 {
    match source {
        CompletionSource::Slash => 4,
        CompletionSource::File => 3,
        CompletionSource::Shell => 2,
        CompletionSource::History => 1,
    }
}
