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
by this testing map:

```bash
python3 scripts/check-test-suite-gates.py
python3 scripts/check-test-suite-gates.py --report-only --json
python3 scripts/check-test-suite-gates.py --self-test
```

The gates cover deterministic-test sleeps, process-global env/cwd mutation, subprocess and
real-world dependency usage, widened test-file focus, T5 signoff-file line budget, arrange/act/assert
conventions, cassette secret hygiene, committed snapshot orphans, and test taxonomy. Any residual
arrange/act/assert debt is stored as SHA-256 keys in `docs/testing/test-suite-conventions-baseline.json`;
the gate fails on new or stale debt without storing source-brand terms in docs. The baseline is
currently empty; re-adding entries requires explicit approval and a
documented removal path, because the goal is to keep this file at zero debt. Committed `.snap`
files with `source:` metadata must point at an existing source file with an insta assertion, or be
referenced by snapshot name in crate Rust code. Acceptance requires the strict command without
`--report-only` to return zero violations.

Real-world dependencies are permitted only in repository Rust test files whose basename ends in
`_recorded.rs`. This is explicit non-default real-world/signoff evidence and remains opt-in;
ordinary `_test.rs` and unsuffixed test files remain subject to the no-real-world-deps gate.

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
at two-decimal precision against `docs/testing/test-suite-coverage-baseline.txt` by default. When the
baseline is absent, the lane records the current value as a new seed. Override
`COVERAGE_BASELINE_PATH` only for local experiments.

### Engine metrics baseline

The simplification work also has a source-and-runtime inventory command:

```bash
bash scripts/engine-metrics.sh --output artifacts/qa-evidence/20260823-engine-simplification-baseline/engine-metrics.json --baseline 060ee1fd
```

The versioned `engine-metrics-v1` JSON is written atomically after the supplied baseline commit
resolves. It excludes target, session/artifact directories, reference caches, Rust tests, and
`cfg(test)` code from production LOC. A missing baseline fails before an output is created. It
does not relabel the one-session golden run as corpus or long-session evidence: those timing
fields are explicitly `unavailable` until the perf fixture produces a successful artifact.

## G004 typed session owner checks

The canonical typed session reducer and read-only V1 compatibility boundary are owned by these
literal filters:

```bash
cargo nextest run -p harness-core --test conversation_projection_test --test resume_plan_test --test session_lineage_materialization_test -E 'test(/canonical_session_|canonical_active_path_|canonical_tool_pairing_|canonical_root_child_isolation/)'
cargo nextest run -p harness-core --test foreign_session_test --test session_lineage_materialization_test --test resume_plan_test -E 'test(/legacy_adapter_|canonical_foreign_identity_|canonical_branch_selection/)'
```

The second filter covers real tool-call-id correlation, provider lifecycle ordering, semantic
payload and warning inventory, restart fidelity, deterministic collision-resistant identities, and
zero-write source preservation. Product-surface QA runs:

```bash
bash scripts/harness-qa-dogfood.sh --slug m04-session
```

The resulting deterministic `scenario_fixture` can be inspected and reopened directly. Because
`sessions list` intentionally hides scenario fixtures, list/inspect/reopen coverage additionally
creates a successful isolated `harness prompt --mock` run in the same evidence session directory
and targets that operator-mode run.

## G005 semantic history owner checks

The focused core owners cover self-contained assistant commits, chunk-boundary independence,
non-durable runtime fragments, interrupted requests, old delta-only logs, deterministic restart,
and semantic conversation/transcript projection:

```bash
cargo nextest run -p harness-core --test coord_test --test conversation_projection_test --test transcript_projection_test --test resume_plan_test -E 'test(/semantic_history_|semantic_conversation_|semantic_transcript_|semantic_restart_|provider_chunk_boundaries_|lost_live_deltas_|interrupted_fragments_|runtime_subscription_delivers_live_deltas_|legacy_conversation_|legacy_interrupted_history_/)'
```

The product owner checks that committed assistant content replaces a conflicting legacy fragment
and that an `interactive_mock` session reopens with an offline continuation hint, accepts
`--mock --resume`, and appends the next semantic commit without durable deltas. The docs owner
checks the public event inventory and completion fields:

```bash
cargo nextest run -p harness --test replay_sessions_cli_test -E 'test(/export_uses_committed_assistant_content|interactive_mock_reopen_hint_preserves_offline_resume_mode|prompt_cli_accepts_mock_resume_for_offline_continuation|interactive_mock_session_continues_offline_from_semantic_commit/)'
cargo nextest run -p harness --test event_docs_reference_test
```

These deterministic owners don't assert PTY, live-provider, native visual, or dogfood evidence.

## G006 Compaction V2 owner checks

Compaction V2 has one active coordinator pipeline for manual, pre-prompt, and overflow triggers.
The exact twenty scenario owners are distributed across the coordinator, conversation-projection,
and memory-queue targets:

```text
compaction_v2_long_session_preempts_overflow
compaction_v2_unexpected_overflow_retries_once
compaction_v2_second_overflow_terminates
compaction_v2_failed_or_cancelled_generation_preserves_boundary
compaction_v2_repeated_runs_keep_latest_rolling_summary
compaction_v2_previous_summary_counted_once
compaction_v2_old_branch_summary_not_reintroduced
compaction_v2_huge_turn_splits_utf8_safe_prefix
compaction_v2_tool_pair_stays_atomic
compaction_v2_orphan_tool_result_excluded
compaction_v2_large_tool_result_preserves_protocol
compaction_v2_unicode_attachment_payload_is_safe
compaction_v2_attachments_charge_budget_once
compaction_v2_aborted_usage_not_anchor
compaction_v2_model_downshift_regenerates_summary
compaction_v2_root_child_histories_isolated
compaction_v2_restart_context_equals_live_context
compaction_v2_current_intent_survives_summary
compaction_v2_file_state_survives_summary
compaction_v2_manual_auto_share_event_shape
```

The unchanged literal owner commands are:

```bash
cargo nextest run -p harness-core --test coord_test --test conversation_projection_test --test memory_queue_compaction_test
cargo nextest run -p harness-core --test coord_test --test conversation_projection_test
cargo nextest run -p harness --test event_docs_reference_test
cargo fmt --all -- --check
cargo check -p harness-core
cargo clippy -p harness-core --all-targets -- -D warnings
git diff --check
bash scripts/harness-qa-dogfood.sh --slug m06-compaction-v2
```

The first two commands are deterministic owner suites; they must report zero skipped tests. The
dogfood command is an offline mock product check, not live-provider, PTY, or visual evidence. Task
receipts and the scenario matrix are retained under
`artifacts/qa-evidence/20260823-engine-simplification-ulw/m06-compaction-v2/` and the active
attempt evidence directory. The event-doc owner additionally verifies that the documented
`SessionCompaction` field list exactly matches `SessionCompactionEvent`, including serde-defaulted
optional fields.

### Static simplification receipts

`engine-metrics-v1` is the reproducible before/after inventory for the verified G005 commit
`56edaeaa6090fbe33c198013822c66b5497151a3` and the current tree:

```bash
bash scripts/engine-metrics.sh \
  --output artifacts/qa-evidence/20260823-engine-simplification-ulw/m06-compaction-v2/engine-metrics-final.json \
  --baseline 56edaeaa6090fbe33c198013822c66b5497151a3
```

The receipt records production LOC, frozen-overlap LOC, module/file inventory, compaction and
event-variant counts, durable reducer/projection count, representative event-log bytes, and
`SIZE_OK` inventory. The current Task14 tree is expected to show a temporary positive compaction
bucket delta because V2 introduces typed preparation, generation, validation, commit, and
read-only-adapter boundaries. This is a measured G006 transition, not a claim that the overall
G003-G012 overlap is net positive: later approved milestones own projection consolidation,
bounded indexing, compatibility deletion, and core-boundary cleanup. Do not attribute those later
deletions to G006.

The static audit must show exactly one active V2 `SessionCompaction` success writer/constructor,
zero active checkpoint writers, the current `EventV1` variant count, the durable projection/reducer
count, and the `SIZE_OK` status. Deprecated lifecycle constructors and checkpoint readers may appear
only in the read-only `session::legacy` adapter or compatibility fixtures until G010; their presence
does not make them active writers.

## Deterministic simulation lane

Run this lane when a change needs offline behavioral evidence that agents can diff and inspect:

```bash
scripts/test-lanes.sh simulation
```

`simulation` is offline-only. It uses the checked-in `docs/testing/simulation-matrix.json`, runs the real
`harness run --scenario golden_path --deterministic` path twice with the built-in mock provider,
derives read-only replay summaries for both runs, then generates and validates a simulation evidence
bundle through `harness-testkit`.

Current stage commands:

- `cargo nextest run -p harness-testkit --test simulation_validator_test`
- `cargo run -p harness -- --session-dir <artifact-root>/simulation/data/sessions-baseline run --scenario golden_path --deterministic --out <artifact-root>/simulation/data/baseline.events.jsonl --print-run-dir`
- `cargo run -p harness -- --session-dir <artifact-root>/simulation/data/sessions-repeat run --scenario golden_path --deterministic --out <artifact-root>/simulation/data/repeat.events.jsonl --print-run-dir`
- read-only replay summary generation for `<baseline-run-dir>`
- read-only replay summary generation for `<repeat-run-dir>`
- `cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root <artifact-root>/simulation/stages/simulation_evidence/artifacts --matrix docs/testing/simulation-matrix.json --baseline-events <baseline.events.jsonl> --baseline-replay <baseline.replay.json> --repeat-events <repeat.events.jsonl> --repeat-replay <repeat.replay.json> --seed 0`
- `env HARNESS_SECRETS_SCAN_ARTIFACTS=1 HARNESS_SIMULATION_ARTIFACT_DIR=<simulation-artifacts> cargo nextest run -p harness-testkit --test secretscan_test`

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

The simulation matrix currently admits **`golden_path` only** as
`offline-deterministic` (INV-001…004). Additional offline themes are owned by
focused nextest (see agent dogfood / theme table below), not by expanding the
simulation lane multi-scenario runner in this PRD V1.

## Offline agent dogfood channel

Product-touching runtime, CLI, tool, scenario, or session-path changes should
leave offline mock dogfood evidence in addition to owner nextest:

```bash
bash scripts/harness-qa-dogfood.sh --self-test
# or: bash scripts/harness-qa-dogfood.sh --slug <short-slug>
```

- Runtime skill: `.agent-harness/skills/harness-qa/` (`skill:project:harness-qa`).
- Evidence root (gitignored): `artifacts/qa-evidence/<YYYYMMDD>-<slug>/` with
  `README.md`, `commands.log`, `isolation-receipt.txt`, `events-excerpt.jsonl`,
  and `lane-or-run-summary.txt`.
- Isolation: session roots under the evidence directory or `/tmp`; do not pollute
  developer global harness config/home.
- Non-claims: not live provider proof; not PTY/native visual; not simulation
  matrix ownership; not a substitute for owner nextest.

Owner tests: `cargo nextest run -p harness-tools --test skill_load_discovery_test`
(includes harness-qa quality contract) and the script `--self-test` itself.

### Offline theme owners (WS-P1 disposition)

| Theme | Owner surface |
|-------|----------------|
| T-permissions | `interactive_golden_path_deny_emits_edit_rejected_without_applying_file` (`harness` run unit tests) |
| T-multi-tool | `determinism_multi_turn_tools_test` |
| T-compaction | harness-core coord compaction tests (manual/overflow/checkpoint) |
| T-task-lineage | `session_lineage_materialization_test`; transcript projection task-lineage tests |
| T-provider-error | `prompt_cli_exits_nonzero_on_provider_error_finish`; categorized provider error prompt CLI tests |
| T-session-inspect | `session_inspect_side_effect_free_test` (`sessions list`/`inspect` leave `events.jsonl` unchanged) |

## Deterministic signoff PTY lane

Run the PTY lane when changing TUI rendering, transcript behavior, viewport-sensitive flows, or
anything that needs the deterministic headless UI oracle:

```bash
scripts/test-lanes.sh signoff-pty
```

`signoff-pty` is a **strict fail-closed** lane (no soft `|| true` stages). Missing owners,
missing `cargo`, stage failures, or dual-binary journey failures fail the run and write
`pty-lane-verdict.txt`. Silent skip is forbidden.

This lane runs the PTY E2E tests single-threaded and writes manifest-backed visual evidence under
the configured artifact root. Legacy committed harness-testkit PTY snapshots were removed during
T5 slimming; current PTY evidence is generated under `target/pty-visual-artifacts/`, while retained
committed snapshots are owned by harness-tui deterministic snapshot tests. The harness-tui PTY test
target is fail-closed behind `HARNESS_TUI_PTY_SIGNOFF=1`, so ordinary
`cargo nextest run -p harness-tui --test pty_e2e` remains a fast non-terminal helper check while the
signoff lane opts into the real PTY captures. Do not parallelize PTY signoff.

Fail-closed stages (no `|| true`):

| Stage | What it proves |
|-------|----------------|
| `pty_prerequisites` | owner files exist; `cargo` on `PATH` (missing owner = FAIL) |
| `harness_testkit_pty_e2e` | testkit PTY E2E + visual artifact provenance |
| `harness_tui_pty_e2e` | harness-tui PTY E2E under `HARNESS_TUI_PTY_SIGNOFF=1`, including reply-capable emulation and canonical P0-06 artifacts |
| `harness_tui_p0_03_pty_recorded` | P0-03 boxed markdown, OSC-8, and event-driven streaming-fence PTY regression |
| `harness_tui_p0_04_pty_recorded` | P0-04 persistent multiline, queued send, interject, and cancel-and-replace PTY regression |
| `harness_tui_p1_02_pty_recorded` | P1-02 reply-capable native PTY journey for Commands -> Settings chrome, tabs, restoration, stale pointer input, six-cell close target, and 80x24/120x40/160x50 alignment |
| `harness_tui_happy_path_pty` | compiled `harness` CLI mock happy path (`pty_happy_path_recorded`) |
| `p0_06_xterm_tests` | xterm.js structured collector, canonical viewport, runtime-branding, and evidence contract tests in real Chromium |
| `p1_02_xterm_tests` | dedicated JS owner for the shipped-binary P1-02 scenario, canonical close coordinates, and interaction contract |
| `xterm_harness_binary` | validates the shipped Harness build before browser capture; each scenario rebuilds from the recorded clean tree, copies, pre-hashes, executes, and post-hashes an isolated tested binary |
| `p0_06_xterm_80x24`, `p0_06_xterm_120x40`, `p0_06_xterm_160x50` | the compiled Harness mock TUI driven through a native PTY into xterm.js at each canonical size |
| `p1_02_xterm_80x24`, `p1_02_xterm_120x40`, `p1_02_xterm_160x50` | `target/debug/harness` driven through util-linux `script` into Chromium+xterm.js; captures modal/tab/breadcrumb/footer states and keyboard/mouse restoration history at each canonical size |

The dedicated P1-02 owners can also be run directly:

```bash
env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 \
  cargo nextest run -p harness-tui --test p1_02_pty_recorded --test-threads 1 --ignore-default-filter
node --test scripts/qa/p1-02-modal-chrome.test.mjs
cargo build -p harness
node scripts/qa/web-terminal-visual-qa.mjs \
  --scenario p1-02-modal-chrome \
  --evidence-dir target/artifacts/p1-02-modal-chrome-120x40 \
  --cols 120 --rows 40
```

Repeat the browser command with `80 24` and `160 50` for the other canonical geometries. Prerequisites are Linux PTY support, `cargo`, Node/npm, util-linux `script`, and executable `/usr/bin/chromium`; `npm ci --prefix scripts/qa` installs the pinned local xterm.js/Playwright packages. The scenario opens Settings only through Commands, verifies Harness-owned frame/title, Runtime/TUI tabs, breadcrumb, shortcut footer, Tab/Shift+Tab, Escape restoration, stale outside input, and mouse-close restoration. Automated assertions establish interaction and evidence contracts; visual parity still requires reviewing the emitted screenshots.

Each P1-02 xterm stage writes into the lane's ignored `target/test-lanes/.../signoff-pty/stages/<stage>/artifacts/` tree. Evidence includes indexed and final PNGs, `terminal.ansi`, `terminal-ansi.txt`, `terminal.txt`, `buffer.json`, `interactions.json`, `metadata.json`, `harness-binary-provenance.txt`, `artifact-manifest.json`, `PASS.json`, and `cleanup.json`. The binary receipt ties a just-completed `cargo build -p harness` on the recorded clean HEAD/tree to matching source-binary and isolated tested-copy hashes before and after execution; the receipt itself is manifest-hashed. PASS is refused unless that chain remains unchanged, PTY/browser/profile/temp-root cleanup receipts are complete, visible runtime evidence contains Harness, and collected runtime text contains neither Grok nor xAI branding. The lane atomically allocates owner-only security-allowlisted `/tmp/harness-xterm-p1-02-*` roots and trap-cleans them after evidence is copied without replacing the owned root inode during preparation.

The P0-06 owner feeds native PTY output into a reply-capable vt100 emulator and forwards generated cursor-position reports to the child. It asserts cells, cursor position/visibility, alternate-screen transitions, input modes, wrapping, and emulator scrollback. Under the lane-provided absolute `HARNESS_P0_06_ARTIFACT_DIR`, it records before/after ANSI, text, and structured-screen captures at 80x24, 120x40, and 160x50 plus a hash-verified manifest and aggregate cleanup receipt. Native captures truthfully identify the hashed `harness-tui` integration-test executable and its direct production entrypoint, `harness_tui::run_tui_with_options`; they do not relabel that owner binary as the shipped `harness` CLI. The xterm stages separately drive the compiled `target/debug/harness` binary in Chromium at every canonical size, persist screenshot/ANSI/text/structured evidence plus a hashed shipped-binary provenance sidecar in their lane stage artifacts, and fail when collected runtime state lacks Harness branding or contains Grok/xAI marks.

For a combined deterministic closeout, use:

- `env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p0_03_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p0_04_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p1_02_pty_recorded --test-threads 1 --ignore-default-filter`
- `node --test scripts/qa/p1-02-modal-chrome.test.mjs`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=<dir> cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored --exact scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit`


Snapshot reconciliation note: the `command_palette_renders_without_pty` and
`tool_lifecycle_rows_stay_ordered_without_pty` snapshots were reconciled to
match current render behavior (live composer placeholder line and ordered tool
lifecycle rows). The verdict was fixture drift — the committed snapshots
predated the current render output; no behavior change was introduced.

```bash
scripts/test-lanes.sh all-deterministic
```

`all-deterministic` runs `quality-gates`, then `simulation`, then `fast`, then `integration`, then `signoff-pty` only when PTY support checks
pass. Its PTY gate requires `cargo` on `PATH`, both PTY test files to exist, and
`HARNESS_TEST_LANES_SKIP_PTY` not set to `1`.

## Live provider opt-in lane

Live signoff is opt-in and env-gated. After T5 slimming, `signoff-live` remains a **preflight +
signoff** lane: env/config/provider-model tuple checks and the retained prompt/TUI signoff
wrappers. It does **not** own the offline native tool behavioral matrix (that stays with
deterministic provider cassette, harness-tools, and harness-tui owner tests).

### Live smoke pack (residual PRD WS-L1 / WS-L2)

A **budgeted live smoke pack** is available as an opt-in agent channel (not CI default):

```bash
# Fail-closed without live env (must exit non-zero):
bash scripts/harness-qa-live-smoke.sh --self-test-fail-closed

# With live env (fixed short smoke + budgets + redacted evidence):
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
bash scripts/harness-qa-live-smoke.sh --slug <short-slug>
```

- Runtime skill channel: `.agent-harness/skills/harness-qa/` live section invokes the script.
- Evidence root: `artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/` (README, commands.log,
  isolation-receipt, budget-receipt, events-excerpt, secret-scan, lane-or-run-summary).
- Fixed smoke list: preflight env/config/provider/model; one short non-tool prompt; optional
  one env-safe tool path only if `HARNESS_LIVE_SMOKE_TOOL=1` (never documented as matrix ownership).
- Budgets: short prompts, max turns 1–3, wall-clock cap, cost if available else unmetered, secret
  hard-fail.
- **T5 non-ownership:** live smoke proves transport/auth/fixed smoke only; it does **not** re-own
  the native tool behavioral matrix.
- Non-claims: not freestyle quality; not multi-provider matrix; not PTY/native; not offline dogfood
  substitute; not CI default.

Slim `live_proxy_e2e` wrappers still write **no** live artifact trees by design; the smoke pack
script is the budgeted evidence path.

### signoff-live lane

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
scripts/test-lanes.sh signoff-live
```

Required live environment:

- `HARNESS_LIVE_PROXY=1`
- `HARNESS_LIVE_PROXY_CONFIG=<path>`
- `HARNESS_LIVE_PROXY_PROVIDER=<provider>`
- `HARNESS_LIVE_PROXY_MODEL=<model>`

`signoff-live` fails closed when the live environment is missing. When the environment is present,
it runs `live_proxy_preflight_requires_live_env` first, then the prompt wrapper, then the TUI wrapper.
The execution order is documented in
[`crates/harness-testkit/tests/README.live-proxy.md`](../../crates/harness-testkit/tests/README.live-proxy.md):
CLI signoff runs `live_proxy_preflight_requires_live_env` and `live_proxy_prompt_signoff`;
TUI signoff runs `live_proxy_preflight_requires_live_env` and
`live_proxy_e2e_tui_signoff`.

Current stage commands:

- `cargo nextest run -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact`
- `cargo nextest run -p harness-testkit live_proxy_prompt_signoff -- --ignored --exact`
- `cargo nextest run -p harness-testkit live_proxy_e2e_tui_signoff -- --ignored --exact`

Use the live README for exact preflight details, optional live vars, artifacts, retention, and
agent iteration order instead of duplicating that contract here.

Optional local free live targets (for example Ollama) are **deferred non-CI residual (WS-L4)** and
are not part of `signoff-live` or default quality gates. See
[`docs/configuration/provider-support.md`](../configuration/provider-support.md).

Open-ended live freestyle eval missions (for example benchmark sweeps or open-ended agent
missions) are **rejected as CI or release proof** for V1. Local human experimentation is fine, but
it is not evidence for release readiness.


## Binary shim smoke

The single real-process CLI shim smoke is ignored by default and excluded from the deterministic
nextest profile. Run it only when validating the compiled `main.rs` wiring:

```bash
scripts/test-lanes.sh signoff-binary
```

`signoff-binary` sets `HARNESS_BINARY_SMOKE=1` plus `HARNESS_BINARY_SMOKE_ARTIFACT_DIR` and runs the ignored
`cargo nextest run -p harness --test binary_smoke -- --ignored --exact` stage through the canonical
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

- `cargo nextest run -p harness-testkit --test native_visual_e2e --test-threads 1 -- --ignored`

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
HARNESS_LIVE_PROXY_CONFIG=harness.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=umans-ai-coding-plan \
HARNESS_LIVE_PROXY_MODEL=umans-kimi-k2.7 \
scripts/test-lanes.sh stress-live
```

`stress-offline` delegates to `scripts/stress-harness.sh --mode offline`. `stress-live` uses the
same live env guard as `signoff-live` and delegates to `scripts/stress-harness.sh --mode live` with
`--config` set from `HARNESS_LIVE_PROXY_CONFIG`. Both stress lanes add `--artifact-dir`, and both
add `--harness-bin` when a binary was supplied to `scripts/test-lanes.sh` or an existing
`target/debug/harness` can be reused.

## Scenario growth policy

New scenarios and simulation matrix admissions follow this policy:

1. Prefer focused owner nextest over simulation matrix admission.
2. New CLI scenarios are fine when they have named owners.
3. Matrix `offline-deterministic` admission happens only after measured `expected_predicates` and a simulation lane update plan.
4. Never grow `golden_path` into an unmaintainable mega-scenario.
5. No new INV ids by default.

## Deletion policy and invariant map

Before deleting or narrowing tests, update the test-suite overhaul evidence rather than relying on
memory. Every deletion needs a preserved invariant owner in the current map, or replacement coverage
that proves the same behavior before the old test is removed.

Current invariant owners:

| Protected invariant | Owning tests / lane |
|---|---|
| Coordinator scheduling, cancellation, failed-turn handling, compaction, and tool lifecycle | `cargo nextest run -p harness-core --test coord_test`; focused chunks under `crates/harness-core/tests/coord/` |
| Replay purity and projection derivation from append-only events | `cargo nextest run -p harness --test replay_sessions_cli_test`; `cargo nextest run -p harness-core --test conversation_projection_test`; `cargo nextest run -p harness-core --test transcript_projection_test`; `cargo nextest run -p harness-core --test resume_plan_test`; `cargo nextest run -p harness-core --test session_lineage_materialization_test` |
| Permission checks and redelegation guard | `cargo nextest run -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test`; `cargo nextest run -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` |
| Native tool catalog and stable public tool IDs | `cargo nextest run -p harness-tools` |
| Doctor/support catalog metadata and redaction | `cargo nextest run -p harness --test config_schema_cli_test doctor_cli`; `cargo nextest run -p harness --test replay_sessions_cli_test sessions_export_cli_support_includes_readiness_and_config_summaries`; `cargo nextest run -p harness --test replay_sessions_cli_test sessions_export_cli_redacts_support_bundle_secret_shapes` |
| Provider serialization, replay-only cassettes, redaction, and checkpoint accounting | `cargo nextest run -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test`; `cargo nextest run -p harness-providers --test recorded_test`; `cargo nextest run -p harness-testkit --test secretscan_test` |
| Offline deterministic simulation matrix, semantic predicates, same-seed normalization, artifact index, and simulation redaction | `scripts/test-lanes.sh simulation`; `cargo nextest run -p harness-testkit --test simulation_validator_test`; `cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root <dir> --matrix docs/testing/simulation-matrix.json --baseline-events <events.jsonl> --baseline-replay <replay.json> --repeat-events <events.jsonl> --repeat-replay <replay.json> --seed 0` |
| Config/event docs drift and public schema generation | `cargo nextest run -p harness --test config_docs_reference_test`; `cargo nextest run -p harness --test event_docs_reference_test`; `cargo nextest run -p harness --test config_schema_cli_test` |
| Deterministic UI content rendering, transcript layout, and navigation | `cargo nextest run -p harness-tui --test deterministic_render_test`; `cargo nextest run -p harness-tui --test lineage_view_model_test`; `cargo nextest run -p harness-tui --test model_switcher_metadata_test`; `cargo nextest run -p harness-tui --test session_navigation_keybindings_test`; `cargo nextest run -p harness-tui --test pty_e2e` as the fail-closed helper lane |
| TUI visual/provenance flow coverage | `cargo nextest run -p harness-tui --test deterministic_render_test`; `env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1`; `scripts/test-lanes.sh signoff-pty` |
| Live, PTY, native visual provenance contracts | `scripts/test-lanes.sh signoff-pty`; `scripts/test-lanes.sh signoff-live`; `scripts/test-lanes.sh signoff-native` as opt-in T5 lanes only |

The acceptance owner map above is the source of truth for the test-suite overhaul. Concrete lane
artifacts land under `target/test-suite-overhaul/` when those stages run.
