# Testing and signoff map

Use the narrowest test that proves a change, then run the workspace gates before release:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Drift checks

- `cargo test -p harness --test config_docs_reference` verifies public config docs against generated schemas.
- `cargo test -p harness --test event_docs_reference` verifies `docs/architecture.md` lists every `EventV1` variant from `harness-core`.

## Coordinator loop contracts

- `cargo test -p harness-core` covers event-sourced coordinator scheduling, provider-call identity,
  source-order model-context projection for parallel tool batches, cancellation/late-result handling,
  and old-log replay compatibility.
- `cargo test -p harness --test prompt_cli` covers headless prompt completion waiting for correlated
  agent-turn terminal lifecycle events rather than stopping at provider finish.
- `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order`
  covers `task`/`batch` re-entry through coordinator scheduling, lineage preservation, permission
  handling, batch limits, and input-order result reporting.

## Provider context compaction regressions

- `cargo test -p harness-core deterministic_summary_uses_required_harness_sections`
- `cargo test -p harness-core model_summary_validation_rejects_missing_required_harness_section`
- `cargo test -p harness-core compaction_trigger_pre_prompt_uses_estimate_without_provider_usage`
- `cargo test -p harness-core compaction_trigger_uses_fallback_budget_without_model_metadata`
- `cargo test -p harness-core failed_turn_context`
- `cargo test -p harness-core failed_terminal_compaction_preserves_original_failure`
- `cargo test -p harness-core split_oversized_turn`
- `cargo test -p harness-core operational_memory`
- `cargo test -p harness --test config_schema_cli public_runtime_config_accepts_new_compaction_settings`
- `cargo test -p harness --test config_schema_cli public_runtime_config_accepts_compaction_settings`
- `cargo test -p harness --test config_docs_reference`
- `cargo test -p harness --test event_docs_reference`
- `cargo test -p harness-core conversation_projection_failed_checkpoint_turn_status`
- `cargo test -p harness-core --test resume_plan session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts`

## Live parity lanes

The live proxy signoff order and required environment variables are documented in
`crates/harness-testkit/tests/README.live-proxy.md`. Keep that file and this map aligned when adding
CLI/TUI parity coverage or documenting known live-coverage gaps.
