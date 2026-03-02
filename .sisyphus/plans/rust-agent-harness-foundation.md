# Rust Agent Harness — Foundation (Rust + Ratatui, Event-Sourced, Test-First)

## TL;DR
> **Summary**: Build a greenfield Rust-first multi-agent harness with an append-only event log, a single-authority Coordinator for scheduling/permissions, hash-anchored (“hashline”) edits, a modern Ratatui TUI, and CI-grade headless + PTY E2E tests.
> **Deliverables**:
> - Cargo workspace + crates for core/runtime, providers, tools, TUI, testkit, and a single `harness` CLI binary
> - Event schema v1 + JSONL session store + replay
> - Coordinator (single scheduling authority) with background concurrency limits + stale/cancel semantics
> - Permission engine (allow/deny/ask) with interactive TUI prompts + deterministic headless resolution
> - Hashline edit engine + filesystem tool (atomic apply, reject on mismatch) + diff viewer
> - Provider abstraction: deterministic MockProvider (offline) + OpenAI-compatible proxy provider (optional live)
> - Comprehensive tests: unit + ratatui integration snapshots + headless E2E + PTY E2E (portable-pty + vt100)
> **Effort**: XL
> **Parallel**: YES — 5 waves
> **Critical Path**: Event schema → Event store → Coordinator → Permissions → Hashline edits → Headless scenario runner → TUI → PTY E2E → CI

## Context
### Original Request
- Rust-first multi-agent orchestration harness inspired by Oh My OpenCode (OMO) + Oh My Pi.
- Must be event-driven and replayable (persist sessions as JSONL).
- Must support background/parallel agents/tasks with concurrency + stale/cancel semantics.
- Must have tool permissions (allow/deny/ask) and an anti-footgun (“delegated executors cannot re-delegate”).
- Must have a modern terminal UI (OpenCode-like ergonomics) **without OpenTUI**.
- **Hashline edits are Day-1** and must be heavily tested.
- Provider constraint (MVP): OpenAI-compatible HTTP proxy only (CLIProxy-style baseURL/apiKey); offline deterministic tests required; optional gated live proxy tests.
- Tests must be first-class and fully agent-runnable (no human input), including PTY E2E for the fullscreen TUI.

### Interview Summary (decisions locked)
- TUI stack: **Ratatui + Crossterm**.
- Platform target for MVP + CI PTY E2E: **Linux-first (Linux-only acceptable)**.
- Config: **harness config only** (no auto-import of OpenCode config); ship **example config** with explicit guidance + generate JSON Schema.

### Metis Review (gaps addressed)
- Locked replay/cancel semantics (late results policy) and deterministic log requirements for CI.
- Added explicit TUI event-loop guardrails (don’t mix Crossterm APIs; key-press only; resize coalescing).
- Added explicit redaction-before-persist policy (no secrets in JSONL/snapshots).
- Avoided cancellation-unsafe semaphore acquisition patterns by using coordinator-managed concurrency slots.
- Added deterministic “run twice → identical JSONL digest” acceptance criteria for golden path.

## Work Objectives
### Core Objective
Deliver an orchestration-ready, event-sourced harness foundation that enables rapid iteration toward OpenCode-level UX **without architectural rework**.

### Deliverables
1. **Core runtime**: Coordinator + event schema/store + scheduling + cancellation/stale policy.
2. **Safety**: permissions (allow/deny/ask), capability-gated tools, “workers cannot re-delegate”.
3. **Editing**: hashline engine + atomic apply tool + diff rendering.
4. **Providers**: deterministic MockProvider (offline) + optional OpenAI-compatible proxy provider.
5. **Interfaces**: headless scenario runner + modern Ratatui TUI (live + replay).
6. **Testing pyramid**: unit + integration + headless E2E + PTY E2E; all CI-runnable.

### Definition of Done (agent-verifiable)
- [ ] `cargo fmt --check` passes (workspace).
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test --workspace --all-features` passes offline.
- [ ] Headless golden-path run is deterministic:
  - produces identical JSONL digest across two runs when `HARNESS_DETERMINISTIC=1`.
- [ ] PTY E2E TUI test passes on Linux CI (portable-pty + vt100 snapshots).
- [ ] `harness tui --scenario golden_path_interactive --deterministic` runs and shows: PermissionRequested → PermissionResolved → EditApplied → RunFinished.
- [ ] `harness replay --session <path>` replays JSONL into the same final projected state as the live run.

### Must Have
- Append-only event log (JSONL) with schema versioning and monotonic `seq`.
- Coordinator is the only scheduling authority; tasks are cancellable and stale-detectable.
- Permission requests are events; ask/allow/deny works in TUI and headless.
- Hashline edits reject on mismatch and never partially apply.
- Deterministic tests: fake clock, fixed PTY size, snapshot normalization.

### Must NOT Have (guardrails)
- NO OpenTUI dependency.
- NO direct vendor SDK/provider support (beyond OpenAI-compatible HTTP proxy).
- NO copying OMO prompt text/code (license hygiene); treat as behavioral reference only.
- NO unbounded queues for event streaming or provider deltas.
- NO secrets persisted in JSONL logs or snapshots (redact-before-persist).

## Verification Strategy
> ZERO HUMAN INTERVENTION — all verification is agent-executed.
- Test decision: **TDD for core invariants** (event/store/scheduler/permissions/hashline), tests-after for pure rendering polish.
- UI testing:
  - unit/widget tests: direct `ratatui::buffer::Buffer` assertions
  - integration snapshots: `Terminal<TestBackend>` + `insta`
- E2E testing:
  - headless scenario runner emits JSONL + stdout
  - PTY E2E spawns fullscreen TUI under PTY; inject keystrokes; parse with `vt100`; assert snapshots
- Evidence: `.sisyphus/evidence/task-{N}-{slug}.{ext}`

## Execution Strategy
### Parallel Execution Waves
Wave 1 (Scaffold + invariants): workspace/CI + config + event schema/store + clock + redaction
Wave 2 (Core runtime): Coordinator + scheduler slots + permissions + tools framework + hashline engine/tool
Wave 3 (Providers + headless): MockProvider + proxy provider + agent minimal runtime + scenario runner + session layout/replay
Wave 4 (TUI): Ratatui app (live + replay) + permission UI + grouped streams + diff viewer
Wave 5 (E2E hardening): PTY harness + flake reduction + CI integration + docs/example config polish

### Dependency Matrix (full, all tasks)

| Task | Wave | Depends On |
|------|------|------------|
| 1 | 1 | — |
| 2 | 1 | 1 |
| 3 | 1 | 1 |
| 4 | 1 | 1 |
| 5 | 1 | 1, 3, 4 |
| 6 | 1 | 1, 5 |
| 7 | 2 | 2, 3, 4, 5, 6 |
| 8 | 2 | 7 |
| 9 | 2 | 7 |
| 10 | 2 | 7, 9 |
| 11 | 2 | 1, 4 |
| 12 | 2 | 7, 9, 10, 11 |
| 13 | 3 | 1, 5 |
| 14 | 3 | 2, 4, 13 |
| 15 | 3 | 7, 8, 13 |
| 16 | 3 | 2, 7, 8, 9, 10, 11, 12, 15 |
| 17 | 3 | 6, 16 |
| 18 | 3 | 5 |
| 19 | 4 | 18 |
| 20 | 4 | 6, 7, 19 |
| 21 | 4 | 17, 19 |
| 22 | 4 | 9, 20 |
| 23 | 4 | 12, 17, 19 |
| 24 | 5 | 20, 22, 23 |
| 25 | 5 | 24 |
| 26 | 5 | 2, 5 |
| 27 (Optional) | 5 | 14, 16 |

### Agent Dispatch Summary (wave → task count → categories)

- Wave 1 (6 tasks): 2×`deep`, 2×`quick`, 2×`ultrabrain`
- Wave 2 (6 tasks): 2×`deep`, 4×`ultrabrain`
- Wave 3 (6 tasks): 4×`deep`, 1×`ultrabrain`, 1×`quick`
- Wave 4 (5 tasks): 5×`visual-engineering`
- Wave 5 (3 required + 1 optional): 1×`ultrabrain`, 1×`quick`, 1×`writing` (+ optional 1×`deep`)

## Decision-Complete Checklist (frozen in this plan)
1. **Replay contract**: replay is **side-effect free**; it only rebuilds projections from JSONL. CI determinism is achieved by `HARNESS_DETERMINISTIC=1` (fake clock + stable serialization).
2. **Cancel/late results**: cancellation is best-effort; if a background task reports after cancel, Coordinator records it as `TaskResultLate` and projections ignore side effects.
3. **Concurrency gates**: coordinator-managed slot counters + queues (no semaphore acquire-in-select patterns).
4. **Hashline canonicalization**: split on `\n`, strip trailing `\r` per line before hashing; keep all other whitespace; hash = `blake3(line_bytes)` truncated to 12 hex.
5. **Hashline mismatch policy**: hard reject, atomic batch (no partial apply), structured mismatch payload.
6. **Headless permission behavior**: default-deny on `ask` unless scenario script provides a deterministic approval/deny step.
7. **TUI determinism**: single-threaded `poll → read` loop; only key-press events; coalesce resize; fixed PTY size in tests.
8. **Redaction-before-persist**: redact API keys and known secret patterns in any persisted text fields; store hashes/digests where possible.

## TODOs
> Implementation + Test = ONE task. Never separate.
> EVERY task MUST have: Agent Profile + Parallelization + QA Scenarios.

### Wave 1 — Scaffold + invariants (Linux-first)

- [x] 1. Initialize Rust workspace + crate layout + Rust CI (GitLab)

  **What to do**:
  - Keep repo root as workspace.
  - Create `Cargo.toml` (virtual workspace) with members:
    - `crates/harness` (bin)
    - `crates/harness-core` (lib)
    - `crates/harness-providers` (lib)
    - `crates/harness-tools` (lib)
    - `crates/harness-tui` (lib)
    - `crates/harness-testkit` (lib; test helpers)
  - Add `rust-toolchain.toml`:
    - channel: `stable`
    - components: `rustfmt`, `clippy`
  - Create minimal compilation-only skeletons:
    - `crates/harness/src/main.rs` with `clap` subcommands: `tui`, `run`, `replay`, `schema`, `config validate` (stubs are fine; compile must pass)
    - each lib crate exports `pub fn _placeholder()` until real APIs land
  - Add `LICENSE` (MIT by default) + `.gitignore` (target/, sessions/, artifacts/).
  - Update `.gitlab-ci.yml` to add Rust jobs (keep existing SAST/secret detection):
    - `fmt`: `cargo fmt --check`
    - `clippy`: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
    - `test`: `cargo test --workspace --all-features`
    - (PTY E2E job added later once tests exist)

  **Must NOT do**:
  - Do not add OpenTUI or any UI dependency other than Ratatui+Crossterm.
  - Do not add real provider/network integration yet.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: multi-file scaffolding + CI wiring
  - Skills: [`git-master`] — Reason: clean initial commits

  **Parallelization**: Can Parallel: NO | Wave 1 | Blocks: 2-27 | Blocked By: none

  **References**:
  - Existing CI file: `.gitlab-ci.yml:1-19` — extend with Rust jobs

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features` passes
  - [ ] `cargo fmt --check` passes
  - [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
  - [ ] `cargo run -p harness -- --help` prints help and exits 0

  **QA Scenarios**:
  ```
  Scenario: Workspace compiles and CI commands pass
    Tool: Bash
    Steps:
      - cargo test --workspace --all-features
      - cargo fmt --check
      - cargo clippy --workspace --all-targets --all-features -- -D warnings
      - cargo run -p harness -- --help
    Expected: all commands exit 0
    Evidence: .sisyphus/evidence/task-1-workspace-ci.txt

  Scenario: CI config still includes SAST/secret detection
    Tool: Bash
    Steps:
      - sed -n '1,120p' .gitlab-ci.yml
    Expected: SAST + secret_detection remain present AND Rust fmt/clippy/test jobs exist
    Evidence: .sisyphus/evidence/task-1-gitlab-ci.txt
  ```

  **Commit**: YES | Message: `chore(workspace): init rust workspace and ci` | Files: `Cargo.toml`, `crates/**`, `.gitlab-ci.yml`, `rust-toolchain.toml`, `.gitignore`, `LICENSE`

- [x] 2. Implement harness config system (JSON5/JSONC-like) + example config + JSON Schema

  **What to do**:
  - In `crates/harness-core`, create `config` module with:
    - `HarnessConfig` root struct (serde + schemars)
    - `background_task` settings: `defaultConcurrency`, `providerConcurrency`, `modelConcurrency`, `staleTimeoutMs`, `messageStalenessTimeoutMs`
    - `providers` map supporting **only** `openai_compatible` provider type for now:
      - `base_url`, `api_key` (string or `${ENV_VAR}`), `timeout_ms`, optional `headers` (use `BTreeMap<String,String>` for stable serialization)
      - `models` map (model_id → display name + limits + optional variants)
    - `categories` map (category_name → { description, model_ref, variant, temperature, permissions, tools })
    - `permissions`:
      - global defaults per tool capability: `edit`, `shell`, `network`, with allow/deny/ask
      - `shell_allowlist`: allowed executables + allowed cwd roots
    - `paths.session_dir` default: `.agent-harness/sessions`
    - `deterministic` block for CI/tests: `{ enabled: bool, seed: u64 }`
  - Parse config using `json5` crate (comments + trailing commas).
  - Define config resolution order in `crates/harness/src/main.rs`:
    1) `--config <path>`
    2) `./harness.jsonc`
    3) `$XDG_CONFIG_HOME/harness/config.jsonc` (fallback `~/.config/harness/config.jsonc`)
  - Add CLI overrides (apply after config load):
    - `--session-dir <path>` overrides `paths.session_dir`
  - Add `configs/harness.example.jsonc` with annotated blocks (CLIProxy-style defaults):
    - include a commented example using `base_url: "http://127.0.0.1:8317/v1"`
  - Add `harness schema` subcommand that prints JSON Schema for `HarnessConfig`.
  - Add `harness config validate` subcommand that loads config and prints validation errors.
  - Add unit tests:
    - example config parses successfully
    - missing required fields produce deterministic error messages
    - `${ENV_VAR}` substitution works

  **Must NOT do**:
  - Do not read/import `~/.config/opencode/opencode.json` automatically.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: schema + parsing + CLI wiring + tests
  - Skills: [`rust-async-patterns`] — Reason: minimal, but helpful for tokio usage later

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 7-27 | Blocked By: 1

  **References**:
  - Example inspiration (local, do not auto-import): `~/.config/opencode/opencode.json` and `~/.config/opencode/oh-my-opencode.json`
  - OMO schema concepts (categories/permissions/concurrency): `~/.config/opencode/node_modules/oh-my-opencode/dist/oh-my-opencode.schema.json` (behavioral reference only)

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core` passes
  - [ ] `cargo run -p harness -- schema` prints valid JSON Schema
  - [ ] `cargo run -p harness -- config validate --config configs/harness.example.jsonc` exits 0

  **QA Scenarios**:
  ```
  Scenario: Example config validates
    Tool: Bash
    Steps:
      - cargo run -p harness -- config validate --config configs/harness.example.jsonc
    Expected: exit 0
    Evidence: .sisyphus/evidence/task-2-config-validate.txt

  Scenario: Missing config errors are deterministic
    Tool: Bash
    Steps:
      - printf '{"version":1}' > /tmp/harness-bad.jsonc
      - cargo run -p harness -- config validate --config /tmp/harness-bad.jsonc || true
    Expected: non-zero exit AND stable error message mentions missing required blocks
    Evidence: .sisyphus/evidence/task-2-config-invalid.txt
  ```

  **Commit**: YES | Message: `feat(config): jsonc config + schema + example` | Files: `crates/harness-core/src/config/**`, `crates/harness/src/**`, `configs/harness.example.jsonc`

- [x] 3. Add deterministic clock abstraction (real + fake) for replayable, testable time

  **What to do**:
  - In `crates/harness-core/src/clock.rs` define:
    - `trait Clock { fn mono_ms(&self) -> u64; fn system_time_rfc3339(&self) -> Option<String>; }`
    - `RealClock` (mono from `Instant`, system from `SystemTime::now()`)
    - `FakeClock` (starts at 0ms; manual `advance(ms)`)
  - Add a `Determinism` helper that selects `FakeClock` when `HARNESS_DETERMINISTIC=1` or config says so.
  - Unit tests:
    - `FakeClock` starts at 0, advances deterministically
    - deterministic mode produces `system_time_rfc3339 == None` (or constant) for stable JSONL

  **Must NOT do**:
  - Do not use wall-clock timestamps as ordering keys; ordering must be `seq`.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: small core primitive + tests
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 5-27 | Blocked By: 1

  **References**:
  - Tokio deterministic time tooling (for later async tests): https://tokio.rs/tokio/topics/testing

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core clock` (or full crate tests) passes

  **QA Scenarios**:
  ```
  Scenario: FakeClock is deterministic
    Tool: Bash
    Steps:
      - cargo test -p harness-core clock
    Expected: tests pass and assert exact ms values
    Evidence: .sisyphus/evidence/task-3-clock-tests.txt

  Scenario: Deterministic mode removes wall clock
    Tool: Bash
    Steps:
      - HARNESS_DETERMINISTIC=1 cargo test -p harness-core clock
    Expected: tests pass and `system_time_rfc3339` is None/constant
    Evidence: .sisyphus/evidence/task-3-clock-deterministic.txt
  ```

  **Commit**: YES | Message: `feat(core): deterministic clock abstraction` | Files: `crates/harness-core/src/clock.rs`

- [x] 4. Implement redact-before-persist engine (secrets never written to JSONL/snapshots)

  **What to do**:
  - In `crates/harness-core/src/redact.rs` implement:
    - `Redactor` trait: `fn redact_text(&self, s: &str) -> String`
    - `DefaultRedactor` with regex patterns:
      - `sk-[A-Za-z0-9]{10,}` → `[REDACTED_API_KEY]`
      - `Bearer\s+[A-Za-z0-9._\-]+` → `Bearer [REDACTED]`
    - helpers to redact structured values used in events/tool summaries
  - Add tests:
    - known patterns are removed
    - non-secret text unchanged
  - Add a `SecretScan` test helper that greps produced JSONL/snapshots for `sk-` and fails.

  **Must NOT do**:
  - Never persist raw API keys, Authorization headers, or environment dumps.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: isolated utility + tests
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 5-27 | Blocked By: 1

  **References**:
  - insta redactions/filters (for snapshot tests): https://insta.rs/docs/redactions/ and https://insta.rs/docs/filters/

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core redact` (or full crate tests) passes

  **QA Scenarios**:
  ```
  Scenario: Redactor strips API keys
    Tool: Bash
    Steps:
      - cargo test -p harness-core redact
    Expected: tests assert redaction output
    Evidence: .sisyphus/evidence/task-4-redact-tests.txt

  Scenario: SecretScan helper fails on injected secret
    Tool: Bash
    Steps:
      - (within test) write a temp file containing "sk-abc123..." and assert SecretScan detects it
    Expected: detection is reliable and deterministic
    Evidence: .sisyphus/evidence/task-4-secretscan.txt
  ```

  **Commit**: YES | Message: `feat(core): redact-before-persist utilities` | Files: `crates/harness-core/src/redact.rs`

### Wave 1 — continued

- [x] 5. Define event schema v1 (envelope + core event vocabulary) + stable serialization snapshots

  **What to do**:
  - In `crates/harness-core/src/event/` define:
    - `const SCHEMA_VERSION: u16 = 1`.
    - `EventEnvelopeV1` (serde) with fields:
      - `schema_version`, `event_id`, `seq`, `run_id`
      - `mono_ms` (from `Clock`), `ts` (optional RFC3339 string)
      - `actor` (`ActorKind` + optional `agent_id`)
      - `correlation_id`, `causation_id` (optional), `stream_key` (optional)
      - `payload: EventV1`
    - `ActorKind`: `Supervisor | Worker | User | System`
    - `EventV1` enum (minimal but extensible) with variants:
      - `RunStarted`, `RunFinished`, `RunFailed`
      - `AgentSpawned`, `AgentStopped`
      - `TaskScheduled`, `TaskCancelled`, `TaskCompleted`, `TaskResultLate`
      - `StaleDetected`
      - `ProviderRequestStarted`, `ProviderStreamDelta`, `ProviderRequestFinished`
      - `ToolCallRequested`, `ToolCallStarted`, `ToolCallFinished`
      - `PermissionRequested`, `PermissionResolved`
      - `EditProposed`, `EditApplied`, `EditRejected`
      - `ArtifactWritten`
      - `PolicyViolationDetected`
      - `UiIntentReceived` (optional; used only for PTY debugging)
  - Add `EventBuilder` helpers that:
    - stamp time from `Clock`
    - apply `Redactor` to any persisted strings
  - Add `insta` snapshots for:
    - a representative `RunStarted` envelope (deterministic mode)
    - a representative `PermissionRequested` envelope
  - Enforce stable JSON output:
    - avoid `HashMap` in event structs; use `BTreeMap` when needed

  **Must NOT do**:
  - Do not persist raw tool arguments that may contain secrets; persist a redacted summary + digest.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: schema correctness + replay contract is foundational
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 6-27 | Blocked By: 1, 3, 4

  **References**:
  - Pi inspiration (layered events, JSONL sessions): https://docs.rs/pi_agent_rust/latest/pi/
  - insta snapshots: https://insta.rs/docs/

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core event` passes
  - [ ] In deterministic mode, serialized example event snapshots are stable (`INSTA_UPDATE=no`)

  **QA Scenarios**:
  ```
  Scenario: Event schema snapshots are stable
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no HARNESS_DETERMINISTIC=1 cargo test -p harness-core event
    Expected: exit 0, no snapshot updates required
    Evidence: .sisyphus/evidence/task-5-event-snapshots.txt

  Scenario: Redaction is applied before persistence
    Tool: Bash
    Steps:
      - (unit test) build an event with "sk-..." in a string field and assert serialized JSON contains [REDACTED_API_KEY]
    Expected: secret string never appears in JSON
    Evidence: .sisyphus/evidence/task-5-event-redaction.txt
  ```

  **Commit**: YES | Message: `feat(core): event schema v1` | Files: `crates/harness-core/src/event/**`

- [x] 6. Implement append-only JSONL event store + replay/subscribe (in-memory + filesystem)

  **What to do**:
  - In `crates/harness-core/src/store/` define `EventStore` trait:
    - `append(envelope_without_seq) -> envelope_with_seq`
    - `replay(from_seq) -> Iterator/Stream`
    - `subscribe(from_seq) -> Stream` (replay then live)
  - Implement:
    - `InMemoryEventStore` for tests (vector + broadcast channel)
    - `JsonlFileEventStore`:
      - session dir: `<session_dir>/<run_id>/`
      - file: `events.jsonl`
      - append: write one JSON line + `\n`, fsync/flush in deterministic mode
      - on open: compute next `seq` by scanning file once
  - Add replay validation:
    - invalid JSON line → deterministic error (and stop)
  - Tests:
    - append order and `seq` monotonicity
    - replay from seq N returns correct suffix
    - deterministic mode produces byte-identical JSONL across two identical runs in unit tests

  **Must NOT do**:
  - Do not emit events from multiple writers; only Coordinator appends to store (enforced later).

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: correctness + determinism + durability
  - Skills: [`rust-async-patterns`] — Reason: tokio streams/channels patterns

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 7-27 | Blocked By: 1, 5

  **References**:
  - JSONL session concept (pi): https://github.com/Dicklesworthstone/pi_agent_rust/blob/5bffab9e715fcaa4ec2053b21c2da3e7b8db7e50/src/session.rs#L1-L4

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core store` passes
  - [ ] deterministic store tests verify identical JSONL digests

  **QA Scenarios**:
  ```
  Scenario: Replay from JSONL yields same projection inputs
    Tool: Bash
    Steps:
      - cargo test -p harness-core store
    Expected: tests pass, including corruption-path test
    Evidence: .sisyphus/evidence/task-6-store-tests.txt

  Scenario: Corrupt JSONL line fails deterministically
    Tool: Bash
    Steps:
      - (unit test) write invalid JSON line and assert error code/message
    Expected: stable error surface (no panics)
    Evidence: .sisyphus/evidence/task-6-store-corruption.txt
  ```

  **Commit**: YES | Message: `feat(core): jsonl event store + replay` | Files: `crates/harness-core/src/store/**`

### Wave 2 — Core runtime (Coordinator + permissions + tools + hashline)

- [x] 7. Implement Coordinator actor (single scheduling authority + single event writer)

  **What to do**:
  - In `crates/harness-core/src/coord/` implement:
    - `Command` enum (bounded mpsc) for:
      - `StartRun { run_name, workspace_root }`
      - `StopRun`
      - `SpawnAgent { profile, parent_agent_id }`
      - `RequestToolCall { actor, tool_id, args_json }`
      - `ResolvePermission { permission_id, decision }`
      - `JobProgress { task_id, kind }` (for stale tracking)
      - `JobFinished { task_id, outcome }`
    - `Coordinator` task that:
      - is the **only** component allowed to call `EventStore::append`
      - owns run state: agents/tasks/pending permissions
      - stamps + appends events using `Clock` + `Redactor`
      - spawns background jobs with `CancellationToken` and receives results via `Command`
  - Session directory creation:
    - on `StartRun`, create `<session_dir>/<run_id>/` and `<session_dir>/<run_id>/artifacts/`
    - open `JsonlFileEventStore` at `<session_dir>/<run_id>/events.jsonl`
  - Enforce role rules:
    - only `ActorKind::Supervisor` may `SpawnAgent`
    - Worker attempts cause an event `PolicyViolationDetected` (add to `EventV1`) and return error
  - Add integration tests (tokio):
    - start run → `RunStarted` appended
    - spawn agent → `AgentSpawned` appended
    - stop run → `RunFinished` appended

  **Must NOT do**:
  - Do not block the coordinator loop on long IO; long work must be in spawned jobs.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: central invariant (single writer, ordering)
  - Skills: [`rust-async-patterns`] — Reason: tokio actor patterns + cancellation

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8-27 | Blocked By: 2, 3, 4, 5, 6

  **References**:
  - Oracle architecture guidance (single-writer coordinator): bg_1ee07d02 results (session: ses_353478d02ffe8i427VVqk6kDEw)
  - Tokio cancellation: `tokio_util::sync::CancellationToken`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core coord` passes
  - [ ] Starting/stopping a run writes a session directory with `events.jsonl`

  **QA Scenarios**:
  ```
  Scenario: Coordinator appends lifecycle events
    Tool: Bash
    Steps:
      - cargo test -p harness-core coord
    Expected: tests assert RunStarted/AgentSpawned/RunFinished ordering by seq
    Evidence: .sisyphus/evidence/task-7-coordinator-tests.txt

  Scenario: Worker cannot spawn agents
    Tool: Bash
    Steps:
      - (integration test) send SpawnAgent as Worker
    Expected: error returned AND PolicyViolationDetected event recorded
    Evidence: .sisyphus/evidence/task-7-no-redelegate.txt
  ```

  **Commit**: YES | Message: `feat(core): coordinator actor + run lifecycle` | Files: `crates/harness-core/src/coord/**`, `crates/harness-core/src/event/**`

- [x] 8. Add scheduler slots (provider/model concurrency), cancellation, and stale detection

  **What to do**:
  - Implement coordinator-managed concurrency gates (no semaphore acquire-in-select):
    - `ConcurrencyKey`:
      - `ProviderModel { provider_id, model_id }`
      - `Tool { tool_id }`
    - `SlotGate { limit, in_flight, queue: VecDeque<TaskSpec> }`
  - When scheduling jobs:
    - if slot available → start immediately and emit `TaskScheduled { state: started }`
    - else enqueue and emit `TaskScheduled { state: queued }`
    - when dequeued → emit `TaskScheduled { state: started }` for same `task_id`
  - Cancellation:
    - queued task cancellation removes from queue
    - running cancellation triggers token; if completion arrives after cancel, record `TaskResultLate` and ignore side effects
  - Stale detection:
    - track `last_progress_mono_ms` per task (updated via `Command::JobProgress`)
    - watchdog tick (tokio interval) checks `mono_ms - last_progress > staleTimeoutMs`
    - on stale: emit `StaleDetected` (add to `EventV1`), then cancel token
  - Tests (use `#[tokio::test(start_paused = true)]` where needed):
    - concurrency limit 1 queues second job, starts after first finishes
    - queued cancel removes entry
    - stale detection emits event and cancels task

  **Must NOT do**:
  - Do not rely on task completion order for correctness (`JoinSet` is completion-ordered).

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: scheduler correctness + determinism
  - Skills: [`rust-async-patterns`] — Reason: cancellation + watchdog testing

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 13-27 | Blocked By: 7

  **References**:
  - Tokio JoinSet completion ordering note: https://github.com/tokio-rs/tokio/blob/8c980ea75a0f8dd2799403777db700c2e8f4cda4/tokio/src/task/join_set.rs#L19-L22
  - Tokio cancellation-safety list (avoid semaphore acquire in select): https://github.com/tokio-rs/tokio/blob/8c980ea75a0f8dd2799403777db700c2e8f4cda4/tokio/src/macros/select.rs#L121-L129

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core sched` passes
  - [ ] tests demonstrate queueing + stale cancel deterministically under paused time

  **QA Scenarios**:
  ```
  Scenario: Concurrency slots queue and dequeue deterministically
    Tool: Bash
    Steps:
      - HARNESS_DETERMINISTIC=1 cargo test -p harness-core sched
    Expected: tests assert TaskScheduled queued→started and no deadlocks
    Evidence: .sisyphus/evidence/task-8-scheduler.txt

  Scenario: Stale task is cancelled
    Tool: Bash
    Steps:
      - (test) advance tokio time beyond staleTimeoutMs and assert StaleDetected + TaskCancelled
    Expected: cancellation occurs and late results become TaskResultLate
    Evidence: .sisyphus/evidence/task-8-stale.txt
  ```

  **Commit**: YES | Message: `feat(core): concurrency slots + stale watchdog` | Files: `crates/harness-core/src/sched/**`, `crates/harness-core/src/coord/**`, `crates/harness-core/src/event/**`

- [x] 9. Implement permission engine (allow/deny/ask) with deterministic headless resolution

  **What to do**:
  - In `crates/harness-core/src/perm/` define:
    - `PermissionKind`: `EditFs | Shell | Network`
    - `PermissionDecision`: `Allow | Deny`
    - `PermissionPolicy` reads config defaults + per-category overrides
  - Coordinator flow for privileged actions:
    - before starting a tool call, evaluate policy
    - if allow → proceed
    - if deny → emit `PermissionResolved` (deny) and fail tool call
    - if ask → emit `PermissionRequested { permission_id, tool_call_id, summary_redacted, request_digest, timeout_ms, default=Deny }` and pause tool until `ResolvePermission`
    - timeout → auto-resolve to default (deny)
  - Headless runner behavior (locked): default-deny on asks unless scenario sends `ResolvePermission`.
  - Tests:
    - allow path proceeds
    - ask path blocks until resolved
    - timeout path denies deterministically

  **Must NOT do**:
  - Do not allow implicit approvals in headless/CI mode.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: policy + coordinator integration
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 12, 16, 22 | Blocked By: 7

  **References**:
  - OMO permission schema semantics (ask/allow/deny): `~/.config/opencode/node_modules/oh-my-opencode/dist/oh-my-opencode.schema.json` (see permission enums around lines 153-221)

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core perm` passes

  **QA Scenarios**:
  ```
  Scenario: Ask permission blocks until resolved
    Tool: Bash
    Steps:
      - cargo test -p harness-core perm
    Expected: tests assert PermissionRequested then PermissionResolved then ToolCallStarted
    Evidence: .sisyphus/evidence/task-9-permissions.txt

  Scenario: Headless default-deny without resolve
    Tool: Bash
    Steps:
      - (test) request edit permission without ResolvePermission
    Expected: tool call fails after timeout with deterministic error
    Evidence: .sisyphus/evidence/task-9-headless-deny.txt
  ```

  **Commit**: YES | Message: `feat(core): permissions allow/deny/ask` | Files: `crates/harness-core/src/perm/**`, `crates/harness-core/src/event/**`, `crates/harness-core/src/coord/**`

- [x] 10. Implement tool framework + capability-gated registries (anti-footgun)

  **What to do**:
  - In `crates/harness-core/src/tool/` define:
    - `ToolId` (string), `ToolCapability` (`ReadFs | EditFs | Shell | Network | SpawnAgent`)
    - `ToolCall` { `tool_call_id`, `actor`, `tool_id`, `args_digest`, `args_redacted_summary` }
    - `ToolResult` { `display_text`, `structured_json`, `artifacts: Vec<ArtifactRef>` }
    - `Tool` trait: async `call(ctx, args_json) -> ToolResult`
    - `ToolRegistry` with capability filtering by `ActorKind`
  - Coordinator integration:
    - emits `ToolCallRequested/Started/Finished` events with shared `correlation_id = tool_call_id`
    - permission preflight uses `PermissionKind` derived from `ToolCapability`
  - Anti-footgun:
    - worker registry excludes tools requiring `SpawnAgent`
    - coordinator double-checks capability on every tool call and emits `PolicyViolationDetected` on violation
  - Add a tiny built-in tool in `harness-tools` to validate end-to-end wiring:
    - `fs.read` (ReadFs) reading a file from workspace root
    - `shell.run` (Shell) with strict allowlist from config (`permissions.shell_allowlist`), capturing stdout/stderr digests (not full text by default)
  - Tests:
    - worker cannot call SpawnAgent tools
    - tool events include correlation ids and redacted args summary
    - shell allowlist denies unknown executables and unsafe cwd

  **Must NOT do**:
  - Do not allow tools to append events directly; tools return results and coordinator records events.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: core interface surface
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 12-27 | Blocked By: 7, 9

  **References**:
  - Tool output should be UI-friendly structured data (Pi philosophy): see user brief

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features` passes
  - [ ] A tool call produces ToolCallRequested/Started/Finished events with correlation ids

  **QA Scenarios**:
  ```
  Scenario: Tool call emits correlated events
    Tool: Bash
    Steps:
      - cargo test -p harness-core tool
      - cargo test -p harness-tools
    Expected: tests assert correlation_id linkage and permission preflight integration
    Evidence: .sisyphus/evidence/task-10-tools.txt

  Scenario: Worker capability gating
    Tool: Bash
    Steps:
      - (test) attempt a restricted tool as worker
    Expected: PolicyViolationDetected event and tool call denied
    Evidence: .sisyphus/evidence/task-10-tools-gating.txt
  ```

  **Commit**: YES | Message: `feat(core): tool framework + capability gating` | Files: `crates/harness-core/src/tool/**`, `crates/harness-tools/src/**`, `crates/harness-core/src/coord/**`

- [x] 11. Implement hashline engine (pure, clean-room) + exhaustive tests

  **What to do**:
  - In `crates/harness-core/src/edit/hashline.rs` implement:
    - `LineAnchor { line: u32, hash: String /* hex12 */ }`
    - `fn compute_line_hash(line: &str) -> String`:
      - strip trailing `\r`, then `blake3` bytes, take 12 hex chars
    - Patch format (serde):
      - `HashlinePatch { edit_id, path, ops: Vec<HashlineOp> }`
      - `HashlineOp` variants:
        - `InsertBefore { anchor: LineAnchor, lines: Vec<String> }`
        - `InsertAfter { anchor: LineAnchor, lines: Vec<String> }`
        - `Replace { expected: Vec<LineAnchor>, lines: Vec<String> }`
        - `Delete { expected: Vec<LineAnchor> }`
    - Apply algorithm (atomic):
      1) validate all anchors match current content at the referenced line numbers
      2) detect overlaps/conflicts between ops
      3) apply ops bottom-up (descending line) to avoid index drift
      4) return new content + changed ranges
    - Structured errors: `ANCHOR_MISMATCH`, `OUT_OF_RANGE`, `OVERLAP`, `EMPTY_PATCH`
  - Tests (MUST be thorough):
    - unit tests for hashing normalization (CRLF/LF, unicode, tabs, empty lines)
    - golden tests for small file edits (expected output)
    - property tests (proptest): random non-overlapping ops preserve atomicity (either full success or unchanged)

  **Must NOT do**:
  - Do not implement fuzzy relocation on Day-1; strict mismatch reject only.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: correctness primitive + property testing
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 12, 23 | Blocked By: 1, 4 (hash function), checklist

  **References**:
  - Hashline conceptual references (do not copy code):
    - https://github.com/can1357/oh-my-pi/blob/4e2ff5acd7df3bfd572848ae98cca56bdf6e0052/packages/coding-agent/src/patch/hashline.ts#L1-L12
    - RFC6902 JSON Patch `test` semantics (preconditions): https://datatracker.ietf.org/doc/html/rfc6902#section-4.6

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core hashline` passes
  - [ ] property tests run in CI (may be `--release` with bounded cases)

  **QA Scenarios**:
  ```
  Scenario: Hashline rejects on mismatch and never partially applies
    Tool: Bash
    Steps:
      - cargo test -p harness-core hashline
    Expected: tests cover mismatch, overlap, and atomicity invariants
    Evidence: .sisyphus/evidence/task-11-hashline-tests.txt

  Scenario: CRLF normalization
    Tool: Bash
    Steps:
      - (test) hash "line\r" equals hash "line"
    Expected: equality holds
    Evidence: .sisyphus/evidence/task-11-hashline-crlf.txt
  ```

  **Commit**: YES | Message: `feat(edit): hashline engine + tests` | Files: `crates/harness-core/src/edit/hashline.rs`, `crates/harness-core/src/edit/**`

- [x] 12. Implement hashline filesystem tool (atomic apply) + permission integration

  **What to do**:
  - In `crates/harness-tools/src/hashline_apply.rs` implement tool `edit.hashline_apply` (capability `EditFs`):
    - reads target file under workspace root
    - computes current anchors and validates patch anchors
    - applies patch using core hashline engine
    - writes file atomically (tempfile + rename)
  - Coordinator integration:
    - tool call emits: `ToolCall*` + `PermissionRequested/Resolved` + `EditProposed` + `EditApplied/EditRejected`
    - on `EditApplied`, compute `new_file_digest` (blake3 of full file) and include in event
  - Add integration tests using `tempfile::TempDir`:
    - success path writes correct content
    - mismatch path does not change file
    - permission ask path blocks until resolved

  **Must NOT do**:
  - Never write partial files; atomic rename only.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: correctness + filesystem atomicity
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 16, 22, 23 | Blocked By: 7, 9, 10, 11

  **References**:
  - tempfile atomic write patterns: https://docs.rs/tempfile

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tools` passes
  - [ ] hashline tool integration tests prove no partial writes on mismatch

  **QA Scenarios**:
  ```
  Scenario: Hashline tool applies patch atomically
    Tool: Bash
    Steps:
      - cargo test -p harness-tools hashline_apply
    Expected: tests assert file contents and emitted events
    Evidence: .sisyphus/evidence/task-12-hashline-tool.txt

  Scenario: Permission ask blocks apply
    Tool: Bash
    Steps:
      - (integration test) request edit, do not resolve, assert no file change
    Expected: apply does not occur without PermissionResolved
    Evidence: .sisyphus/evidence/task-12-hashline-permission.txt
  ```

  **Commit**: YES | Message: `feat(edit): hashline apply tool + permissions` | Files: `crates/harness-tools/src/hashline_apply.rs`, `crates/harness-core/src/coord/**`

### Wave 3 — Providers + headless runner + sessions/replay

- [x] 13. Add provider abstraction + deterministic MockProvider (offline-first)

  **What to do**:
  - In `crates/harness-providers/src/lib.rs` define:
    - `ProviderId` (string)
    - `ModelId` (string)
    - `CompletionRequest { model_id, messages, temperature, max_tokens, stream }`
    - `ProviderStreamEvent`: `Start | TextDelta(String) | Done { usage } | Error { message }`
    - `trait Provider { async fn stream_completion(req) -> Stream<ProviderStreamEvent>; }`
  - Implement `MockProvider`:
    - keyed by `request_digest` (blake3 of normalized request)
    - returns scripted `ProviderStreamEvent` sequences from fixtures
  - Add fixtures under `crates/harness-testkit/fixtures/mock_provider/`.
  - Tests:
    - deterministic streaming order
    - unknown digest returns `Error` deterministically

  **Must NOT do**:
  - No network access in `MockProvider`.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: foundational abstraction + fixtures
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 14-18 | Blocked By: 1, 5

  **References**:
  - Pi provider stream event layering inspiration: `pi::model::StreamEvent` https://github.com/Dicklesworthstone/pi_agent_rust/blob/5bffab9e715fcaa4ec2053b21c2da3e7b8db7e50/src/model.rs#L225-L278

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers` passes offline

  **QA Scenarios**:
  ```
  Scenario: MockProvider streams deterministic deltas
    Tool: Bash
    Steps:
      - cargo test -p harness-providers mock_provider
    Expected: tests assert exact emitted delta sequence
    Evidence: .sisyphus/evidence/task-13-mock-provider.txt

  Scenario: Unknown request digest returns deterministic error
    Tool: Bash
    Steps:
      - (test) request unknown digest
    Expected: ProviderStreamEvent::Error with stable message
    Evidence: .sisyphus/evidence/task-13-mock-provider-error.txt
  ```

  **Commit**: YES | Message: `feat(provider): abstraction + mock provider` | Files: `crates/harness-providers/src/**`, `crates/harness-testkit/fixtures/**`

- [x] 14. Implement OpenAI-compatible proxy provider (streaming SSE) + offline wiremock tests + gated live test

  **What to do**:
  - In `crates/harness-providers/src/openai_compatible.rs` implement provider using config:
    - base URL like `http://127.0.0.1:8317/v1`
    - endpoint: `POST {base_url}/chat/completions` (OpenAI-compatible)
    - `Authorization: Bearer <api_key>` header (never persisted)
    - `stream: true` and parse SSE chunks into `TextDelta`
  - Use an SSE parser crate (e.g., `eventsource-stream`) or strict manual parsing.
  - Offline tests:
    - `wiremock` server emits a deterministic SSE transcript
    - test asserts deltas are parsed into identical `ProviderStreamEvent`s
  - Gated live test (ignored by default):
    - enabled only when `HARNESS_LIVE_PROXY=1`
    - reads provider config from file
    - hits CLIProxy baseURL and asserts basic response shape
  - Redaction:
    - ensure logs/events never contain `api_key` or Authorization header

  **Must NOT do**:
  - Do not make live network a requirement for the default test suite.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: streaming parsing is subtle + flaky without care
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 15-18 | Blocked By: 2, 4, 13

  **References**:
  - CLIProxy-style baseURL example: `~/.config/opencode/opencode.json:108-110` (behavioral reference)

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers openai_compatible` passes offline
  - [ ] `HARNESS_LIVE_PROXY=1 cargo test -p harness-providers -- --ignored` passes locally (optional)

  **QA Scenarios**:
  ```
  Scenario: SSE parsing works via wiremock
    Tool: Bash
    Steps:
      - cargo test -p harness-providers openai_compatible
    Expected: tests assert exact delta stream
    Evidence: .sisyphus/evidence/task-14-openai-compatible-wiremock.txt

  Scenario: No API key leaked in logs
    Tool: Bash
    Steps:
      - (test) run provider with api_key="sk-TESTSECRET" and assert any persisted JSON/event strings are redacted
    Expected: persisted text contains [REDACTED_API_KEY]
    Evidence: .sisyphus/evidence/task-14-no-leak.txt
  ```

  **Commit**: YES | Message: `feat(provider): openai-compatible proxy streaming` | Files: `crates/harness-providers/src/openai_compatible.rs`

- [x] 15. Implement minimal agent runtime (single-turn streaming) + parallel multi-agent scheduling

  **What to do**:
  - In `crates/harness-core/src/agent/` define:
    - `AgentProfile { name, category, model_ref, system_prompt, toolset }`
    - `AgentRequest { agent_id, prompt, model_ref }`
  - Coordinator integration:
    - `SpawnAgent` creates agent_id and schedules an `AgentTurn` task
    - agent task calls provider and streams `ProviderStreamDelta` back to Coordinator via `Command`
    - coordinator records `ProviderRequestStarted/Delta/Finished`
  - Scope for MVP:
    - one-shot completion only (no tool calling loop yet)
  - Tests:
    - spawn 2 agents concurrently under a concurrency limit, assert queueing + both complete
    - correlation ids isolate each agent turn

  **Must NOT do**:
  - Do not introduce complex multi-turn memory or tool-calling; keep to one-shot streaming.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: coordinator + provider integration
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 16-18, 20-22 | Blocked By: 7, 8, 13

  **References**:
  - Event envelope correlation/causation fields (Oracle guidance): bg_1ee07d02

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features` passes
  - [ ] integration tests demonstrate concurrent agents with deterministic event ordering by `seq`

  **QA Scenarios**:
  ```
  Scenario: Two agents run concurrently and both finish
    Tool: Bash
    Steps:
      - cargo test -p harness-core agent
    Expected: tests assert queued→started transitions under concurrency gate
    Evidence: .sisyphus/evidence/task-15-agents.txt

  Scenario: Cancellation stops an agent turn
    Tool: Bash
    Steps:
      - (test) cancel running agent task token
    Expected: TaskCancelled emitted; any late deltas become TaskResultLate
    Evidence: .sisyphus/evidence/task-15-agent-cancel.txt
  ```

  **Commit**: YES | Message: `feat(core): minimal agent runtime (streaming)` | Files: `crates/harness-core/src/agent/**`, `crates/harness-core/src/coord/**`

- [x] 16. Build headless scenario runner (golden_path) + deterministic "run twice → identical JSONL digest"

  **What to do**:
  - Implement `harness run` subcommand:
    - `--scenario golden_path` (built-in headless scenario; auto-resolves permission)
    - `--scenario golden_path_interactive` (built-in interactive scenario; waits for user approval in TUI)
    - `--deterministic` (forces FakeClock + deterministic run_id)
    - `--session-dir <path>` (overrides config; required for tests)
    - `--out <path>` optional (copies/concats JSONL to a single file for CI diff)
    - `--print-run-dir` (prints run directory path only; no extra logs; for scripts/tests)
  - Golden path scenario steps (MUST include permissions + edit + parallelism):
    1) create a temp workspace directory and a file `demo.txt` with fixed content
    2) start run
    3) spawn **two agents** (planner + worker) using MockProvider
    4) worker requests `edit.hashline_apply` to replace one line in `demo.txt`
    5) permission is `ask` →
       - headless `golden_path`: scenario sends `ResolvePermission(Allow)`
       - interactive `golden_path_interactive`: scenario waits until UI resolves permission
    6) run finishes
  - Deterministic run_id:
    - derive from `seed` + scenario name via UUIDv5 namespace constant
  - Test (E2E, offline):
    - run golden_path twice in deterministic mode, write `target/run1.jsonl` and `target/run2.jsonl`
    - assert `sha256(run1)==sha256(run2)`

  **Must NOT do**:
  - Do not rely on real time or network.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: end-to-end orchestration wiring
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 19-24 | Blocked By: 2, 7-12, 15

  **References**:
  - Metis deterministic run requirement: “run twice → identical JSONL digest”

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace --all-features` passes offline
  - [ ] `HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --out target/run.jsonl` exits 0
  - [ ] deterministic test asserts identical digests across two runs

  **QA Scenarios**:
  ```
  Scenario: Golden path produces stable JSONL digest
    Tool: Bash
    Steps:
      - cargo test -p harness -- golden_path_determinism
    Expected: test writes run1.jsonl/run2.jsonl and hashes match
    Evidence: .sisyphus/evidence/task-16-golden-digest.txt

  Scenario: Missing permission resolution fails deterministically
    Tool: Bash
    Steps:
      - cargo run -p harness -- run --scenario golden_path --deterministic || true
    Expected: run fails with PermissionTimeout/Denied and no file changes
    Evidence: .sisyphus/evidence/task-16-missing-permission.txt
  ```

  **Commit**: YES | Message: `feat(headless): scenario runner + golden path` | Files: `crates/harness/src/**`, `crates/harness-core/src/**`

- [x] 17. Implement session metadata + artifact store + replay CLI

  **What to do**:
  - Session layout (under `paths.session_dir`, default `.agent-harness/sessions`):
    - `<run_id>/meta.json`
    - `<run_id>/events.jsonl`
    - `<run_id>/artifacts/`
  - Write `meta.json` on `RunStarted` with:
    - `run_id`, `run_name`, `workspace_root`, `created_at` (null in deterministic mode), `config_digest`, `harness_version`
  - Implement `ArtifactStore` in core:
    - `write_text(name, contents) -> ArtifactRef { rel_path, digest }`
    - stores under `artifacts/`
  - Implement `harness replay --session <run_dir>`:
    - reads JSONL, applies projections, prints summary (and `--json` mode)
  - Implement `harness sessions list`:
    - lists run dirs, shows run_name + status from projections
  - Tests:
    - meta.json written and deterministic
    - replay summary matches expected values for golden_path

  **Must NOT do**:
  - Do not require SQLite/indexing in MVP; JSONL is source of truth.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: CLI + file IO + projections integration
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 21, 23-24 | Blocked By: 6, 16

  **References**:
  - Pi session header concept: `SessionHeader` https://github.com/Dicklesworthstone/pi_agent_rust/blob/5bffab9e715fcaa4ec2053b21c2da3e7b8db7e50/src/session.rs#L2843-L2862

  **Acceptance Criteria**:
  - [ ] `cargo run -p harness -- replay --session <run_dir>` prints summary and exits 0
  - [ ] unit/integration tests cover deterministic meta.json and replay

  **QA Scenarios**:
  ```
  Scenario: Replay prints stable summary
    Tool: Bash
    Steps:
      - RUN_DIR=$(HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --print-run-dir)
      - cargo run -p harness -- replay --session "$RUN_DIR"
    Expected: summary includes RunFinished and EditApplied
    Evidence: .sisyphus/evidence/task-17-replay.txt

  Scenario: meta.json is deterministic
    Tool: Bash
    Steps:
      - RUN_DIR=$(HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --print-run-dir)
      - jq . "$RUN_DIR/meta.json"
    Expected: created_at is null/constant and config_digest stable
    Evidence: .sisyphus/evidence/task-17-meta.txt
  ```

  **Commit**: YES | Message: `feat(session): meta + artifacts + replay` | Files: `crates/harness-core/src/session/**`, `crates/harness/src/**`

- [x] 18. Implement projections (pure reducers) for UI + replay invariants

  **What to do**:
  - In `crates/harness-core/src/proj/` implement pure projections:
    - `RunSummary` (status, counts, last error, tasks in flight, pending permissions)
    - `TimelineIndex` (events list, correlation groupings)
  - Ensure projections consume events strictly by `seq` order.
  - Add tests:
    - applying the same JSONL twice yields identical `RunSummary`
    - projections ignore side effects (no tool execution during replay)

  **Must NOT do**:
  - No IO in projections (pure functions only).

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: pure logic + tests
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 19-23 | Blocked By: 5

  **References**:
  - Event-sourcing projection model (Oracle guidance): bg_1ee07d02

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core proj` passes

  **QA Scenarios**:
  ```
  Scenario: Projections are deterministic
    Tool: Bash
    Steps:
      - cargo test -p harness-core proj
    Expected: tests assert identical summaries across replays
    Evidence: .sisyphus/evidence/task-18-projections.txt

  Scenario: Replay is side-effect free
    Tool: Bash
    Steps:
      - (test) run replay on JSONL while tools are disabled
    Expected: projection succeeds without calling any tool
    Evidence: .sisyphus/evidence/task-18-replay-no-tools.txt
  ```

  **Commit**: YES | Message: `feat(core): projections for replay + ui` | Files: `crates/harness-core/src/proj/**`

### Wave 4 — Modern Ratatui TUI (live + replay) with snapshot + PTY E2E hooks

- [x] 19. Build Ratatui TUI skeleton (multi-pane layout, keymap, canonical event loop) + snapshot tests

  **What to do**:
  - In `crates/harness-tui` implement:
    - `AppState` (selected event index, focus, follow mode, active tab)
    - tabs: `Events | Output | Diff | Help`
    - Events tab invariant (required for PTY tests): each row must include the **exact** `EventV1` variant name (e.g., `PermissionRequested`, `EditApplied`, `RunFinished`)
    - renderer functions per tab (pure-ish; take state + projection)
  - Canonical crossterm loop (single thread):
    - `event::poll(timeout)` then `event::read()`
    - process **only key-press** events (`KeyEventKind::Press` / `as_key_press_event`)
    - coalesce resize bursts
  - Keymap (locked for tests):
    - `q` quit
    - `?` help
    - `Tab` cycle focus
    - `1` Events, `2` Output, `3` Diff
    - `j/k` or `Up/Down` navigate
    - `Space` toggle follow/auto-scroll
  - Add integration snapshot tests:
    - use `Terminal<TestBackend>` to render fixed-size frames (80x24)
    - snapshot with `insta` (with filters/redactions)

  **Must NOT do**:
  - Do not use Crossterm `EventStream` or mix event APIs across threads.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: UI layout + ergonomics
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 20-24 | Blocked By: 18

  **References**:
  - Ratatui testing recipes: https://ratatui.rs/recipes/testing/ and https://ratatui.rs/recipes/testing/snapshots/
  - TestBackend docs: https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html
  - Ratatui demo event loop: https://github.com/ratatui/ratatui/blob/ee4b7a900cf293fc1ed3b406a4edeaa220d0b93f/examples/apps/demo/src/crossterm.rs#L52-L75
  - Crossterm warning (don’t mix APIs): https://github.com/crossterm-rs/crossterm/blob/4f08595ef4477de2d504dcced24060ed9e3d582a/src/event.rs#L12-L16
  - Key-press filtering helper: https://github.com/crossterm-rs/crossterm/blob/4f08595ef4477de2d504dcced24060ed9e3d582a/src/event.rs#L630-L655
  - Resize bursts note: https://github.com/crossterm-rs/crossterm/blob/4f08595ef4477de2d504dcced24060ed9e3d582a/src/event.rs#L547-L549

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui` passes
  - [ ] snapshots are stable (`INSTA_UPDATE=no`)

  **QA Scenarios**:
  ```
  Scenario: TUI renders deterministically to TestBackend
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui
    Expected: snapshot tests pass without updates
    Evidence: .sisyphus/evidence/task-19-tui-snapshots.txt

  Scenario: Keymap is wired
    Tool: Bash
    Steps:
      - (unit test) feed key events and assert state transitions (tab switch, follow toggle)
    Expected: deterministic state updates
    Evidence: .sisyphus/evidence/task-19-tui-keymap.txt
  ```

  **Commit**: YES | Message: `feat(tui): skeleton layout + keymap + snapshots` | Files: `crates/harness-tui/src/**`

- [x] 20. Implement TUI live mode (subscribe to Coordinator/store) + grouped tool/provider streams

  **What to do**:
  - Wire `harness tui` subcommand to run coordinator + TUI in one process:
    - support `--session-dir <path>` to override config (required for PTY tests)
    - add `--exit-on-finish` (automation mode): exit 0 after `RunFinished`/`RunFailed` is observed and no permission modal is active
    - coordinator runs the selected scenario (or idle mode later)
    - TUI subscribes to `EventStore::subscribe(from_seq)` and updates projections
  - In UI model, group by `correlation_id`:
    - ToolCallRequested/Started/Finished in one expandable group
    - ProviderRequestStarted/Delta/Finished in one expandable group
  - Output tab:
    - show streaming text assembled from ProviderStreamDelta events
    - follow-mode auto-scroll
  - Tests:
    - feed a synthetic event stream and snapshot Events + Output tabs

  **Must NOT do**:
  - Do not block UI on event reads; use bounded channels and skip intermediate renders if behind.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: streaming UX + grouping
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 22-24 | Blocked By: 19, 6, 7

  **References**:
  - Event ordering: rely on `seq` only (not completion order)

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui` passes
  - [ ] `INSTA_UPDATE=no cargo test -p harness-tui live_mode` covers live-mode rendering snapshots

  **QA Scenarios**:
  ```
  Scenario: Live mode renders golden path
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui live_mode
    Expected: snapshot tests cover a representative live stream including RunStarted → PermissionRequested → EditApplied → RunFinished
    Evidence: .sisyphus/evidence/task-20-tui-live.txt

  Scenario: Grouped tool calls render as one group
    Tool: Bash
    Steps:
      - (snapshot test) render timeline with a tool call triple and assert grouping label
    Expected: grouped display
    Evidence: .sisyphus/evidence/task-20-tui-grouping.txt
  ```
  **Evidence**: `.sisyphus/evidence/task-6-tui-live.txt`, `.sisyphus/evidence/task-20-tui-live.txt`


  **Commit**: YES | Message: `feat(tui): live mode + grouped streams` | Files: `crates/harness-tui/src/**`, `crates/harness/src/**`

- [x] 21. Implement TUI replay mode (load session JSONL and inspect)

  **What to do**:
  - Add `harness tui --replay <run_dir>`:
    - reads `events.jsonl`
    - builds projections
    - allows navigating timeline without coordinator running
  - Add replay UI affordances:
    - header shows session path + run_id
    - `r` reloads from disk
  - Tests:
    - snapshot replay mode using golden_path JSONL fixture

  **Must NOT do**:
  - Replay mode must not execute tools or providers.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 24 | Blocked By: 17, 19

  **References**:
  - Replay contract: Decision-Complete Checklist #1

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no cargo test -p harness-tui` covers replay-mode snapshots

  **QA Scenarios**:
  ```
  Scenario: Replay mode renders from JSONL
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui replay_mode
    Expected: snapshot tests cover replay rendering deterministically
    Evidence: .sisyphus/evidence/task-21-tui-replay.txt

  Scenario: Replay does not execute tools
    Tool: Bash
    Steps:
      - (test) run replay with tools disabled and assert no tool calls occur
    Expected: pure render
    Evidence: .sisyphus/evidence/task-21-tui-replay-no-tools.txt
  ```

  **Commit**: YES | Message: `feat(tui): replay mode` | Files: `crates/harness-tui/src/**`, `crates/harness/src/**`

- [x] 22. Add permission prompt UI (ask/allow/deny) + keyboard approvals

  **What to do**:
  - When a `PermissionRequested` event is active and unresolved:
    - show modal dialog with redacted summary
    - key binds:
      - `a` allow
      - `d` deny
      - `Esc` dismiss (does not resolve)
  - On `a/d`, send `ResolvePermission` command to coordinator (live mode only).
  - Tests:
    - snapshot modal rendering
    - state test: pressing `a` emits ResolvePermission command

  **Must NOT do**:
  - Do not auto-approve in UI; user must explicitly allow.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 24 | Blocked By: 9, 20

  **References**:
  - Permission model locked in Task 9

  **Acceptance Criteria**:
  - [ ] PTY E2E (later) can approve permission with `a`
  - [ ] snapshot tests for modal pass

  **QA Scenarios**:
  ```
  Scenario: Permission modal appears and approves
    Tool: Bash
    Steps:
      - cargo test -p harness-tui permission_modal
    Expected: tests assert modal shown and ResolvePermission issued
    Evidence: .sisyphus/evidence/task-22-permission-ui.txt

  Scenario: Deny path
    Tool: Bash
    Steps:
      - (test) press `d` and assert PermissionResolved(Deny)
    Expected: tool call fails and UI shows denial
    Evidence: .sisyphus/evidence/task-22-permission-deny.txt
  ```
  **Evidence**: `.sisyphus/evidence/task-5-tui-replay.txt`


  **Commit**: YES | Message: `feat(tui): permission modal + approvals` | Files: `crates/harness-tui/src/**`

- [x] 23. Implement diff viewer tab for hashline edits (read diff artifact)

  **What to do**:
  - Update hashline apply tool (Task 12) to also write diff artifact:
    - `artifacts/edit-<edit_id>.diff` (unified diff text)
    - include `diff_rel_path` + `diff_digest` in `EditApplied` event
  - In TUI Diff tab:
    - when selected event is `EditApplied`, load the diff artifact and render in a scrollable view
    - key binds: `j/k` scroll
  - Tests:
    - snapshot Diff tab with a known diff fixture

  **Must NOT do**:
  - Do not embed entire diffs into JSONL events; always store in artifact file.

  **Recommended Agent Profile**:
  - Category: `visual-engineering`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 24 | Blocked By: 12, 17, 19

  **References**:
  - Diff computation crate suggestion: `similar`

  **Acceptance Criteria**:
  - [ ] selecting an EditApplied event shows a diff in Diff tab
  - [ ] diff snapshots stable

  **QA Scenarios**:
  ```
  Scenario: Diff tab renders edit diff
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui diff_tab
    Expected: snapshot includes expected unified diff lines
    Evidence: .sisyphus/evidence/task-23-diff-ui.txt

  Scenario: Missing artifact gracefully handled
    Tool: Bash
    Steps:
      - (test) select EditApplied with missing diff file
    Expected: UI shows "diff artifact missing" message (no panic)
    Evidence: .sisyphus/evidence/task-23-diff-missing.txt
  ```
  **Evidence**: `.sisyphus/evidence/task-6-tui-live.txt`


  **Commit**: YES | Message: `feat(tui): diff viewer + edit artifacts` | Files: `crates/harness-tui/src/**`, `crates/harness-tools/src/hashline_apply.rs`, `crates/harness-core/src/event/**`

### Wave 5 — PTY E2E hardening + CI + docs

- [x] 24. Implement PTY E2E TUI test harness (portable-pty + vt100) + golden snapshots

  **What to do**:
  - Add PTY E2E tests under `crates/harness-testkit/tests/pty_e2e.rs` (Linux-only `cfg(target_os="linux")`).
  - Use `portable-pty` to spawn the compiled `harness` binary with fixed PTY size (80x24).
  - Set deterministic env:
    - `HARNESS_DETERMINISTIC=1`
    - `HARNESS_DISABLE_ANIMATIONS=1`
    - `TERM=xterm-256color`
    - `LANG=C.UTF-8`, `LC_ALL=C.UTF-8`, `TZ=UTC`
  - Spawn command:
    - `harness tui --scenario golden_path_interactive --deterministic --session-dir <tempdir>`
  - Capture + parse screen:
    - read PTY output bytes on a dedicated thread (avoid pipe deadlocks)
    - feed into `vt100::Parser`
    - snapshot `parser.screen().contents()` at checkpoints
  - Keystroke script (locked):
    1) wait until screen contains `PermissionRequested`
    2) send `a` to allow
    3) wait until screen contains `RunFinished`
    4) send `3` (Diff tab), assert diff renders
    5) send `q` to quit
  - Tests must avoid sleeps where possible:
    - poll screen until condition or timeout

  **Must NOT do**:
  - No real terminal required; PTY only.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: PTY determinism + parsing
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 5 | Blocks: 25 | Blocked By: 20, 22, 23

  **References**:
  - portable-pty docs: https://docs.rs/portable-pty
  - portable-pty read loop example (deadlock avoidance): https://github.com/wez/wezterm/blob/05343b387085842b434d267f91b6b0ec157e4331/pty/examples/whoami.rs#L27-L47
  - vt100 screen contents: https://docs.rs/vt100

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no cargo test -p harness-testkit pty_e2e` passes on Linux
  - [ ] snapshots are stable across 5 repeated runs in one CI job

  **QA Scenarios**:
  ```
  Scenario: PTY E2E golden path snapshot
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --nocapture
    Expected: test passes and snapshots match
    Evidence: .sisyphus/evidence/task-24-pty-e2e.txt

  Scenario: Repeat run flake check
    Tool: Bash
    Steps:
      - for i in 1 2 3 4 5; do INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e || exit 1; done
    Expected: all 5 passes
    Evidence: .sisyphus/evidence/task-24-pty-repeat.txt
  ```
  **Evidence**: `.sisyphus/evidence/task-9-pty-e2e.txt`, `.sisyphus/evidence/task-11-pty-e2e.txt`


  **Commit**: YES | Message: `test(e2e): pty tui harness + vt100 snapshots` | Files: `crates/harness-testkit/tests/**`, `crates/harness-testkit/src/**`

- [x] 25. Extend GitLab CI to run Rust fmt/clippy/tests + PTY E2E (Linux) with deterministic env

  **What to do**:
  - Update `.gitlab-ci.yml`:
    - keep existing SAST + secret detection jobs
    - add cache for cargo registry/git
    - ensure Rust jobs run with:
      - `INSTA_UPDATE=no`
      - `RUST_TEST_THREADS=1`
      - `TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8`
    - add PTY E2E job:
      - runs Task 24 repeat loop (5x)
  - Add CI artifacts for failing snapshots/logs (e.g., `target/insta/**` or test stdout).

  **Must NOT do**:
  - Do not require macOS/Windows runners.

  **Recommended Agent Profile**:
  - Category: `quick` — Reason: CI wiring
  - Skills: [`git-master`]

  **Parallelization**: Can Parallel: YES | Wave 5 | Blocks: Final Verification Wave | Blocked By: 24 (for PTY job)

  **References**:
  - Current CI file: `.gitlab-ci.yml:1-19`

  **Acceptance Criteria**:
  - [ ] CI has jobs: fmt, clippy, test, pty_e2e, plus existing SAST/secret detection

  **QA Scenarios**:
  ```
  Scenario: CI file includes PTY E2E job
    Tool: Bash
    Steps:
      - sed -n '1,220p' .gitlab-ci.yml
    Expected: pty_e2e job exists and sets deterministic env vars
    Evidence: .sisyphus/evidence/task-25-gitlab-ci.txt

  Scenario: Local CI command parity
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test --workspace --all-features
    Expected: passes
    Evidence: .sisyphus/evidence/task-25-local-parity.txt
  ```
  **Evidence**: `.sisyphus/evidence/task-10-gitlab-ci.txt`


  **Commit**: YES | Message: `ci: add rust + pty e2e jobs` | Files: `.gitlab-ci.yml`

- [x] 26. Replace README template with real docs (architecture, config, testing) + licensing hygiene note

  **What to do**:
  - Replace `README.md` boilerplate with:
    - project overview + goals
    - quickstart (`harness run`, `harness tui`, `harness replay`)
    - config guide + where to put config + example CLIProxy base_url
    - testing pyramid + how to run PTY E2E deterministically
    - explicit “behavioral inspiration only” note (OMO license hygiene)
  - Add docs:
    - `docs/architecture.md` (crate boundaries, event schema v1, coordinator invariants, permission model, hashline spec)
    - `docs/testing.md` (unit/integration/PTY; deterministic env; snapshot policy)
    - `docs/config.md` (document keys; link to generated schema)

  **Must NOT do**:
  - Do not paste OMO prompts or code.

  **Recommended Agent Profile**:
  - Category: `writing` — Reason: technical docs
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 5 | Blocks: Final Verification Wave | Blocked By: 2, 5

  **References**:
  - User brief (licensing hygiene reminder)

  **Acceptance Criteria**:
  - [ ] README is project-specific and includes commands + testing instructions

  **QA Scenarios**:
  ```
  Scenario: Docs match reality
    Tool: Bash
    Steps:
      - rg "harness run" -n README.md docs/*.md
      - cargo run -p harness -- --help
    Expected: documented commands exist in CLI help
    Evidence: .sisyphus/evidence/task-26-docs.txt

  Scenario: Example config referenced
    Tool: Bash
    Steps:
      - test -f configs/harness.example.jsonc
    Expected: file exists
    Evidence: .sisyphus/evidence/task-26-example-config.txt
  ```

  **Commit**: YES | Message: `docs: add architecture + config + testing` | Files: `README.md`, `docs/**`

- [ ] 27. (Optional) Add VCR record/replay + conformance harness + stdio JSON control plane (pi-inspired)

  **What to do**:
  - Add a `vcr` module (providers):
    - record OpenAI-compatible HTTP/SSE responses to fixtures
    - replay fixtures offline for regression tests
  - Add a conformance harness:
    - re-run golden_path and assert event schema compatibility (versioning rules)
  - Add `harness rpc` subcommand (JSONL protocol over stdin/stdout):
    - accept Command JSON lines, emit Event JSON lines
    - enables automation without TUI

  **Must NOT do**:
  - This task must not become “build plugin system”; keep scope to record/replay + stdio control.

  **Recommended Agent Profile**:
  - Category: `deep`
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 5 | Blocks: none | Blocked By: 14, 16

  **References**:
  - Pi stdio JSON protocol module: https://github.com/Dicklesworthstone/pi_agent_rust/blob/5bffab9e715fcaa4ec2053b21c2da3e7b8db7e50/src/rpc.rs#L1-L4
  - Pi VCR module docs: https://docs.rs/pi_agent_rust/0.1.7/pi/vcr/

  **Acceptance Criteria**:
  - [ ] Offline tests can replay recorded proxy transcripts
  - [ ] `harness rpc` can run golden_path headless via JSON commands

  **QA Scenarios**:
  ```
  Scenario: VCR replay works offline
    Tool: Bash
    Steps:
      - cargo test -p harness-providers vcr
    Expected: passes without network
    Evidence: .sisyphus/evidence/task-27-vcr.txt

  Scenario: Stdio control plane runs a scenario
    Tool: Bash
    Steps:
      - printf '{"cmd":"StartRun"}\n' | cargo run -p harness -- rpc
    Expected: emits Event JSON lines
    Evidence: .sisyphus/evidence/task-27-rpc.txt
  ```

  **Commit**: YES | Message: `feat(headless): vcr + rpc control plane` | Files: `crates/harness-providers/src/vcr/**`, `crates/harness/src/rpc/**`

## Final Verification Wave (4 parallel agents, ALL must APPROVE)
- [ ] F1. Plan Compliance Audit — oracle
- [ ] F2. Code Quality Review — unspecified-high
- [ ] F3. Real Manual QA (Agent-Driven) — unspecified-high (+ PTY tests)
- [ ] F4. Scope Fidelity Check — deep

## Commit Strategy
- Atomic commits per task (or per 2 tightly related tasks) with conventional messages:
  - `chore(workspace): init rust workspace and ci`
  - `feat(core): add event schema v1 and jsonl store`
  - `feat(core): coordinator + permissions + scheduler`
  - `feat(edit): hashline engine + tool`
  - `feat(tui): live + replay ui`
  - `test(e2e): pty harness + golden snapshots`
- Keep CI green: never land snapshot updates without deterministic mode.

## Success Criteria
- A contributor can run `harness tui --scenario golden_path_interactive --deterministic` and see a complete run with permissions + hashline edit, and then replay it.
- CI runs fmt/clippy/tests, including PTY E2E on Linux, without flakes across 5 repeats.
