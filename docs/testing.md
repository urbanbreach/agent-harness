# Plan/Build signoff map

This is the canonical verification map for the shipped Plan/Build path.

## What must be true before the roadmap item is marked done

- `build` is the shipped default lane
- `plan` is explicitly read-only and approval-gated
- `plan.exit` hands off clearly into `build`
- docs, README, config, and tests all describe the same workflow
- verification evidence is recorded with remaining gaps called out honestly

## Issue #104 verification path

Run the narrowest useful checks first, then widen only where needed.

### Contract / config checks
- `cargo test -p harness --test bootstrap_profiles`
- `cargo test -p harness --test config_schema_cli`
- `cargo test -p harness-tools --test native_control_plane_tools`

These prove the shipped config keeps `build` as default, preserves plan metadata, and enforces the explicit `plan.exit` handoff contract.

### User-visible lane proof
- `cargo test -p harness-tui app::tests::tool_call_finished_plan_exit_handoff_emits_switch_model_then_submit_prompt`
- `cargo test -p harness-tui --test model_switcher_metadata`

These prove the active profile can move between `plan` and `build`, and that the handoff updates the next-turn lane visibly enough for the user not to guess.

### Shipped example config / signoff coverage
- `cargo test -p harness-testkit live_proxy_e2e`

This keeps the shipped example config honest by checking the blessed provider path, agent/tool surface, and prompt contract.

### Workspace baseline
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Known remaining gaps

- This signoff does not cover future orchestration surfaces such as swarms, Ralph loops, `$` commands, or plugins.
- The Plan/Build proof is focused on the shipped contract, not every future UX polish item on the roadmap.
- PTY/live screenshot drift remains a known harness-testkit concern outside the narrow scope of issue #104; do not treat this issue as a blanket fix for that lane.
