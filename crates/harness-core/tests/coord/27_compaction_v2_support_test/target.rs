use super::*;
use harness_core::config::{ModelLimitProvenance, ResolvedModelLimits, ResolvedModelTarget};
use harness_core::model_resolution::ModelResolution;

pub(crate) fn journal_hash(run: &RunInfo) -> blake3::Hash {
    blake3::hash(&fs::read(&run.events_path).unwrap_or_abort())
}

pub(crate) fn compaction_v2_target(model: &str, input: u32, output: u32) -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: format!("mock:{model}"),
        provider: "mock".to_string(),
        model: model.to_string(),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        limits: ResolvedModelLimits::from_values(
            Some(input.saturating_add(output)),
            Some(input),
            Some(output),
            ModelLimitProvenance::explicit("G006 RED fixture"),
        ),
        resolution: ModelResolution::default(),
        catalog_entry: None,
    }
}
