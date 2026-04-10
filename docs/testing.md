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
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact`
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact`

These keep the shipped example config honest by checking the blessed provider path, agent/tool surface, prompt contract, and TUI parity wrappers that `README.live-proxy.md` documents for the canonical signoff map.

## Issue #105 verification path

Run the narrowest useful checks first, then widen only where needed.

### Focused transcript-thinking checks
- `cargo test -p harness-tui lib_tests::transcript_reasoning_precedes_answer_and_tool_rows -- --exact`
- `cargo test -p harness-tui lib_tests::transcript_shell_remains_scannable_without_bubble_cards -- --exact`
- `cargo test -p harness-tui lib_tests::nested_transcript_rows_preserve_prefix_on_wrapped_continuations -- --exact`
- `cargo test -p harness-tui lib_tests::thinking_visibility_toggle_hides_and_restores_inline_thinking_rows -- --exact`
- `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e native_tool_parity_pty_lane -- --exact`

If the PTY lane fails with unchanged markers/focus region and only snapshot/hash drift, report that as the current known PTY drift unless the thinking-row contract intentionally changed.

### Visible transcript live follow-up
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact`
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact`
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact`

Broader Batch 1 parity wrappers remain out of scope for issue #105. If the live environment is unavailable, call that out explicitly as a remaining risk instead of silently skipping it.

## Issue #106 verification path

Reproduce the alias/setup blocker first, then prove the prepared config stays runtime-loadable before relying on live signoff lanes.

### Focused prepared-config regression
- `cargo test -p harness-testkit prepared_restricted_tools_config_from_example_loads_in_harness_prompt -- --exact`

This keeps the shipped `configs/harness.example.jsonc` honest by preparing a restricted-tools live config from the legacy top-level `agent` shape, asserting the rendered config keeps only canonical `agents` / `ui.default_profile`, and running a real harness prompt against a local wiremock responses endpoint. If this fails, treat it as a prepared-config regression before widening into live-provider debugging.

### Live follow-up
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact`
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact`
- `HARNESS_LIVE_PROXY=1 HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc HARNESS_LIVE_PROXY_PROVIDER=default HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact`

Once the focused prepared-config regression is green, treat any remaining failures in the live follow-up as downstream lane-specific blockers instead of alias-shape setup failures.

### Workspace baseline
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

## Known remaining gaps

- This signoff does not cover future orchestration surfaces such as swarms, Ralph loops, `$` commands, or plugins.
- The Plan/Build proof is focused on the shipped contract, not every future UX polish item on the roadmap.
- PTY/live screenshot drift remains a known harness-testkit concern outside the narrow scope of issue #104; do not treat this issue as a blanket fix for that lane.
- Issue #105 keeps the live follow-up scoped to `live_proxy_preflight`, `live_proxy_prompt_chat_tool_flow`, and `live_proxy_e2e_tui_tool_flow`; broader Batch 1 wrapper coverage remains separate signoff work.
