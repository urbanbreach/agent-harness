# Testing and signoff map

`scripts/test-lanes.sh` is the canonical lane runner. Use the narrowest lane that proves a
change, keep the generated artifacts with the review evidence, and run broader lanes only when
the change touches the contracts they cover.

```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh signoff-pty
scripts/test-lanes.sh signoff-browser
scripts/test-lanes.sh signoff-live
scripts/test-lanes.sh signoff-native
scripts/test-lanes.sh stress-offline
scripts/test-lanes.sh stress-live
scripts/test-lanes.sh all-deterministic
```

Use `--dry-run` to write the same command, status, stdout, stderr, and verification artifact
shape without running the underlying commands:

```bash
scripts/test-lanes.sh fast --dry-run
```

Each run writes `<artifact-root>/summary.txt`, `<artifact-root>/env.txt`, and per-stage evidence
under `<artifact-root>/<mode>/stages/<stage>/`. Keep those files with closeout notes when a lane
is used as signoff evidence.

## Fast default developer lane

Run this first for ordinary local changes:

```bash
scripts/test-lanes.sh fast
```

`fast` currently includes:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test -p harness-tui --lib`
- `cargo test -p harness-tui --test model_switcher_metadata`
- `cargo test -p harness-tui --test session_navigation_keybindings`

`fast` explicitly excludes PTY signoff, live provider signoff, native visual signoff, stress
lanes, ignored tests, and real-network signoff commands. Use it for quick deterministic feedback,
not release signoff for UI rendering, provider behavior, native screenshots, or stress coverage.

## Integration CI lane

Run this after `fast` when a change touches deterministic runtime, config, replay, permission,
compaction, or tool-surface contracts:

```bash
scripts/test-lanes.sh integration
```

`integration` carries the protected deterministic checks that are too focused or too slow for
`fast`: public config drift, event docs drift, coordinator scheduling and replay contracts,
permission and redelegation guards, native tool parity, and provider-context compaction
regressions. Run the forbidden-branding scan with integration-focused changes that add public
docs, help text, snapshots, or generated artifacts. It remains non-live, non-native-visual, and
non-PTY-signoff.

Current stage commands:

- `python3 scripts/check-forbidden-branding.py`
- `cargo test -p harness --test bootstrap_profiles`
- `cargo test -p harness --test config_docs_reference`
- `cargo test -p harness --test determinism_multi_turn_tools`
- `cargo test -p harness --test event_docs_reference`
- `cargo run -p harness -- --config configs/harness.example.jsonc config validate`
- `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json`
- `cargo test -p harness --test workflow_cli`
- `cargo test -p harness-core workflow`
- `cargo test -p harness-testkit workflow_simulator`
- `cargo test -p harness --test prompt_cli`
- `cargo test -p harness --test replay_sessions_cli`
- `cargo test -p harness --test run_cli`
- `cargo test -p harness --test stress_harness_script`
- `cargo test -p harness --test tui_cli replay_flag_bypasses_launcher_shell`
- `cargo test -p harness-providers --lib`
- `cargo test -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes`
- `cargo test -p harness-tools --lib`
- `cargo test -p harness-tools --test native_tool_parity_matrix`
- `cargo test -p harness-tools --test hashline_apply`
- `cargo test -p harness-tools --test mcp_generic`
- `cargo test -p harness-tools --test native_agent_spawn_child_session_observability`
- `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order`
- `cargo test -p harness-tools --test native_code_lsp`
- `cargo test -p harness-tools --test native_code_search`
- `cargo test -p harness-tools --test native_github`
- `cargo test -p harness-tools --test native_question_tool`
- `cargo test -p harness-tools --test native_web_fetch`
- `cargo test -p harness-tools --test native_web_search`
- `cargo test -p harness-tools --test native_workspace_edit_routing`
- `cargo test -p harness-tools --test single_surface_live`
- `cargo test -p harness-tools --test skill_load_discovery`
- `cargo test -p harness-testkit --lib`
- `cargo test -p harness-testkit --test live_proxy_e2e`
- `cargo test -p harness-testkit --test secretscan`
- `cargo test -p harness-tools --test native_execution_surface`
- `cargo test -p harness-tools --test native_control_plane_tools`
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
- `cargo test -p harness-core conversation_projection_failed_checkpoint_turn_status`
- `cargo test -p harness-core --test resume_plan session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts`

## Workflow replay and dossier closeout evidence

When a change touches workflow lifecycle, evidence, signoff, replay/restart, or dossier surfaces,
include replay-derived closeout evidence in addition to the narrow unit tests. The usual focused
commands are:

- `cargo test -p harness-testkit workflow_simulator`
- `cargo test -p harness --test workflow_cli`
- `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json`

`harness workflow dossier export --json --run-dir <run>` regenerates the Run Dossier from
`events.jsonl`; do not edit an exported dossier as the workflow authority. Status, dossier,
snapshot, goal, and mission reads must stay projection-only and must not append events. The
deterministic simulator and workflow CLI tests cover intake restart/replay, missing-evidence
denials, mapped evidence, active continuations, workflow-owned tasks, question blockers, required
dossier-export evidence, operator waiver/signoff, audit-only non-mutation, closeout
legal-next-actions/readiness JSON, replay read-only equivalence, stale dossier export semantics, and
dossier export without live providers.

For terminal workflow closeout, keep the acceptance dossier in
`docs/harness-omx-next-completion-dossier.md` aligned with the latest
replay/ledger evidence. The dossier is a human-readable projection of
`.omx/ultragoal/ledger.jsonl`, the workflow inventory fixture, docs/schema drift
tests, and verification artifacts. During staged work it must name pending gates
instead of implying false completion.

Focused commands for that dossier are:

```bash
cargo test -p harness --test workflow_inventory
cargo test -p harness --test config_docs_reference
cargo test -p harness --test config_schema_cli
cargo test -p harness --test event_docs_reference
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/test-lanes.sh fast
scripts/test-lanes.sh all-deterministic
```

## Deterministic signoff PTY lane

Run the PTY lane when changing TUI rendering, transcript behavior, viewport-sensitive flows, or
anything that needs the deterministic headless UI oracle:

```bash
scripts/test-lanes.sh signoff-pty
```

This lane runs the PTY E2E tests single-threaded and writes manifest-backed visual evidence under
the configured artifact root. Do not parallelize it. For a combined deterministic closeout, use:

- `env RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e`
- `env RUST_TEST_THREADS=1 cargo test -p harness-tui pty_e2e`

```bash
scripts/test-lanes.sh all-deterministic
```

`all-deterministic` runs `fast`, then `integration`, then `signoff-pty` only when PTY support checks
pass. Its PTY gate requires `cargo` on `PATH`, both PTY test files to exist, and
`HARNESS_TEST_LANES_SKIP_PTY` not set to `1`.

## Live provider opt-in lane

Live signoff is opt-in and env-gated:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
scripts/test-lanes.sh signoff-live
```

Required live environment:

- `HARNESS_LIVE_PROXY=1`
- `HARNESS_LIVE_PROXY_CONFIG=<path>`
- `HARNESS_LIVE_PROXY_PROVIDER=<provider>`
- `HARNESS_LIVE_PROXY_MODEL=<model>`

`signoff-live` fails closed when the live environment is missing. When the environment is present,
it runs `live_proxy_preflight` first, then the prompt parity wrapper, then the TUI parity wrapper.
The underlying parity order is documented in
[`crates/harness-testkit/tests/README.live-proxy.md`](../crates/harness-testkit/tests/README.live-proxy.md):
CLI parity runs `live_proxy_prompt_responses_smoke`, `live_proxy_prompt_chat_tool_flow`, and
`live_proxy_prompt_native_tool_flow`; TUI parity runs `live_proxy_preflight`,
`live_proxy_e2e_tui_prompt_responses_smoke`, and `live_proxy_e2e_tui_tool_flow`.

Current stage commands:

- `cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact`
- `cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact`
- `cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact`

Use the live README for exact preflight details, optional live vars, artifacts, retention, and
agent iteration order instead of duplicating that contract here.

## Browser/media signoff lane

Browser/media signoff is opt-in and env-gated so browser dependencies are never
downloaded or launched accidentally:

```bash
HARNESS_BROWSER_SIGNOFF=1 scripts/test-lanes.sh signoff-browser
```

Required browser/media environment:

- `HARNESS_BROWSER_SIGNOFF=1`
- `npx` on `PATH` for Playwright-backed skill diagnostics

The lane records doctor browser/terminal diagnostics plus deterministic coverage
for `look_at` media routing and terminal dependency gating. It does not perform
live browser network calls by itself; load `playwright`, `agent-browser`, or
`dev-browser` skills explicitly for task-specific browser work.

Current stage commands:

- `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json`
- `cargo test -p harness-tools --test native_execution_surface native_look_at_extracts_text_and_routes_media`
- `cargo test -p harness-tools --test native_execution_surface native_terminal_tools_are_registered_and_dependency_gated`

## Native visual lane

Native visual signoff is local, ignored by default, and env-gated:

```bash
HARNESS_NATIVE_VISUAL=1 \
DISPLAY=<display> \
scripts/test-lanes.sh signoff-native
```

This lane runs the native visual tests single-threaded. It requires `HARNESS_NATIVE_VISUAL=1` and
`DISPLAY=<display>`, then records native screenshot provenance under the artifact root. Treat this
as local visual evidence, not a portable hash oracle. If native prerequisites are unavailable, use
`signoff-pty` for deterministic UI signoff.

Current stage command:

- `cargo test -p harness-testkit --test native_visual_e2e -- --ignored --test-threads=1`

## Stress lanes

Stress lanes delegate to `scripts/stress-harness.sh` and reuse a built harness binary when
`--harness-bin <path>` is supplied or `target/debug/harness` already exists.

Deterministic offline stress:

```bash
scripts/test-lanes.sh stress-offline
```

Live stress:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
scripts/test-lanes.sh stress-live
```

`stress-offline` delegates to `scripts/stress-harness.sh --mode offline`. `stress-live` uses the
same live env guard as `signoff-live` and delegates to `scripts/stress-harness.sh --mode live` with
`--config` set from `HARNESS_LIVE_PROXY_CONFIG`. Both stress lanes add `--artifact-dir`, and both
add `--harness-bin` when a binary was supplied to `scripts/test-lanes.sh` or an existing
`target/debug/harness` can be reused.

## Deletion policy and invariant map

Before deleting or narrowing tests, update the test-suite overhaul evidence rather than relying on
memory. Every deletion needs a preserved invariant owner in the current map, or replacement coverage
that proves the same behavior before the old test is removed.

Expect the invariant map to keep these owners visible:

- Drift checks for public config docs and event docs.
- Coordinator scheduling, replay-derived background output, cancellation, permission, and redelegation contracts.
- Native tool parity and stable public tool IDs.
- Provider-context compaction regressions and checkpoint artifact accounting.
- Deterministic PTY rendering evidence for UI behavior that cannot be proven with unit tests.
- Live and native visual lanes as opt-in signoff only, with artifacts and provenance included in
  the closeout evidence.
