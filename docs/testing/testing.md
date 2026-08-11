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
scripts/test-lanes.sh signoff-parity
scripts/test-lanes.sh signoff-journeys
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
real-world dependency usage, widened test-file focus, T5 tree-total line budget, arrange/act/assert
conventions, cassette secret hygiene, committed snapshot orphans, and test taxonomy. Any residual
arrange/act/assert debt is stored as SHA-256 keys in `docs/test-suite-conventions-baseline.json`;
the gate fails on new or stale debt without storing source-brand terms in docs. The baseline is
currently empty (Wave 3 Packet 3.5 fixed all 81 listed tests by adding arrange/act/assert
sections and removed every exemption); re-adding entries requires explicit approval and a
documented removal path, because the goal is to keep this file at zero debt. Committed `.snap`
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
| T-multi-tool | `dogfood_harness_jsonc_tool_parity_test`; `determinism_multi_turn_tools_test` |
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
| `harness_tui_pty_e2e` | harness-tui PTY E2E under `HARNESS_TUI_PTY_SIGNOFF=1` |
| `harness_tui_happy_path_pty` | compiled `harness` CLI mock happy path (`pty_happy_path_recorded`) |
| `harness_tui_dual_binary_cli_pty` | compiled CLI dual-binary structural smokes (`dual_binary_cli_pty_*`, 12): startup, overlay keybinds, secondary surfaces (status/model/toggles), scenario permission allow+deny, scenario question open+resolve, scenario auto-complete, mock success+fail chrome, Ctrl+W worktree create, Ctrl+S resume seeded session — under `HARNESS_TUI_PTY_SIGNOFF=1` + strict |
| Aggregate `pty-lane-verdict.txt` | machine-readable PASS/FAIL under dual-binary stage artifacts |

For a combined deterministic closeout, use:

- `env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=<dir> cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored --exact scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_TUI_PARITY_STRICT=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=<dir> cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored dual_binary_cli_pty`

The strict-V1 TUI signoff manifest is checked in at
[`docs/testing/tui-signoff-manifest.v1.json`](tui-signoff-manifest.v1.json). Its schema version is
`harness-tui-signoff-manifest-v1`, and each flow row names:

- deterministic owner tests/snapshots,
- `signoff-pty` artifact stages,
- an explicit note that reference-image comparison is not required, and
- the native-visual policy for env-gated local screenshots.

Required flow coverage is startup, command palette, session picker/resume, permission/question,
provider/tool failure, and diff review. `cargo nextest run -p harness-tui --test deterministic_render_test`
guards the manifest shape, required flows, deterministic owner tests, and the no-reference-image-comparison policy. `env RUST_TEST_THREADS=1 cargo nextest run
-p harness-testkit --test pty_e2e --test-threads 1` copies the manifest and a summary into
`target/pty-visual-artifacts/` for lane provenance. Native visual remains a separate local
provenance class: when `HARNESS_NATIVE_VISUAL=1` and `DISPLAY=<display>` are missing, the manifest
records a documented gap rather than silently converting PTY evidence into native screenshot proof.

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
parity-name** lane: env/config/provider-model tuple checks and the retained prompt/TUI signoff
wrappers. It does **not** own the offline native tool behavioral matrix (that stays with
deterministic provider cassette, harness-tools parity, and harness-tui owner tests).

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

### signoff-live lane (preflight + parity names)

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
it runs `live_proxy_preflight_requires_live_env` first, then the prompt parity wrapper, then the TUI parity wrapper.
The underlying parity order is documented in
[`crates/harness-testkit/tests/README.live-proxy.md`](../../crates/harness-testkit/tests/README.live-proxy.md):
CLI parity runs `live_proxy_preflight_requires_live_env` and `live_proxy_prompt_parity_signoff`;
TUI parity runs `live_proxy_preflight_requires_live_env` and
`live_proxy_e2e_tui_parity_signoff`.

Current stage commands:

- `cargo nextest run -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact`
- `cargo nextest run -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact`
- `cargo nextest run -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact`

Use the live README for exact preflight details, optional live vars, artifacts, retention, and
agent iteration order instead of duplicating that contract here.

Optional local free live targets (for example Ollama) are **deferred non-CI residual (WS-L4)** and
are not part of `signoff-live` or default quality gates. See
[`docs/configuration/provider-support.md`](../configuration/provider-support.md).

Open-ended live freestyle eval missions (for example benchmark sweeps or open-ended agent
missions) are **rejected as CI or release proof** for V1. Local human experimentation is fine, but
it is not evidence for release readiness.

## Strict A-JOURNEYS scaffolding lane

`signoff-journeys` is a **strict fail-closed** lane for journey-template rows in
[`docs/reference/tui-reference-parity-manifest.v1.json`](../reference/tui-reference-parity-manifest.v1.json):

- `JOURNEY-CONFIG-SHOW-EFFECTIVE` — real-process `harness config show --effective`
- `JOURNEY-CONFIG-SOURCES-EXPLAIN` — real-process `harness config sources` + `config explain`
- `JOURNEY-WORKTREE-CTRL-W` — owner documented only; dual-binary PTY remains
  `HARNESS_TUI_PTY_SIGNOFF=1` via `pty_happy_path_recorded::dual_binary_cli_pty_worktree_ctrl_w_creates_git_worktree`
- `JOURNEY-WAIT-ANY-ALL` — owner-doc only (`orchestration.wait_any` / wait-all L2/L5/L6)
- `JOURNEY-FOLDER-TRUST-DENY` — owner-doc only (`workspace.folder_trust`)
- `JOURNEY-MEMORY-CLI` — owner-doc only (`memory.durable_product_surface`)
- `JOURNEY-ALWAYS-APPROVE-MODE` — owner-doc only (`permission.always_approve_mode`)
- `JOURNEY-SETTINGS-EDITOR` — owner-doc only (`tui.settings_editor`)

```bash
scripts/test-lanes.sh signoff-journeys
scripts/test-lanes.sh signoff-journeys --dry-run
```

Ownership:

- **Owns:** offline deterministic CLI journey evidence + worktree owner documentation for A-JOURNEYS
  scaffolding (`crates/harness/tests/journey_signoff_test.rs`).
- **Does not own:** full L1–L6 freeze/pixel/PTY chains, `signoff-parity` cells/pixels, or flipping
  journey rows to `pass` without the complete evidence chain.

Fail-closed stages (no `|| true`):

- Prerequisites: `journey_signoff_test.rs` must exist; `cargo` on `PATH` (missing owner = FAIL)
- `cargo nextest run -p harness --test journey_signoff_test` with
  `HARNESS_JOURNEY_STRICT=1` and `HARNESS_JOURNEY_ARTIFACT_DIR` pointing at the lane artifact tree
- Aggregate `journey-lane-verdict.txt` under the lane artifact tree

`HARNESS_JOURNEY_STRICT=1` enables fail-closed evidence validation: referenced L1/L3/L4/L6 paths
under the gitignored parity evidence root must exist on disk. Ordinary `cargo nextest` runs do not
set this variable and only validate manifest structure plus committed source owners, so the suite
passes from a clean checkout without pre-existing signoff artifacts.

Missing compiled harness binary fails the owner tests (no skip). Journey rows stay `incomplete`
until L1–L6 are complete; this lane only scaffolds L5/L6 owners.

## Strict dual-binary reference parity lane

`signoff-parity` is the **strict fail-closed** lane for dual-binary TUI reference parity (semantic
cells and rendered pixels). It does **not** use the soft `|| true` stage pattern of other lanes:
missing prerequisites, missing owners, timeouts, or stage failures fail the run. Silent skip is
forbidden.

```bash
scripts/test-lanes.sh signoff-parity
scripts/test-lanes.sh signoff-parity --dry-run
```

Ownership:

- **Owns:** dual-binary cells/pixels acceptance against
  [`docs/reference/tui-reference-parity-manifest.v1.json`](../reference/tui-reference-parity-manifest.v1.json) (independent
  of the older signoff manifest).
- **Does not own:** [`docs/testing/tui-signoff-manifest.v1.json`](tui-signoff-manifest.v1.json) flow
  coverage — that remains with `signoff-pty` / `tui_signoff_manifest_test` and is not a dual-binary
  cells/pixels gate.

The standalone `tui-fidelity compare` runner writes `cleanup.json` with schema
`harness.tui-fidelity.cleanup.v3`. `detected_child_pids` records unexpected descendants alive at
the cleanup boundary; `surviving_pids` contains only descendants still alive after termination and
the bounded reap wait. A detected descendant still fails the comparison even when cleanup later
reaps it, while the receipt keeps those two facts distinct.

Current fail-closed stages (no `|| true`):

- Prerequisites gate: independent reference-parity manifest path must exist; `cargo` must be on
  `PATH`; all owner test files listed below must exist (missing owner = FAIL, not skip).
- `test -f docs/reference/tui-reference-parity-manifest.v1.json`
- `reference_binary_present`: the pinned reference binary
  `inspirations/grok-build/target/debug/xai-grok-pager` must exist and its SHA-256 must match the
  pinned digest `883e3dea…3bb9a9a5` (presence and digest check only; the lane never rebuilds or
  copies the binary)
- Fresh L3 capture stages (`reference_parity_capture_*`, including
  `reference_parity_capture_shell_lifecycle` for the 7 shell lifecycle rows): each stage runs a
  real PTY capture rendered through xterm.js/Chromium and writes `terminal.png`, `terminal.txt`,
  `terminal-ansi.txt`, and `metadata.json` under the lane's fresh evidence root. A failed capture
  fails the lane (no silent skip)
- Fresh nonvisual journey L3 capture stages (`reference_parity_capture_journey_*`, one per
  journey row): each stage runs `scripts/tui-parity/capture-journey-l3.sh <key>`, which invokes
  the A-JOURNEYS owner tests in `crates/harness/tests/journey_signoff_test.rs` in self-contained
  mode (CLI/backend evidence only — no Chrome, no pixel PNG), relocates the generated
  `journey-*-v1` evidence directory into the lane's fresh evidence root, and writes a provenance
  `metadata.json` (`behavior_id` + `generating_command`) for the strict provenance validator
- Fresh terminal capability L3 capture stage (`reference_parity_capture_term_cap`, shared by the
  4 `TERM-CAP-*` rows): runs `scripts/tui-parity/capture-term-cap-l3.sh` with `EVIDENCE_DIR`
  pointed at the lane's fresh `actual/harness-term-cap-v1` directory. The script exports
  `HARNESS_TERMCAP_ARTIFACT_DIR` to a temp work dir and invokes
  `cargo nextest run -p harness-tui --test terminal_capability_matrix_capture_test`, whose
  env-gated owner test derives the Harness negotiated terminal mode set from the L2 owner
  `crates/harness-tui/src/runtime.rs`, asserts exact parity with the pinned reference enabled
  mode set (fail-closed), and writes `harness-term-cap-v1/term-cap-matrix.json`. The script
  relocates that receipt into `EVIDENCE_DIR` and writes the provenance `metadata.json`
  (`behavior_ids` + `generating_command`). Journey-style L3+receipt contract — no Chrome, no
  pixel PNG. A failed capture fails the lane (no silent skip)
- Evidence-generation stages after the captures: `reference_parity_freeze_receipt` writes the
  pinned freeze receipt; `reference_parity_generate_evidence_layers` builds evidence only for
  visual rows currently claimed as `pass`/`diverged`, plus all claimed journey and terminal-
  capability rows. Copied receipts remain immutable; embedded digests must already match the
  fresh artifacts or the final provenance gate fails closed
- `cargo nextest run -p harness-tui --test reference_parity_manifest_test`
- `cargo nextest run -p harness-tui --test p0_parity_contract_test`
- `cargo nextest run -p harness-tui --test shell_topology_contract_test`
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_cells_test`
  (missing freeze/actual cell evidence fails closed; soft-skip forbidden)
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_pixels_test`
  (missing freeze PNG evidence fails closed)
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_first_slice_test`
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_perm_question_test`
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_tx_shell_test`
- `HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_responsive_test`
- `HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_TUI_PARITY_STRICT=1 cargo nextest run -p harness-tui --test reference_parity_pty_test`
  (forces PTY owners on; silent no-op without the env is forbidden in this lane)
- `reference_parity_manifest_evidence` (final gate):
  `HARNESS_TUI_PARITY_STRICT=1 HARNESS_TUI_PARITY_ARTIFACT_DIR="$parity_artifacts_dir" cargo nextest run -p harness-tui --test reference_parity_evidence_test`
  runs the strict validator (`validate_manifest_evidence` in
  `crates/harness-tui/tests/support/reference_parity_status.rs`) against the lane's fresh evidence
  root under `target/test-lanes/`, never the repository `artifacts/` tree. Every claimed
  (`pass`/`diverged`) row must have its applicable lane-artifact evidence files present; source
  owner paths remain structural ownership references and are not copied into the evidence root. Declared capture digests
  (`reference_txt_sha256`/`reference_png_sha256`) must hash-match the actual artifact bytes,
  embedded receipt `path`/`sha256` pairs must hash-match, the freeze receipt must match the pinned
  reference block (binary digest, freeze txt/png digests, scenario, viewport), divergence
  approval receipt files must exist, and L3 `metadata.json` behavior_id/viewport provenance must
  match the rows owning that capture. Any missing, stale, copied, or mismatched artifact fails
  the lane.
- Aggregate `parity-lane-verdict.txt` under the lane artifact tree, including an explicit
  `stages=` list of the owners that ran and `parity_complete=true|false` derived from the
  independent manifest rows. A lane `verdict=PASS` proves every required stage passed; it does
  not turn manifest rows still marked `incomplete` or `blocked` into parity claims.

Ordinary `cargo nextest` runs do not set `HARNESS_TUI_PARITY_STRICT`, so the env-gated strict
provenance test stays inert and the suite passes from a clean checkout without signoff artifacts.
Only the lane (with the fresh evidence root populated by the capture flow) drives the executable
provenance validation.

`--dry-run` still records the same stage command shape without executing. Optional live/native
lanes (`signoff-live`, `signoff-native`) and developer lanes (`fast`, …) keep soft-stage semantics.
`signoff-pty` is fail-closed (see Deterministic signoff PTY lane).

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
| Native tool parity and stable public tool IDs | `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test` |
| Doctor/support catalog metadata and redaction | `cargo nextest run -p harness --test config_schema_cli_test doctor_cli`; `cargo nextest run -p harness --test replay_sessions_cli_test sessions_export_cli_support_includes_readiness_and_config_summaries`; `cargo nextest run -p harness --test replay_sessions_cli_test sessions_export_cli_redacts_support_bundle_secret_shapes` |
| Provider serialization, replay-only cassettes, redaction, and checkpoint accounting | `cargo nextest run -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test`; `cargo nextest run -p harness-providers --test recorded_test`; `cargo nextest run -p harness-testkit --test secretscan_test` |
| Offline deterministic simulation matrix, semantic predicates, same-seed normalization, artifact index, and simulation redaction | `scripts/test-lanes.sh simulation`; `cargo nextest run -p harness-testkit --test simulation_validator_test`; `cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root <dir> --matrix docs/simulation-matrix.json --baseline-events <events.jsonl> --baseline-replay <replay.json> --repeat-events <events.jsonl> --repeat-replay <replay.json> --seed 0` |
| Config/event docs drift and public schema generation | `cargo nextest run -p harness --test config_docs_reference_test`; `cargo nextest run -p harness --test event_docs_reference_test`; `cargo nextest run -p harness --test config_schema_cli_test` |
| Deterministic UI content rendering, transcript layout, and navigation | `cargo nextest run -p harness-tui --test deterministic_render_test`; `cargo nextest run -p harness-tui --test lineage_view_model_test`; `cargo nextest run -p harness-tui --test model_switcher_metadata_test`; `cargo nextest run -p harness-tui --test session_navigation_keybindings_test`; `cargo nextest run -p harness-tui --test pty_e2e` as the fail-closed helper lane |
| TUI signoff manifest and visual/provenance flow coverage | `cargo nextest run -p harness-tui --test deterministic_render_test tui_signoff_manifest_covers_required_release_flows`; `env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1 pty_signoff_manifest_declares_required_flow_artifacts`; `scripts/test-lanes.sh signoff-pty` |
| Live, PTY, native visual provenance contracts | `scripts/test-lanes.sh signoff-pty`; `scripts/test-lanes.sh signoff-live`; `scripts/test-lanes.sh signoff-native` as opt-in T5 lanes only |
| Dual-binary TUI reference parity (cells/pixels) | `scripts/test-lanes.sh signoff-parity` (strict fail-closed; owns `docs/tui-reference-parity-manifest.v1.json`). Does **not** use `tui-signoff-manifest.v1.json`. |
| A-JOURNEYS scaffolding (config CLI + worktree owner doc) | `scripts/test-lanes.sh signoff-journeys` (strict fail-closed; owns `crates/harness/tests/journey_signoff_test.rs`). Rows stay `incomplete` until full L1–L6. |

Retired harness-tui PTY helper scenario owners:

| Removed T5 helper scenario | Surviving deterministic owner |
|---|---|
| Startup shell / startup palette / startup session history | `cargo nextest run -p harness-tui --test deterministic_render_test startup_shell_is_compose_first_without_pty command_palette_renders_without_pty startup_session_history_picker_renders_without_pty`; `cargo nextest run -p harness-tui startup_slash_commands_execute_without_menu command_palette_renders_and_filters` |
| Streamed response and completed live shell | `cargo nextest run -p harness-tui live_shell_enter_submits_and_echoes_prompt_snapshot live_shell_type_first_input_snapshot`; `cargo nextest run -p harness-tui --test deterministic_render_test live_transcript_and_composer_shell_render_without_pty` |
| Tool lifecycle and inline diff parity | `cargo nextest run -p harness-tui --test deterministic_render_test tool_lifecycle_rows_stay_ordered_without_pty`; `cargo nextest run -p harness-tui transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs transcript_inline_diff_stays_compact_between_tool_rows transcript_native_edit_renders_inline_diff_from_artifact` |
| Permission and question overlays | `cargo nextest run -p harness-tui permission_modal_preempts_palette_and_slash permission_overlay_preserves_draft_and_transcript_context permission_overlay_ignores_plain_draft_input_once_prompt_is_active`; `cargo nextest run -p harness-tui --test deterministic_render_test permission_modal_preserves_draft_without_pty question_permission_prompt_renders_without_pty` |
| Full-width live shell, secondary operator surfaces, and orchestration states | `cargo nextest run -p harness-tui --test deterministic_render_test live_transcript_and_composer_shell_render_without_pty`; `cargo nextest run -p harness-tui --test shell_topology_contract_test`; `cargo nextest run -p harness-tui operator_sidebar_preserves_section_order_and_copy live_shell_details_drawer_orchestration_snapshot orchestration_projection_tracks_queued_started_completed_counts orchestration_projection_tracks_stale_then_late_result` |
| Degraded/disconnected/replay shells | `cargo nextest run -p harness-tui live_shell_degraded_bootstrap_snapshot live_shell_disconnected_stream_snapshot`; `cargo nextest run -p harness-tui --test deterministic_render_test replay_shell_is_read_only_without_pty` |

Narrowed harness-testkit PTY assertion owners:

| Removed or narrowed T5 assertion | Surviving owner |
|---|---|
| Duplicate operator-sidebar screen-string assertions in `pty_e2e_sidebar_session_parity` and `pty_helper_operator_sidebar_session_contract` | `cargo nextest run -p harness-tui --test deterministic_render_test live_transcript_and_composer_shell_render_without_pty`; `cargo nextest run -p harness-tui --test shell_topology_contract_test`; `cargo nextest run -p harness-tui operator_sidebar_preserves_section_order_and_copy`; remaining T5 manifest-backed screenshots assert only smoke/provenance markers. |
| Duplicate permission-overlay screen-string assertions in `pty_e2e_permission_dock_parity` and `pty_helper_permission_with_draft` | `cargo nextest run -p harness-tui permission_modal_preempts_palette_and_slash permission_overlay_preserves_draft_and_transcript_context permission_overlay_ignores_plain_draft_input_once_prompt_is_active`; `cargo nextest run -p harness-tui --test deterministic_render_test permission_modal_preserves_draft_without_pty`; remaining T5 captures keep permission smoke/provenance markers. |

Retired harness-testkit T5 scenario owners:

| Removed T5 scenario group | Surviving owner |
|---|---|
| PTY startup, command palette, replay/continue history, and type-first shell content checks | `cargo nextest run -p harness-tui --test deterministic_render_test startup_shell_is_compose_first_without_pty startup_session_history_picker_renders_without_pty replay_shell_is_read_only_without_pty`; `cargo nextest run -p harness-tui startup_slash_commands_execute_without_menu command_palette_renders_and_filters`; slim `cargo nextest run -p harness-testkit --test pty_e2e` keeps only single-thread/env/artifact-path smoke. |
| PTY transcript, native-tool row, inline diff, MCP/background, and dense-log screen checks | `cargo nextest run -p harness-tui --test deterministic_render_test tool_lifecycle_rows_stay_ordered_without_pty live_transcript_and_composer_shell_render_without_pty`; `cargo nextest run -p harness-tui transcript_apply_patch_surfaces_rename_and_wrapped_inline_diffs transcript_inline_diff_stays_compact_between_tool_rows transcript_native_edit_renders_inline_diff_from_artifact`; `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test`. |
| PTY child-session, lineage, replay-read-only, active/unrestorable continue rejection checks | `cargo nextest run -p harness --test replay_sessions_cli_test`; `cargo nextest run -p harness-tui --test lineage_view_model_test`; `cargo nextest run -p harness-tui --test session_navigation_keybindings_test`; `cargo nextest run -p harness-core --test session_lineage_materialization_test`. |
| Live proxy prompt/TUI/native tool-flow, provider parity, config mutation, request/evidence, and wiremock checks | `cargo nextest run -p harness-providers --test recorded_test`; `cargo nextest run -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes_test`; `cargo nextest run -p harness-testkit --test live_proxy_e2e` for env-gated signoff names and fail-closed config preflight. |
| Live visual manifest, vision, screenshot evidence, and artifact-retention checks | `cargo nextest run -p harness-tui --test deterministic_render_test`; `cargo nextest run -p harness-testkit --test native_visual_e2e`; opt-in `scripts/test-lanes.sh signoff-native` for local screenshot provenance only. |
| Native visual startup geometry, navigation, permission, transcript, operator-sidebar, Ghostty, capture-helper, and managed-session checks | `cargo nextest run -p harness-tui --test deterministic_render_test`; `cargo nextest run -p harness-tui permission_modal_preempts_palette_and_slash operator_sidebar_preserves_section_order_and_copy`; `cargo nextest run -p harness-testkit --test native_visual_e2e`; `cargo nextest run -p harness-testkit --bin native_visual_helper -- --help` when validating the helper CLI. |
| Shared T5 fixture/rendering helpers (`harness_bin`, session fixtures, temp paths, visual renderer, manifest writers, markers) | `cargo nextest run -p harness-testkit --lib`; `cargo nextest run -p harness-testkit --test secretscan_test`; deterministic fixture ownership moved to crate-local test helpers and harness-tui render fixtures rather than uncompiled T5 support. |

The acceptance owner map above is the source of truth for the test-suite overhaul. Concrete lane
artifacts land under `target/test-suite-overhaul/` when those stages run.
