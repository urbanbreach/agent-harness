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

## Live parity lanes

The live proxy signoff order and required environment variables are documented in
`crates/harness-testkit/tests/README.live-proxy.md`. Keep that file and this map aligned when adding
CLI/TUI parity coverage or documenting known live-coverage gaps.
