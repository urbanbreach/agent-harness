use super::super::state::ProviderContextCacheKey;
use super::*;
use crate::config::{ModelLimitProvenance, ResolvedModelLimits};
use crate::ids::{EntryId, SessionId};
use crate::session::{CanonicalRuntimeSelection, ProviderViewOwner, RecordSequence};

pub(super) fn canonical_provider_context_cache_rejects_every_stale_identity_dimension() {
    // Given a canonical provider context installed for one exact owner and path state.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let mut run_state = test_run_state(temp_dir.path(), "canonical_provider_context_cache");
    let key = cache_key();
    let context = ProviderContext::from_turns(vec![long_turn("canonical turn", 'C')]);
    run_state.install_canonical_provider_context(key.clone(), context.clone());

    // When any identity-bearing cache dimension changes.
    let mut stale_owner = key.clone();
    stale_owner.owner = ProviderViewOwner::root("agent_000002", SessionId::new("session-root"));
    let mut stale_watermark = key.clone();
    stale_watermark.watermark = Some(RecordSequence::new(8));
    let mut stale_leaf = key.clone();
    stale_leaf.selected_leaf = EntryId::new("entry-other");
    let mut stale_runtime = key.clone();
    stale_runtime.runtime_selection.model_id = "model-b".to_string();

    // Then only the exact canonical key can retrieve the context, and invalidation is atomic.
    assert_eq!(run_state.canonical_provider_context(&key), Some(&context));
    assert_eq!(run_state.canonical_provider_context(&stale_owner), None);
    assert_eq!(run_state.canonical_provider_context(&stale_watermark), None);
    assert_eq!(run_state.canonical_provider_context(&stale_leaf), None);
    assert_eq!(run_state.canonical_provider_context(&stale_runtime), None);
    run_state.invalidate_provider_context("agent_000001");
    assert_eq!(run_state.canonical_provider_context(&key), None);
    assert!(!run_state
        .provider_context_cache_key_by_agent
        .contains_key("agent_000001"));
}

fn cache_key() -> ProviderContextCacheKey {
    ProviderContextCacheKey {
        owner: ProviderViewOwner::root("agent_000001", SessionId::new("session-root")),
        watermark: Some(RecordSequence::new(7)),
        selected_leaf: EntryId::new("entry-active"),
        runtime_selection: CanonicalRuntimeSelection::new(
            Some("profile-a".to_string()),
            "mock",
            "model-a",
            AgentModelSettings::default(),
            ResolvedModelLimits::from_values(
                Some(128_000),
                Some(120_000),
                Some(8_000),
                ModelLimitProvenance::explicit("task-9 cache owner"),
            ),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .unwrap_or_abort(),
    }
}
