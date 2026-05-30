# Testing and signoff map

`scripts/test-lanes.sh` is the canonical lane runner. Use the narrowest lane that proves a
change, keep the generated artifacts with the review evidence, and run broader lanes only when
the change touches the contracts they cover.

```bash
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh perf
scripts/test-lanes.sh coverage
scripts/test-lanes.sh simulation
scripts/test-lanes.sh signoff-binary
scripts/test-lanes.sh signoff-pty
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

## Nextest profiles and deterministic partitions

The deterministic suite is configured in `.config/nextest.toml`:

- `default`: parallel T1-T3 tests, `retries = 0`, `fail-fast = false`, `test-threads = "num-cpus"`, and `slow-timeout = { period = "2s", terminate-after = 10 }`.
- `ci`: inherits `default`, emits JUnit at `target/nextest/ci/junit.xml`, and is the CI/default deterministic runner.
- `perf`: T4 budget tests only (`test(/perf_/)`), with JUnit at `target/nextest/perf/junit.xml`.
- `process-global-state`: a documented serial group with zero current members; new tests must not be added to it to hide isolation bugs.

The `ci` profile excludes T5 PTY/live/native visual binaries and perf tests. Ignored live/native
signoff tests remain opt-in through explicit signoff lanes.

## Test-suite overhaul gates

`scripts/check-test-suite-gates.py` is the static gate runner for the test-suite overhaul tracked
by `docs/test-suite-prd.md` and this testing map:

```bash
python3 scripts/check-test-suite-gates.py
python3 scripts/check-test-suite-gates.py --report-only --json
python3 scripts/check-test-suite-gates.py --self-test
```

The gates cover deterministic-test sleeps, process-global env/cwd mutation, subprocess and
real-world dependency usage, widened test-file focus, T5 tree-total line budget, arrange/act/assert
conventions, cassette secret hygiene, committed snapshot orphans, and test taxonomy. Existing
arrange/act/assert debt is stored as SHA-256 keys in `docs/test-suite-conventions-baseline.json`;
the gate fails on new or stale debt without storing source-brand terms in docs. Committed `.snap`
files with `source:` metadata must point at an existing source file with an insta assertion, or be
referenced by snapshot name in crate Rust code. Acceptance requires the strict command without
`--report-only` to return zero violations.

## Fast default developer lane

Run this first for ordinary local changes:

```bash
scripts/test-lanes.sh fast
```

`fast` currently includes:

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo nextest run --profile ci --workspace --all-features`

`fast` explicitly excludes PTY signoff, live provider signoff, native visual signoff, stress lanes,
ignored tests, and real-network signoff commands. Use it for deterministic T1-T3 feedback across
all workspace crates.

## Integration CI partition lane

Run this to prove the deterministic profile can be partitioned without returning to dozens of
bespoke Cargo invocations:

```bash
scripts/test-lanes.sh integration
```

`integration` currently runs:

- `cargo nextest run --profile ci --workspace --all-features --partition hash:1/2`
- `cargo nextest run --profile ci --workspace --all-features --partition hash:2/2`

GitLab CI uses the unpartitioned `rust:test_nextest` job as the canonical JUnit-producing
deterministic job; this lane documents and validates the partition shape for larger runners.

## Quality gates lane

Run static gates before long jobs when changing tests, fixtures, cassettes, docs, or public output:

```bash
scripts/test-lanes.sh quality-gates
```

`quality-gates` runs:

- `python3 scripts/check-test-suite-gates.py`
- `python3 scripts/check-forbidden-branding.py`

The static suite fails deterministic tests that depend on live-provider environment such as
`HARNESS_LIVE_PROXY`; live provider coverage must stay in ignored/env-gated signoff lanes.

## Perf and coverage lanes

T4 performance budgets run through the perf nextest profile:

```bash
scripts/test-lanes.sh perf
cargo nextest run --profile perf --workspace --all-features
```

The current budget owners are `crates/harness-core/tests/perf_test.rs`, which asserts the resume-plan
projection stays under its measured wall-clock budget for a fixed large event log, and
`crates/harness/tests/perf_sessions_surface_test.rs`, which writes `large-session-surfaces.json`
under the perf stage artifact directory. The large-session artifact records corpus size,
`sessions list`, `sessions reopen --json`, and `session_search` timings plus provenance.
After nextest, the lane runs `scripts/check-perf-artifacts.py` in a `perf_artifact_freshness`
stage so missing, stale, or provenance-mismatched perf artifacts fail closed.

Coverage ratchet evidence is produced with:

```bash
scripts/test-lanes.sh coverage
```

`coverage` delegates to `scripts/coverage-ratchet.sh`, which requires `cargo-llvm-cov`, writes
`target/coverage/lcov.info` and `target/coverage/summary.txt`, and compares aggregate line coverage
at two-decimal precision against the source-controlled ratchet seed in
`docs/test-suite-coverage-baseline.txt` by default. Override `COVERAGE_BASELINE_PATH` only for local
experiments; a missing custom baseline records a new seed.

## Deterministic simulation lane

Run this lane when a change needs offline behavioral evidence that agents can diff and inspect:

```bash
scripts/test-lanes.sh simulation
```

`simulation` is offline-only. It uses the checked-in `docs/simulation-matrix.json`, runs the real
`harness run --scenario golden_path --deterministic` path twice with the built-in mock provider,
replays both runs with `harness replay --json`, then generates and validates a simulation evidence
bundle through `harness-testkit`.

Current stage commands:

- `cargo test -p harness-testkit --test simulation_validator_test`
- `cargo run -p harness -- --session-dir <artifact-root>/simulation/data/sessions-baseline run --scenario golden_path --deterministic --out <artifact-root>/simulation/data/baseline.events.jsonl --print-run-dir`
- `cargo run -p harness -- --session-dir <artifact-root>/simulation/data/sessions-repeat run --scenario golden_path --deterministic --out <artifact-root>/simulation/data/repeat.events.jsonl --print-run-dir`
- `cargo run -p harness -- replay --session <baseline-run-dir> --json`
- `cargo run -p harness -- replay --session <repeat-run-dir> --json`
- `cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root <artifact-root>/simulation/stages/simulation_evidence/artifacts --matrix docs/simulation-matrix.json --baseline-events <baseline.events.jsonl> --baseline-replay <baseline.replay.json> --repeat-events <repeat.events.jsonl> --repeat-replay <repeat.replay.json> --seed 0`
- `env HARNESS_SECRETS_SCAN_ARTIFACTS=1 HARNESS_SIMULATION_ARTIFACT_DIR=<simulation-artifacts> cargo test -p harness-testkit --test secretscan_test`

The `simulation_evidence` stage writes the standard lane files plus these simulation artifacts under
`<artifact-root>/simulation/stages/simulation_evidence/artifacts/`:

- `simulation-matrix.json`
- `simulation-events.jsonl` with `schema_version=simulation-event-v1`, monotonic `seq`, scenario,
  seed, actor/component identity, invariant IDs, redaction metadata, replay command fingerprint, and
  redacted predicate payloads.
- `simulation-report.json` with `schema_version=simulation-report-v1`, behavior deltas, invariant
  results, artifact index, replay commands, failure signals, redaction summary, volatile fields, and
  raw evidence paths.
- `artifact-index.jsonl` with `schema_version=artifact-index-v1`, relative artifact paths, clean
  redaction status, producers, and stable content fingerprints.
- `simulation-summary.txt`, `normalized-summary-baseline.json`, `normalized-summary-repeat.json`,
  and `same-seed-comparison.txt`.

Same-seed stability uses normalization profile `simulation-normalization-v1`; raw JSONL equality is
not required. The normalized summaries exclude raw session paths, workspace roots, resolved paths,
artifact roots, and lane timestamps. Provider cassette determinism is post-MVP for this lane because
the admitted scenario uses the mock provider, not recorded cassettes. PTY/live/native signoff lanes
remain provenance-only and must not own simulation behavioral invariants.

## Deterministic signoff PTY lane

Run the PTY lane when changing TUI rendering, transcript behavior, viewport-sensitive flows, or
anything that needs the deterministic headless UI oracle:

```bash
scripts/test-lanes.sh signoff-pty
```

This lane runs the PTY E2E tests single-threaded and writes manifest-backed visual evidence under
the configured artifact root. Legacy committed harness-testkit PTY snapshots were removed during
T5 slimming; current PTY evidence is generated under `target/pty-visual-artifacts/`, while retained
committed snapshots are owned by harness-tui deterministic snapshot tests. The harness-tui PTY test
target is fail-closed behind `HARNESS_TUI_PTY_SIGNOFF=1`, so ordinary
`cargo test -p harness-tui --test pty_e2e` remains a fast non-terminal helper check while the
signoff lane opts into the real PTY captures. Do not parallelize PTY signoff. For a combined
deterministic closeout, use:

- `env RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo test -p harness-tui --test pty_e2e`

The strict-V1 TUI signoff manifest is checked in at
[`docs/tui-signoff-manifest.v1.json`](tui-signoff-manifest.v1.json). Its schema version is
`harness-tui-signoff-manifest-v1`, and each flow row names:

- deterministic owner tests/snapshots,
- `signoff-pty` artifact stages,
- an explicit note that reference-image comparison is not required for this PRD, and
- the native-visual policy for env-gated local screenshots.

Required flow coverage is startup, command palette, session picker/resume, permission/question,
provider/tool failure, and diff review. `cargo test -p harness-tui --test deterministic_render_test`
guards the manifest shape, required flows, deterministic owner tests, and the no-reference-image-comparison policy. `env RUST_TEST_THREADS=1 cargo test
-p harness-testkit --test pty_e2e` copies the manifest and a summary into
`target/pty-visual-artifacts/` for lane provenance. Native visual remains a separate local
provenance class: when `HARNESS_NATIVE_VISUAL=1` and `DISPLAY=<display>` are missing, the manifest
records a documented gap rather than silently converting PTY evidence into native screenshot proof.

```bash
scripts/test-lanes.sh all-deterministic
```

`all-deterministic` runs `quality-gates`, then `simulation`, then `fast`, then `integration`, then `signoff-pty` only when PTY support checks
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
it runs `live_proxy_preflight_requires_live_env` first, then the prompt parity wrapper, then the TUI parity wrapper.
The underlying parity order is documented in
[`crates/harness-testkit/tests/README.live-proxy.md`](../crates/harness-testkit/tests/README.live-proxy.md):
CLI parity runs `live_proxy_preflight_requires_live_env` and `live_proxy_prompt_parity_signoff`;
TUI parity runs `live_proxy_preflight_requires_live_env` and
`live_proxy_e2e_tui_parity_signoff`.

Current stage commands:

- `cargo test -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact`
- `cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact`
- `cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact`

Use the live README for exact preflight details, optional live vars, artifacts, retention, and
agent iteration order instead of duplicating that contract here.

## Binary shim smoke

The single real-process CLI shim smoke is ignored by default and excluded from the deterministic
nextest profile. Run it only when validating the compiled `main.rs` wiring:

```bash
scripts/test-lanes.sh signoff-binary
```

`signoff-binary` sets `HARNESS_BINARY_SMOKE=1` plus `HARNESS_BINARY_SMOKE_ARTIFACT_DIR` and runs the ignored
`cargo test -p harness --test binary_smoke -- --ignored --exact` stage through the canonical
artifact-recording lane runner. The smoke runs `harness --help`, `harness --version`, outside-repository
`harness config validate`, text/JSON `harness doctor`, and a deterministic `harness prompt --mock`
first prompt against a copied canonical config through `CARGO_BIN_EXE_harness`. It also records a
PTY-backed `tui --mock --exit-on-finish` startup and a deterministic `run --scenario golden_path`
tool path with event artifacts under the smoke artifact directory; in-process CLI tests remain the
default proof for command behavior.

## Native visual lane

Native visual signoff is local, ignored by default, and env-gated:

```bash
HARNESS_NATIVE_VISUAL=1 \
DISPLAY=<display> \
scripts/test-lanes.sh signoff-native
```

This lane runs the native visual tests single-threaded. In the current slim T5 surface it fails
closed unless `HARNESS_NATIVE_VISUAL=1` and `DISPLAY=<display>` are present, and preserves the native
visual metadata/artifact-root contract for local signoff tooling. Treat native screenshots as local
visual evidence, not a portable hash oracle. If native prerequisites are unavailable, use `signoff-pty`
for deterministic UI signoff.

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

Current invariant owners:

| Protected invariant | Owning tests / lane |
|---|---|
| Coordinator scheduling, cancellation, failed-turn handling, compaction, and tool lifecycle | `cargo test -p harness-core --test coord_test`; focused chunks under `crates/harness-core/tests/coord/` |
| Replay purity and projection derivation from append-only events | `cargo test -p harness --test replay_sessions_cli_test`; `cargo test -p harness-core --test conversation_projection_test`; `cargo test -p harness-core --test transcript_projection_test`; `cargo test -p harness-core --test resume_plan_test`; `cargo test -p harness-core --test session_lineage_materialization_test` |
| Permission checks and redelegation guard | `cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test`; `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` |
| Native tool parity and stable public tool IDs | `cargo test -p harness-tools --test native_tool_parity_matrix_test` |
| V1 workspace-intelligence/native structural tools (`session_*`, `background_cancel`, `team_list`, `ast_grep_search`, `ast_grep_replace`) | `cargo test -p harness-tools --test native_control_plane_tools_test`; `cargo test -p harness-tools --test native_workspace_intelligence_tools_test`; `cargo test -p harness-tools --test native_ast_grep_replace_test`; `cargo test -p harness-tools --test team_test`; `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` |
| Doctor/support catalog metadata and redaction | `cargo test -p harness --test config_schema_cli_test doctor_cli`; `cargo test -p harness --test replay_sessions_cli_test sessions_export_cli_support_includes_readiness_and_config_summaries`; `cargo test -p harness --test replay_sessions_cli_test sessions_export_cli_redacts_support_bundle_secret_shapes` |
| Provider serialization, replay-only cassettes, redaction, and checkpoint accounting | `cargo test -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test`; `cargo test -p harness-providers --test recorded_test`; `cargo test -p harness-testkit --test secretscan_test` |
| Offline deterministic simulation matrix, semantic predicates, same-seed normalization, artifact index, and simulation redaction | `scripts/test-lanes.sh simulation`; `cargo test -p harness-testkit --test simulation_validator_test`; `cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root <dir> --matrix docs/simulation-matrix.json --baseline-events <events.jsonl> --baseline-replay <replay.json> --repeat-events <events.jsonl> --repeat-replay <replay.json> --seed 0` |
| Config/event docs drift and public schema generation | `cargo test -p harness --test config_docs_reference_test`; `cargo test -p harness --test event_docs_reference_test`; `cargo test -p harness --test config_schema_cli_test` |
| Deterministic UI content rendering, transcript layout, and navigation | `cargo test -p harness-tui --test deterministic_render_test`; `cargo test -p harness-tui --test lineage_view_model_test`; `cargo test -p harness-tui --test model_switcher_metadata_test`; `cargo test -p harness-tui --test session_navigation_keybindings_test`; `cargo test -p harness-tui --test pty_e2e` as the fail-closed helper lane |
| TUI signoff manifest and visual/provenance flow coverage | `cargo test -p harness-tui --test deterministic_render_test tui_signoff_manifest_covers_required_release_flows`; `env RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e pty_signoff_manifest_declares_required_flow_artifacts`; `scripts/test-lanes.sh signoff-pty` |
| Live, PTY, native visual provenance contracts | `scripts/test-lanes.sh signoff-pty`; `scripts/test-lanes.sh signoff-live`; `scripts/test-lanes.sh signoff-native` as opt-in T5 lanes only |

Retired harness-tui PTY helper scenario owners:

| Removed T5 helper scenario | Surviving deterministic owner |
|---|---|
| Startup shell / startup palette / startup session history | `cargo test -p harness-tui --test deterministic_render_test startup_shell_is_compose_first_without_pty command_palette_renders_without_pty startup_session_history_picker_renders_without_pty`; `cargo test -p harness-tui startup_slash_commands_execute_without_menu command_palette_renders_and_filters` |
| Streamed response and completed live shell | `cargo test -p harness-tui live_shell_enter_submits_and_echoes_prompt_snapshot live_shell_type_first_input_snapshot`; `cargo test -p harness-tui --test deterministic_render_test live_transcript_and_operator_sidebar_render_without_pty` |
| Tool lifecycle and inline diff parity | `cargo test -p harness-tui --test deterministic_render_test tool_lifecycle_rows_stay_ordered_without_pty`; `cargo test -p harness-tui transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs transcript_inline_diff_stays_compact_between_tool_rows transcript_native_edit_renders_inline_diff_from_artifact` |
| Permission and question overlays | `cargo test -p harness-tui permission_modal_preempts_palette_and_slash permission_overlay_preserves_draft_and_transcript_context permission_overlay_ignores_plain_draft_input_once_prompt_is_active`; `cargo test -p harness-tui --test deterministic_render_test permission_modal_preserves_draft_without_pty question_permission_prompt_renders_without_pty` |
| Operator sidebar, details drawer, and orchestration states | `cargo test -p harness-tui --test deterministic_render_test live_transcript_and_operator_sidebar_render_without_pty`; `cargo test -p harness-tui operator_sidebar_preserves_section_order_and_copy live_shell_details_drawer_orchestration_snapshot orchestration_projection_tracks_queued_started_completed_counts orchestration_projection_tracks_stale_then_late_result` |
| Degraded/disconnected/replay shells | `cargo test -p harness-tui live_shell_degraded_bootstrap_snapshot live_shell_disconnected_stream_snapshot`; `cargo test -p harness-tui --test deterministic_render_test replay_shell_is_read_only_without_pty` |

Narrowed harness-testkit PTY assertion owners:

| Removed or narrowed T5 assertion | Surviving owner |
|---|---|
| Duplicate operator-sidebar screen-string assertions in `pty_e2e_sidebar_session_parity` and `pty_helper_operator_sidebar_session_contract` | `cargo test -p harness-tui --test deterministic_render_test live_transcript_and_operator_sidebar_render_without_pty`; `cargo test -p harness-tui operator_sidebar_preserves_section_order_and_copy`; remaining T5 manifest-backed screenshots assert only smoke/provenance markers. |
| Duplicate permission-overlay screen-string assertions in `pty_e2e_permission_dock_parity` and `pty_helper_permission_with_draft` | `cargo test -p harness-tui permission_modal_preempts_palette_and_slash permission_overlay_preserves_draft_and_transcript_context permission_overlay_ignores_plain_draft_input_once_prompt_is_active`; `cargo test -p harness-tui --test deterministic_render_test permission_modal_preserves_draft_without_pty`; remaining T5 captures keep permission smoke/provenance markers. |

Retired harness-testkit T5 scenario owners:

| Removed T5 scenario group | Surviving owner |
|---|---|
| PTY startup, command palette, replay/continue history, and type-first shell content checks | `cargo test -p harness-tui --test deterministic_render_test startup_shell_is_compose_first_without_pty startup_session_history_picker_renders_without_pty replay_shell_is_read_only_without_pty`; `cargo test -p harness-tui startup_slash_commands_execute_without_menu command_palette_renders_and_filters`; slim `cargo test -p harness-testkit --test pty_e2e` keeps only single-thread/env/artifact-path smoke. |
| PTY transcript, native-tool row, inline diff, MCP/background, and dense-log screen checks | `cargo test -p harness-tui --test deterministic_render_test tool_lifecycle_rows_stay_ordered_without_pty live_transcript_and_operator_sidebar_render_without_pty`; `cargo test -p harness-tui transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs transcript_inline_diff_stays_compact_between_tool_rows transcript_native_edit_renders_inline_diff_from_artifact`; `cargo test -p harness-tools --test native_tool_parity_matrix_test`. |
| PTY child-session, lineage, replay-read-only, active/unrestorable continue rejection checks | `cargo test -p harness --test replay_sessions_cli_test`; `cargo test -p harness-tui --test lineage_view_model_test`; `cargo test -p harness-tui --test session_navigation_keybindings_test`; `cargo test -p harness-core --test session_lineage_materialization_test`. |
| Live proxy prompt/TUI/native tool-flow, provider parity, config mutation, request/evidence, and wiremock checks | `cargo test -p harness-providers --test recorded_test`; `cargo test -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test`; `cargo test -p harness-testkit --test live_proxy_e2e` for env-gated signoff names and fail-closed config preflight. |
| Live visual manifest, vision, screenshot evidence, and artifact-retention checks | `cargo test -p harness-tui --test deterministic_render_test`; `cargo test -p harness-testkit --test native_visual_e2e`; opt-in `scripts/test-lanes.sh signoff-native` for local screenshot provenance only. |
| Native visual startup geometry, navigation, permission, transcript, operator-sidebar, Ghostty, capture-helper, and managed-session checks | `cargo test -p harness-tui --test deterministic_render_test`; `cargo test -p harness-tui permission_modal_preempts_palette_and_slash operator_sidebar_preserves_section_order_and_copy`; `cargo test -p harness-testkit --test native_visual_e2e`; `cargo test -p harness-testkit --bin native_visual_helper -- --help` when validating the helper CLI. |
| Shared T5 fixture/rendering helpers (`harness_bin`, session fixtures, temp paths, visual renderer, manifest writers, markers) | `cargo test -p harness-testkit --lib`; `cargo test -p harness-testkit --test secretscan_test`; deterministic fixture ownership moved to crate-local test helpers and harness-tui render fixtures rather than uncompiled T5 support. |

The acceptance dossier for the test-suite overhaul is recorded in
`docs/test-suite-prd.md` and this owner map. It maps the Section 12 A1–A15 gates to
the concrete artifacts under `target/test-suite-overhaul/`.
