# Rust Agent Harness — Plan Compliance Remediation + Full E2E (App + TUI)

## TL;DR
> **Summary**: Bring the repo into full compliance with `.sisyphus/plans/rust-agent-harness-foundation.md` tasks **1–26** by fixing the remaining/mis-implemented TUI + CI + PTY E2E items, then execute end-to-end validation for both the CLI application and the fullscreen Ratatui UI.
> **Deliverables**:
> - Working `harness tui` **live** mode (scenario-driven) + **replay** mode (JSONL)
> - Permission modal approvals (`a`/`d`) in TUI
> - Diff viewer wired to hashline edit diff artifacts, with `EditApplied.diff_*` populated
> - Linux PTY E2E tests (portable-pty + vt100 + insta snapshots) + GitLab CI job
> - Evidence outputs proving plan compliance + E2E pass
> **Effort**: Large
> **Parallel**: YES — 4 waves
> **Critical Path**: Coordinator event-stream exposure → `harness tui` wiring → TUI live+permission+diff → PTY E2E → CI → Final E2E

## Context

### Original Request
- Audit the last work plan in `.sisyphus/plans/` and verify everything was implemented correctly.
- If anything is missing/incorrect: fix it.
- After fixes: run E2E testing on the application and the user interface.

### Interview Summary
- Scope locked to **tasks 1–26 only** (skip Optional Task 27).

### Research Findings (repo truth)
- GitLab CI has Rust fmt/clippy/test jobs but **no** deterministic env vars and **no PTY E2E job**: `.gitlab-ci.yml:18-38`.
- `harness tui` CLI is still a stub (no replay/live wiring): `crates/harness/src/tui.rs:20-27`.
- TUI currently renders placeholders and does not subscribe to Coordinator/store (no live mode): `crates/harness-tui/src/lib.rs:12-40`, `crates/harness-tui/src/ui.rs:145-155`.
- Permission modal UI is not implemented (no `a/d` handling): `crates/harness-tui/src/app.rs:52-71`.
- Hashline tool already writes a diff artifact, but `EditApplied` events do **not** include diff refs (hardcoded `None`):
  - tool: `crates/harness-tools/src/hashline_apply.rs:46-64`
  - coordinator: `crates/harness-core/src/coord.rs:1200-1212` and `:2141-2149`
- `harness-testkit` is placeholder; PTY E2E harness/tests are absent.

### Metis Review (gaps addressed)
- Live TUI cannot “subscribe” by opening a second store instance: `EventStore::subscribe()` live updates require the *same* store instance / broadcast sender (`crates/harness-core/src/store.rs:303-317`). Plan must expose the coordinator’s in-process event stream.
- Fix clap so `harness tui --replay <run_dir>` is reachable (scenario must not be required in replay mode): `crates/harness/src/tui.rs:8-17`.
- Remove any CI-path human input; PTY automation must be the only interactive approval path.
- Populate `EditApplied.diff_rel_path/diff_digest` using tool artifacts; add an agent-executable Rust test proving `EditApplied.diff_digest == ArtifactWritten.digest`.
- Define handling for `EventStoreError::SubscriberLagged` (resync via replay).

## Work Objectives

### Core Objective
Ship the missing pieces so the repository **fully implements tasks 1–26** from the foundation plan and has CI-grade **E2E coverage** for both the CLI and the fullscreen Ratatui UI.

### Definition of Done (agent-verifiable)
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --workspace --all-features` passes offline.
- [ ] Headless deterministic golden path is stable (already required by original plan):
  - `HARNESS_DETERMINISTIC=1 cargo test -p harness deterministic_golden_path_twice_produces_identical_sha256_digest` passes (`crates/harness/src/run.rs:366-395`).
- [ ] `harness tui --replay <run_dir>` works (no scenario required) and renders event variant names.
- [ ] In a deterministic run, `EditApplied` includes diff refs and they match `ArtifactWritten`:
  - `diff_rel_path` and `diff_digest` are non-null, and the digest matches the artifact_written event for the same path.
- [ ] Linux PTY E2E passes and is stable across 5 repeats:
  - `INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e` passes.
  - Repeat loop (5×) passes without flakes.
- [ ] GitLab CI includes a PTY E2E job + deterministic env vars while keeping SAST/Secret Detection.

### Must Have
- Follow the original plan’s constraints: Ratatui + Crossterm only; Linux-first; deterministic E2E.
- No human input in CI-path verifications.
- TUI features required by remaining tasks: live mode, replay mode, permission modal, diff viewer.

### Must NOT Have (guardrails)
- Do **NOT** implement Optional Task 27 (VCR/rpc control plane) — explicitly out of scope.
- Do **NOT** introduce new UI frameworks or OpenTUI.
- Do **NOT** redesign event schema beyond populating existing optional fields.
- Do **NOT** add cross-platform PTY ambitions; Linux-only is acceptable.

## Verification Strategy
> ZERO HUMAN INTERVENTION — all verification is agent-executed.
- Test decision: **tests-after** for UI behavior, but add targeted unit tests where state machines are involved (permission modal, replay loader).
- E2E testing:
  - Headless: existing deterministic golden path tests in `crates/harness/src/run.rs`.
  - UI: PTY E2E using portable-pty + vt100 + insta snapshots (Linux-only).
- Evidence: `.sisyphus/evidence/task-{N}-{slug}.{ext}`

## Execution Strategy

### Parallel Execution Waves
Wave 1 (Core wiring + contracts)
- Tasks 1–4

Wave 2 (TUI behavior: replay + live + permission + output + diff)
- Tasks 5–8

Wave 3 (PTY E2E + CI)
- Tasks 9–10

Wave 4 (End-to-end verification + plan compliance closure)
- Tasks 11–12

### Dependency Matrix (full)
| Task | Wave | Depends On |
|------|------|------------|
| 1 | 1 | — |
| 2 | 1 | — |
| 3 | 1 | — |
| 4 | 1 | 2 |
| 5 | 2 | 4 |
| 6 | 2 | 2,4 |
| 7 | 2 | 6 |
| 8 | 2 | 3,5,6 |
| 9 | 3 | 4-8 |
| 10 | 3 | 9 |
| 11 | 4 | 1-10 |
| 12 | 4 | 11 |

## TODOs
> Implementation + Test = ONE task. Never separate.
> EVERY task MUST include agent-executable QA scenarios and concrete acceptance criteria.

- [x] 1. Plan Compliance Baseline (current-state verification + evidence capture)

  **What to do**:
  - Run the full verification commands against current repo state and capture outputs to evidence files:
    - `cargo fmt --check`
    - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `cargo test --workspace --all-features`
    - `HARNESS_DETERMINISTIC=1 INSTA_UPDATE=no cargo test --workspace --all-features`
  - If any failures occur, open follow-up fix PRs within this execution plan’s later tasks (do not create out-of-plan work).

  **Must NOT do**:
  - Do not change requirements/scope based on failures; only fix regressions to meet existing plan intent.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: cross-workspace verification + triage
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 10-12 | Blocked By: none

  **References**:
  - DoD commands: `.sisyphus/plans/rust-agent-harness-foundation.md:53-61`

  **Acceptance Criteria**:
  - [ ] Evidence files exist with command outputs:
    - `.sisyphus/evidence/task-1-fmt.txt`
    - `.sisyphus/evidence/task-1-clippy.txt`
    - `.sisyphus/evidence/task-1-test.txt`
    - `.sisyphus/evidence/task-1-deterministic-tests.txt`

  **QA Scenarios**:
  ```
  Scenario: Baseline verification
    Tool: Bash
    Steps:
      - cargo fmt --check |& tee .sisyphus/evidence/task-1-fmt.txt
      - cargo clippy --workspace --all-targets --all-features -- -D warnings |& tee .sisyphus/evidence/task-1-clippy.txt
      - cargo test --workspace --all-features |& tee .sisyphus/evidence/task-1-test.txt
      - HARNESS_DETERMINISTIC=1 INSTA_UPDATE=no cargo test --workspace --all-features |& tee .sisyphus/evidence/task-1-deterministic-tests.txt
    Expected: all commands exit 0
    Evidence: .sisyphus/evidence/task-1-*.txt
  ```

  **Commit**: NO

- [x] 2. Expose Coordinator event stream for in-process subscribers (required for live TUI)

  **What to do**:
  - Update coordinator runtime so the live TUI can subscribe to the *same* in-process event stream:
    - Change `RunState.event_store` to be shareable (`Arc<...>`), not a private value.
    - Add a new coordinator command + handle method to fetch the shared store after `StartRun`.
      - **Decision locked**: implement `CoordinatorHandle::event_store()` returning `Arc<dyn EventStore>`.
      - Implement via new `Command::GetEventStore { respond_to }` which returns `RunNotStarted` when no active run.
  - Ensure the TUI calls `store.subscribe(1)` on the returned **shared** store instance (so broadcast live updates work).
  - Add a minimal integration test proving an external subscriber receives live events without reading the file.
    - **Decision locked**: add a new test in `crates/harness-core/tests/coord.rs` named `coord_event_store_subscribe_emits_live_events`.

  **Must NOT do**:
  - Do not implement file-tail polling as the primary live mechanism.
  - Do not introduce multi-writer event stores.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: concurrency + API design + correctness
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 4-8 | Blocked By: none

  **References**:
  - `spawn_coordinator` current API: `crates/harness-core/src/coord.rs:398-412`
  - `RunState.event_store` is currently private + non-shareable: `crates/harness-core/src/coord.rs:1488-1491`
  - `EventStore::subscribe` requires same broadcast sender: `crates/harness-core/src/store.rs:303-317`

  **Acceptance Criteria**:
  - [ ] New public API exists to obtain a shared event stream/store from `CoordinatorHandle` after `start_run`.
  - [ ] A test subscribes and observes at least one live event appended after subscription.
  - [ ] `cargo test -p harness-core coord` (or targeted test) passes.

  **QA Scenarios**:
  ```
  Scenario: External subscriber receives live events
    Tool: Bash
    Steps:
      - cargo test -p harness-core coord -- --nocapture |& tee .sisyphus/evidence/task-2-coord-subscribe.txt
    Expected: new test asserts subscribe receives events appended after start_run
    Evidence: .sisyphus/evidence/task-2-coord-subscribe.txt
  ```

  **Commit**: YES | Message: `feat(core): expose shared event store for live subscribers` | Files: `crates/harness-core/src/coord.rs`, `crates/harness-core/tests/coord.rs`

- [x] 3. Populate `EditApplied.diff_rel_path/diff_digest` from hashline tool artifacts

  **What to do**:
  - In coordinator tool-completion flow, when a hashline edit succeeds:
    - locate the diff artifact produced by `edit.hashline_apply` (by artifact path suffix `.diff` or by structured_json keys).
    - pass `diff_rel_path` + `diff_digest` into the `EditApplied` event (fields already exist in schema).
  - Add an agent-executable Rust test in `crates/harness/src/run.rs` (tests module) named:
    - `edit_applied_diff_refs_match_artifact_written`
    - It must run the deterministic golden path (`run_once`) and assert:
      - `EditApplied.diff_rel_path` and `EditApplied.diff_digest` are `Some(...)`
      - `ArtifactWritten.digest` for the same path equals `EditApplied.diff_digest`

  **Must NOT do**:
  - Do not embed full diff text into JSONL events.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: coordinator/tool integration + tests
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 8-9 | Blocked By: none

  **References**:
  - Hashline tool writes diff artifacts: `crates/harness-tools/src/hashline_apply.rs:46-64`
  - `EditAppliedEvent` already has optional diff fields: `crates/harness-core/src/event.rs:266-274`
  - Coordinator currently hardcodes `None`: `crates/harness-core/src/coord.rs:2141-2149`
  - Tool completion hook: `crates/harness-core/src/coord.rs:1200-1235`

  **Acceptance Criteria**:
  - [ ] In a deterministic golden_path run, `EditApplied` JSON includes non-null `diff_rel_path` + `diff_digest`.
  - [ ] Digest matches `artifact_written` for the same path.

  **QA Scenarios**:
  ```
  Scenario: Deterministic run asserts diff refs match artifact_written
    Tool: Bash
    Steps:
      - HARNESS_DETERMINISTIC=1 cargo test -p harness edit_applied_diff_refs_match_artifact_written -- --nocapture |& tee .sisyphus/evidence/task-3-diff-refs-test.txt
    Expected: test passes and prints no panics
    Evidence: .sisyphus/evidence/task-3-diff-refs-test.txt
  ```

  **Commit**: YES | Message: `feat(core): include diff refs in edit_applied events` | Files: `crates/harness-core/src/coord.rs`, `crates/harness/src/run.rs`

- [x] 4. Implement `harness tui` CLI wiring for live + replay modes (fix clap + add `--deterministic`)

  **What to do**:
  - Fix clap args so replay mode is reachable and does not require `--scenario`.
  - Implement `harness tui` execution paths:
    - **Replay**: `--replay <run_dir>` loads JSONL and launches the TUI in replay mode.
    - **Live**: `--scenario <name>` launches coordinator + runs the scenario and launches TUI live mode.
  - Add `--deterministic` to `harness tui` (mirrors `harness run`).
  - Ensure live mode has **no stdin prompts**; permissions are resolved only via TUI (Task 6).
  - **Decision locked (runtime orchestration)**:
    - Use a Tokio **multi-thread** runtime in `crates/harness/src/tui.rs` so the synchronous Crossterm UI loop can run while async tasks continue.
    - Live mode must start these components:
      1) Coordinator (already spawned via `spawn_coordinator`)
      2) Scenario runner task (starts run, spawns agents, requests tool call)
      3) Event forwarder task (subscribes to shared store, sends events to TUI via bounded channel)
      4) UI intent handler task (receives `ResolvePermission` intents from TUI and calls `CoordinatorHandle::resolve_permission`)
  - **Decision locked (scenario behavior)**:
    - `--scenario golden_path_interactive`: do **not** auto-resolve permissions; the run must block until the TUI approves/denies.
    - `--scenario golden_path`: auto-resolve `PermissionRequested` to `Allow` inside the scenario runner (so the run completes without UI intervention).

  **Must NOT do**:
  - Do not leave `// TODO: Implement ...` stubs in the production command path.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: multi-module wiring + async/runtime orchestration
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 5-9 | Blocked By: 2

  **References**:
  - Current stubbed TUI command: `crates/harness/src/tui.rs:20-27`
  - `harness run` is the wiring reference pattern: `crates/harness/src/run.rs:52-102`.
  - Scenario helpers: `crates/harness/src/scenarios.rs:39-95`.

  **Acceptance Criteria**:
  - [ ] `cargo run -p harness -- tui --help` shows `--replay`, `--scenario`, and `--deterministic`.
  - [ ] `cargo run -p harness -- tui --replay <run_dir>` is a valid invocation path (no clap errors). (Interactive quit is verified via PTY E2E in Task 9.)

  **QA Scenarios**:
  ```
  Scenario: tui subcommand exposes required flags
    Tool: Bash
    Steps:
      - cargo run -p harness -- tui --help |& tee .sisyphus/evidence/task-4-tui-help.txt
    Expected: help output includes --replay, --scenario, --deterministic, --session-dir, --exit-on-finish
    Evidence: .sisyphus/evidence/task-4-tui-help.txt
  ```

  **Commit**: YES | Message: `feat(cli): wire tui live+replay modes` | Files: `crates/harness/src/tui.rs`

- [x] 5. TUI replay mode: load session JSONL, header (run id), and `r` reload

  **What to do**:
  - Implement replay-mode data loading in `harness-tui`:
    - load `<run_dir>/events.jsonl` into memory
    - set `replay_mode=true` and show header with run_dir + run_id
    - implement `r` key to reload from disk (recompute projections)
  - **Decision locked**: implement a two-pane (list + details) layout in replay mode and use `Tab` to switch focus:
    - Focus=List: `j/k` navigates events
    - Focus=Details: `j/k` scrolls details text (diff/output/event JSON)
  - Add snapshot tests.
    - **Decision locked**: generate replay fixtures inside tests using `tempfile`:
      - create a temp `run_dir/`
      - write `events.jsonl` by serializing a small Vec of `EventEnvelopeV1` (no external fixture files)
      - set `app.replay_mode=true` + `app.session_path=Some(run_dir)` and render

  **Must NOT do**:
  - Replay mode must not execute tools/providers.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: UI behavior + snapshot stabilization
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8 | Blocked By: 4

  **References**:
  - Existing replay header placeholder: `crates/harness-tui/src/ui.rs:13-18`
  - Existing help text shows `r` but key handler is missing: `crates/harness-tui/src/ui.rs:168-170` and `crates/harness-tui/src/app.rs:52-71`.

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no cargo test -p harness-tui` passes with new replay snapshots.
  - [ ] Pressing `r` in replay mode reloads events deterministically (unit test).

  **QA Scenarios**:
  ```
  Scenario: Replay mode snapshots are stable
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui replay_mode |& tee .sisyphus/evidence/task-5-tui-replay.txt
    Expected: exit 0, no snapshot updates required
    Evidence: .sisyphus/evidence/task-5-tui-replay.txt
  ```

  **Commit**: YES | Message: `feat(tui): replay mode loads jsonl + reload key` | Files: `crates/harness-tui/src/**`

- [x] 6. TUI live mode: subscribe to shared event stream, update projections, and render grouped output

  **What to do**:
  - Implement live-mode event ingestion:
    - subscribe to coordinator’s shared event store (Task 2) using `EventStore::subscribe(1)`
    - forward events into the UI loop (bounded channel)
    - **Decision locked**: handle `SubscriberLagged` by (a) setting a visible status banner, and (b) replaying from `last_seq_seen+1` via `EventStore::replay` and then continuing the live stream while deduping by `seq`.
  - Update UI model to maintain:
    - event list
    - correlation groups (`correlation_id`) and provider output assembled from `ProviderStreamDelta`
    - **Decision locked**: when `follow_mode=true`, set `selected_event_index = events.len()-1` whenever new events arrive.
  - Replace Output tab placeholder with grouped display:
    - left pane: correlation groups (tool calls vs provider requests)
    - right pane: assembled provider text (for selected provider group) or tool call summary (for tool group)
    - **Decision locked**: “active group” is determined by the selected event’s `correlation_id` if present; otherwise use the most recent provider-request group.
  - **Decision locked**: implement a two-pane layout in live mode and use `Tab` to switch focus:
    - Focus=List: `j/k` navigates events
    - Focus=Details: `j/k` scrolls the right-pane text (Output/Diff)
  - **Decision locked**: Events tab becomes two-pane:
    - left pane: event timeline list (as today)
    - right pane: selected event details rendered as stable pretty JSON (`serde_json::to_string_pretty`) in a scrollable paragraph
  - **Decision locked**: preserve the Events tab invariant that each row includes the exact `EventV1` variant name (already implemented via `event_variant_name` in `crates/harness-tui/src/ui.rs:78-105`).
  - Add live-mode snapshot tests using a synthetic event stream.
  - Implement `--exit-on-finish` behavior (automation mode):
    - **Decision locked**: when `exit_on_finish=true`, auto-exit with code 0 once a `RunFinished` or `RunFailed` event has been observed **and** there is no active permission modal.

  **Must NOT do**:
  - Do not block UI on event reads; skip intermediate renders if behind.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: live UX + grouping + snapshots
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8 | Blocked By: 2,4

  **References**:
  - Provider events correlation id is request_id: `crates/harness-core/src/coord.rs:1342-1421`.
  - Tool calls correlation id is tool_call_id: `crates/harness-core/src/coord.rs:2000-2093`.
  - Current placeholders: `crates/harness-tui/src/ui.rs:145-155`.

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no cargo test -p harness-tui live_mode` passes.
  - [ ] Output tab shows assembled provider deltas in snapshot.
  - [ ] A snapshot test asserts grouped tool calls render as a single group label.

  **QA Scenarios**:
  ```
  Scenario: Live mode renders grouped streams
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui live_mode |& tee .sisyphus/evidence/task-6-tui-live.txt
    Expected: exit 0 and snapshots include grouping + output text
    Evidence: .sisyphus/evidence/task-6-tui-live.txt
  ```

  **Commit**: YES | Message: `feat(tui): live mode subscribe + grouped output` | Files: `crates/harness-tui/src/**`, `crates/harness/src/tui.rs`

- [x] 7. Permission modal UI: render PermissionRequested + approve/deny with `a`/`d`

  **What to do**:
  - When an unresolved `PermissionRequested` exists:
    - render a modal dialog showing the redacted summary (`PermissionRequestedEvent.summary`)
    - key binds: `a` allow, `d` deny, `Esc` dismiss (no resolve)
  - Live mode only: on `a/d`, invoke coordinator `ResolvePermission` (via channel to async handler).
  - **Decision locked**: modal key handling is global (works regardless of focus/tab) while modal is visible.
  - Add tests:
    - snapshot: modal rendering
    - unit: keypress emits resolve intent and closes modal once `PermissionResolved` observed

  **Must NOT do**:
  - Do not auto-approve any permission; must be explicit.
  - Do not require stdin for approvals.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8 | Blocked By: 6

  **References**:
  - Permission event fields: `crates/harness-core/src/event.rs:231-240`.
  - Coordinator resolve API exists: `crates/harness-core/src/coord.rs` (handle method `resolve_permission`, already used by `harness run`).
  - Current key handling location: `crates/harness-tui/src/app.rs:52-71`.

  **Acceptance Criteria**:
  - [ ] A deterministic interactive run can be approved using `a` in TUI and proceeds to completion.

  **QA Scenarios**:
  ```
  Scenario: Permission modal appears and approves
    Tool: Bash
    Steps:
      - cargo test -p harness-tui permission_modal |& tee .sisyphus/evidence/task-7-permission-ui.txt
    Expected: tests assert modal shown and resolve intent issued
    Evidence: .sisyphus/evidence/task-7-permission-ui.txt
  ```

  **Commit**: YES | Message: `feat(tui): permission modal + approvals` | Files: `crates/harness-tui/src/**`, `crates/harness/src/tui.rs`

- [x] 8. Diff viewer tab: load diff artifact for `EditApplied` and render scrollable diff

  **What to do**:
  - In Diff tab:
    - **Decision locked (PTY script compatibility)**: render the diff for the **most recent** `EditApplied` event at-or-before the current selection; if none exist before selection, use the most recent `EditApplied` in the entire event list.
    - load the diff file from `run_dir.join(diff_rel_path)`
    - render unified diff text in a scrollable view
    - handle missing file gracefully (“diff artifact missing”) with no panics
  - Add snapshot tests for:
    - known diff fixture rendering
    - missing artifact handling
    - **Decision locked**: generate diff fixtures inside tests using `tempfile`:
      - create `run_dir/artifacts/edit-edit-golden-path.diff` containing a small unified diff
      - write `events.jsonl` containing an `EditApplied` with matching `diff_rel_path` and `diff_digest`

  **Must NOT do**:
  - Do not embed full diffs into JSONL events.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 9-12 | Blocked By: 3,5,6

  **References**:
  - `EditApplied` diff fields: `crates/harness-core/src/event.rs:266-274`
  - Hashline tool diff artifact name: `crates/harness-tools/src/hashline_apply.rs:46-64`

  **Acceptance Criteria**:
  - [ ] Selecting an EditApplied event shows a diff in Diff tab (live + replay).
  - [ ] Missing artifact does not panic.

  **QA Scenarios**:
  ```
  Scenario: Diff tab renders diff fixture
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui diff_tab |& tee .sisyphus/evidence/task-8-diff-ui.txt
    Expected: snapshots show expected diff content
    Evidence: .sisyphus/evidence/task-8-diff-ui.txt
  ```

  **Commit**: YES | Message: `feat(tui): diff viewer loads edit artifacts` | Files: `crates/harness-tui/src/**`

- [x] 9. PTY E2E harness (portable-pty + vt100) for fullscreen TUI + golden snapshots

  **What to do**:
  - Implement Linux-only PTY E2E tests under `crates/harness-testkit/tests/pty_e2e.rs`:
    - **Decision locked**: resolve the `harness` binary path as follows:
      1) If `$HARNESS_BIN` is set, use that.
      2) Else, build once via `cargo build -p harness`, then use `<repo>/target/debug/harness`.
      - Use `env!("CARGO_MANIFEST_DIR")` to compute `<repo>` from `crates/harness-testkit/`.
    - spawn `harness tui --scenario golden_path_interactive --deterministic --session-dir <tempdir>` under PTY (80x24)
      - set env: `HARNESS_DETERMINISTIC=1`, `HARNESS_DISABLE_ANIMATIONS=1`, `TERM=xterm-256color`, `LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, `TZ=UTC`
    - read PTY output on a dedicated thread; feed to `vt100::Parser`
    - scripted keystrokes:
      1) wait until screen contains `PermissionRequested`
      2) send `a`
      3) wait until screen contains `RunFinished`
      4) send `3` and snapshot diff tab
      5) send `q` and assert exit 0
  - Snapshot `parser.screen().contents()` at checkpoints using `insta`.
  - Add flake check test or script-friendly loop command.

  **Must NOT do**:
  - No sleeps as synchronization primitive; poll screen content with timeouts.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: PTY determinism + parsing + flaky avoidance
  - Skills: []

  **Parallelization**: Can Parallel: NO | Wave 3 | Blocks: 10-12 | Blocked By: 4-8

  **References**:
  - PTY plan requirements in original plan (Task 24): `.sisyphus/plans/rust-agent-harness-foundation.md:1440-1498`

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e` passes on Linux.
  - [ ] Repeat loop (5×) passes.

  **QA Scenarios**:
  ```
  Scenario: PTY E2E golden path
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --nocapture |& tee .sisyphus/evidence/task-9-pty-e2e.txt
    Expected: tests pass; snapshots match
    Evidence: .sisyphus/evidence/task-9-pty-e2e.txt

  Scenario: Repeat run flake check
    Tool: Bash
    Steps:
      - for i in 1 2 3 4 5; do INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e || exit 1; done |& tee .sisyphus/evidence/task-9-pty-repeat.txt
    Expected: all 5 passes
    Evidence: .sisyphus/evidence/task-9-pty-repeat.txt
  ```

  **Commit**: YES | Message: `test(e2e): pty tui harness + vt100 snapshots` | Files: `crates/harness-testkit/**`

- [x] 10. Extend GitLab CI: deterministic env + PTY E2E job (Linux) while keeping SAST/Secret Detection

  **What to do**:
  - Update `.gitlab-ci.yml`:
    - keep SAST + Secret Detection includes/jobs
    - add cargo cache (registry + git)
    - set deterministic vars for Rust test jobs:
      - `INSTA_UPDATE=no`
      - `RUST_TEST_THREADS=1`
      - `TZ=UTC`, `LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, `TERM=xterm-256color`
      - `HARNESS_DETERMINISTIC=1`, `HARNESS_DISABLE_ANIMATIONS=1`
    - add `rust:pty_e2e` job that runs the 5× repeat loop
  - Add CI artifacts for failing snapshots/logs.

  **Must NOT do**:
  - Do not require non-Linux runners.

  **Recommended Agent Profile**:
  - Category: `quick`
  - Skills: [`git-master`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 11-12 | Blocked By: 9

  **References**:
  - Current CI file: `.gitlab-ci.yml:1-38`

  **Acceptance Criteria**:
  - [ ] CI contains jobs: `rust:fmt`, `rust:clippy`, `rust:test`, `rust:pty_e2e`, plus SAST + secret detection.

  **QA Scenarios**:
  ```
  Scenario: CI contains deterministic env and PTY job
    Tool: Bash
    Steps:
      - sed -n '1,260p' .gitlab-ci.yml |& tee .sisyphus/evidence/task-10-gitlab-ci.txt
    Expected: file shows PTY job + deterministic env vars
    Evidence: .sisyphus/evidence/task-10-gitlab-ci.txt
  ```

  **Commit**: YES | Message: `ci: add deterministic env + pty e2e job` | Files: `.gitlab-ci.yml`

- [x] 11. Full E2E test pass (application + UI) with captured evidence

  **What to do**:
  - Run the end-to-end commands locally and capture evidence:
    - headless run: `cargo run -p harness -- run --scenario golden_path --deterministic --out target/run.jsonl`
    - replay: `cargo run -p harness -- replay --session <run_dir>`
    - PTY UI E2E: `cargo test -p harness-testkit pty_e2e`

  **Recommended Agent Profile**:
  - Category: `deep`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 12 | Blocked By: 1-10

  **References**:
  - Headless run pattern: `crates/harness/src/run.rs:52-102`.
  - Replay CLI: `crates/harness/src/replay.rs`.

  **Acceptance Criteria**:
  - [ ] Evidence files exist:
    - `.sisyphus/evidence/task-11-headless-run.txt`
    - `.sisyphus/evidence/task-11-replay.txt`
    - `.sisyphus/evidence/task-11-pty-e2e.txt`

  **QA Scenarios**:
  ```
  Scenario: E2E app + UI
    Tool: Bash
    Steps:
      - RUN_DIR=$(HARNESS_DETERMINISTIC=1 cargo run -q -p harness -- run --scenario golden_path --deterministic --print-run-dir | tee .sisyphus/evidence/task-11-headless-run.txt)
      - cargo run -q -p harness -- replay --session "$RUN_DIR" |& tee .sisyphus/evidence/task-11-replay.txt
      - INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e |& tee .sisyphus/evidence/task-11-pty-e2e.txt
    Expected: all commands exit 0
    Evidence: .sisyphus/evidence/task-11-*.txt
  ```

  **Commit**: NO

- [x] 12. Close the loop: update foundation plan status + add compliance evidence links

  **What to do**:
  - In `.sisyphus/plans/rust-agent-harness-foundation.md`:
    - mark tasks 20, 22, 23, 24, 25 as `[x]` once implemented
    - correct any tasks that were previously marked `[x]` but were incomplete (notably TUI replay wiring) and ensure they are now truly complete
    - add a short “Evidence” note per task pointing to `.sisyphus/evidence/...` artifacts created by tasks 1, 3, 5-11

  **Must NOT do**:
  - Do not change the original task requirements; only update completion status and add evidence pointers.

  **Recommended Agent Profile**:
  - Category: `writing`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: Final Verification Wave | Blocked By: 11

  **References**:
  - Foundation plan file: `.sisyphus/plans/rust-agent-harness-foundation.md`.

  **Acceptance Criteria**:
  - [ ] Foundation plan accurately reflects completion and points to evidence files.

  **QA Scenarios**:
  ```
  Scenario: Evidence links exist
    Tool: Bash
    Steps:
      - rg "\.sisyphus/evidence/task-" -n .sisyphus/plans/rust-agent-harness-foundation.md |& tee .sisyphus/evidence/task-12-evidence-links.txt
    Expected: evidence references exist for completed tasks
    Evidence: .sisyphus/evidence/task-12-evidence-links.txt
  ```

  **Commit**: YES | Message: `docs(plan): close compliance loop with evidence` | Files: `.sisyphus/plans/rust-agent-harness-foundation.md`

## Final Verification Wave (4 parallel agents, ALL must APPROVE)
- [x] F1. Plan Compliance Audit — oracle
- [x] F2. Code Quality Review — unspecified-high
- [x] F3. Real Manual QA (Agent-Driven) — unspecified-high (+ PTY tests)
- [x] F4. Scope Fidelity Check — deep

## Commit Strategy
- Prefer atomic commits per TODO above.
- Keep snapshot updates tightly coupled to the feature that changes UI rendering.

## Success Criteria
- Repo satisfies tasks 1–26 of the foundation plan, with PTY E2E and CI coverage.
- E2E tests validate both the application (headless golden path + replay) and the UI (PTY automation).
