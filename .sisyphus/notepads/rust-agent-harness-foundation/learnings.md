# Accumulated Learnings for rust-agent-harness-foundation

## Conventions

### Code Style
- Rust 2021 edition, stable toolchain
- Use `rustfmt` + `clippy` with `-D warnings`
- Prefer `BTreeMap` over `HashMap` for deterministic serialization
- Use `blake3` for hashing (12 hex chars for line hashes)

### Event Sourcing
- `seq` is the single source of ordering truth (not timestamps)
- Coordinator is the ONLY writer to EventStore
- Events are append-only JSONL
- Redaction happens BEFORE persistence

### Hashline Spec
- Line hash = blake3(line_bytes without trailing \r), truncated to 12 hex chars
- Atomic apply: validate all anchors → detect overlaps → apply bottom-up
- Mismatch = hard reject, never partial apply

### TUI Guidelines
- Single-threaded `poll → read` loop only
- Only `KeyEventKind::Press` events
- Coalesce resize bursts
- Fixed PTY size (80x24) in tests

### Testing
- Unit: direct assertions
- Integration: `insta` snapshots with redactions
- E2E: PTY with `portable-pty` + `vt100`
- Deterministic mode: `HARNESS_DETERMINISTIC=1` uses FakeClock

## Dependencies

### Key Crates
- `tokio` - async runtime
- `ratatui` + `crossterm` - TUI
- `serde` + `serde_json` - serialization
- `json5` - config parsing (JSONC-like)
- `schemars` - JSON Schema generation
- `blake3` - hashing
- `clap` - CLI
- `insta` - snapshot testing
- `tempfile` - atomic file operations
- `portable-pty` + `vt100` - PTY E2E tests
- `wiremock` - HTTP mocking for tests
- `eventsource-stream` - SSE parsing
- `proptest` - property testing

## Patterns

### Actor Pattern (Coordinator)
- Single mpsc command channel
- Spawn background jobs with CancellationToken
- Job results sent back via Command channel

### Permission Flow
1. Tool call requested
2. Policy evaluated (allow/deny/ask)
3. If ask → emit PermissionRequested → pause
4. Wait for ResolvePermission command
5. Timeout → default (deny in headless)

### Anti-Footguns
- Workers cannot spawn agents (enforced by registry + coordinator)
- No fuzzy relocation on hashline mismatch
- No secrets in JSONL (redact before persist)
- No unbounded queues

## File Locations
- Config: `configs/harness.example.jsonc`
- Sessions: `.agent-harness/sessions/<run_id>/`
- Events: `<run_id>/events.jsonl`
- Artifacts: `<run_id>/artifacts/`
- Meta: `<run_id>/meta.json`

## [2026-03-02T05:27:12Z] Task 1 Complete
- Workspace structure created
- CI updated with Rust jobs
- All crates compile
- Evidence: .sisyphus/evidence/task-1-fmt.txt, .sisyphus/evidence/task-1-clippy.txt, .sisyphus/evidence/task-1-test.txt, .sisyphus/evidence/task-1-help.txt

## [2026-03-02T05:34:08Z] Task 2 Complete
- Config system implemented with JSON5 parsing
- JSON Schema generation working
- Example config created
- Evidence: .sisyphus/evidence/task-2-harness-core-test.txt, .sisyphus/evidence/task-2-schema.txt, .sisyphus/evidence/task-2-config-validate.txt, .sisyphus/evidence/task-2-config-invalid.txt

## [2026-03-02T05:33:19Z] Task 4 Complete
- Redaction engine implemented
- SecretScan helper added
- Evidence: .sisyphus/evidence/task-4-redact-test.txt

## [2026-03-02T05:32:43Z] Task 3 Complete
- Clock abstraction with RealClock + FakeClock
- Determinism helper implemented
- Evidence: .sisyphus/evidence/task-3-clock-test.txt

## [2026-03-02T05:41:01Z] Task 5 Complete
- Event schema v1 defined with all required variants
- EventBuilder with Clock + Redactor integration
- Insta snapshots for deterministic serialization
- Evidence: .sisyphus/evidence/task-5-*.txt

## [2026-03-02T05:49:43Z] Task 6 Complete
- EventStore trait with in-memory and JSONL implementations
- Replay and subscribe functionality
- Deterministic JSONL tests
- Evidence: .sisyphus/evidence/task-6-*.txt

## [2026-03-02T05:59:45Z] Task 7 Complete
- Coordinator actor with Command mpsc
- Single event writer invariant enforced
- Role-based access control (Supervisor only spawns agents)
- Evidence: .sisyphus/evidence/task-7-*.txt

## [2026-03-02T06:13:34Z] Task 9 Complete
- Permission engine with allow/deny/ask
- Headless default-deny behavior
- Evidence: .sisyphus/evidence/task-9-*.txt

## [2026-03-02T06:17:43Z] Task 11 Complete
- Hashline engine with atomic apply
- Property tests for atomicity
- Evidence: .sisyphus/evidence/task-11-*.txt

## [2026-03-02T06:29:52Z] Task 10 Complete
- Tool framework with capability gating
- Built-in fs.read and shell.run tools
- Anti-footgun: workers cannot use SpawnAgent tools
- Evidence: .sisyphus/evidence/task-10-*.txt

## [2026-03-02T06:40:46Z] Task 12 Complete
- Hashline filesystem tool with atomic apply
- Permission integration
- No partial writes on mismatch
- Evidence: .sisyphus/evidence/task-12-*.txt

## [2026-03-02T06:45:50Z] Task 13 Complete
- Provider abstraction trait
- MockProvider with fixture-based responses
- Evidence: .sisyphus/evidence/task-13-*.txt

## [2026-03-02T06:45:20Z] Task 18 Complete
- Projections for RunSummary and TimelineIndex
- Pure functions, no IO
- Evidence: .sisyphus/evidence/task-18-*.txt

## [2026-03-02T07:05:00Z] Task 15 Complete
- Minimal agent runtime with streaming
- Concurrent multi-agent scheduling
- Evidence: .sisyphus/evidence/task-15-runtime.txt

## [2026-03-02T06:55:48Z] Task 14 Complete
- OpenAI-compatible provider with SSE streaming
- Wiremock tests for offline validation
- Gated live tests
- Evidence: .sisyphus/evidence/task-14-*.txt

## [2026-03-02T07:04:41Z] Task 16 Complete
- Headless scenario runner
- Deterministic run digest verification
- Evidence: .sisyphus/evidence/task-16-*.txt

## [2026-03-02T07:13:12Z] Task 17 Complete
- Session metadata + artifacts + replay CLI
- Evidence: .sisyphus/evidence/task-17-*.txt

## [2026-03-02T08:02:00Z] harness-tui compile fix
- `harness-tui` crate requires direct `anyhow` dependency when `use anyhow::Result` is used in crate root.
- `ratatui::widgets::Tabs::new` inference can require explicit `Vec<Line>` annotation in some contexts.
- `EventEnvelopeV1` tests must include full v1 envelope metadata fields (`schema_version`, `event_id`, `run_id`, `mono_ms`, `ts: Option<String>`, `actor`, `causation_id`, `stream_key`) matching `harness_core::event`.

## [2026-03-02T08:30:00Z] Task 20 Partial Implementation
- CLI arguments wired (--scenario, --session-dir, --exit-on-finish)
- Compilation fixed with harness-tui dependency
- execute() function is stub - needs coordinator integration
- Missing: EventStore::subscribe integration, correlation grouping, output streaming
- Blocker: Complex async coordination between TUI event loop and coordinator

## [2026-03-02T10:45:31+02:00] Task 21 Complete - TUI Replay Mode
- Added --replay argument to harness tui
- Header shows session path in replay mode
- 'r' key reloads events from disk
- All tests pass including new replay_mode test
## [2026-03-02T11:02:40+02:00] Progress Summary

### Completed Tasks:
- Tasks 1-18: Waves 1-3 complete (scaffold, core runtime, providers, headless)
- Task 19: TUI Skeleton complete
- Task 20: TUI Live Mode - CLI wired (coordinator integration stubbed)
- Task 21: TUI Replay Mode complete
- Task 26: Documentation complete (README, docs/architecture.md, docs/testing.md, docs/config.md)

### Remaining Critical Path:
- Task 20: Complete coordinator integration in TUI (subscribe to EventStore, update projections)
- Task 22: Permission Prompt UI (blocked by Task 20)
- Task 23: Diff Viewer Tab (similar crate added, needs integration)
- Task 24: PTY E2E Test Harness (blocked by 20, 22, 23)
- Task 25: GitLab CI PTY E2E (blocked by 24)

## [2026-03-02T11:18:07+02:00] Task Status Update
- Tasks 1-19, 21, 26: COMPLETE
- Task 20: INCOMPLETE (stub - needs coordinator integration)
- Tasks 22, 24, 25: BLOCKED by Task 20
- Task 23: PARTIAL (similar crate added, needs integration)
- Task 27: Optional, not started


## [2026-03-02T11:23:37+02:00] Final Progress Summary

### COMPLETED (23/31 tasks):
- Tasks 1-19: All Waves 1-3 + TUI Skeleton complete
- Task 21: TUI Replay Mode complete
- Task 26: Documentation complete
- Task 23 Part 1: Diff generation in hashline tool
- Task 23 Part 2: EditAppliedEvent diff fields added

### PARTIAL (2 tasks):
- Task 20: CLI wired but coordinator integration stubbed
- Task 23: Needs coordinator to pass diff info to events, TUI diff viewer rendering

### BLOCKED/PENDING (6 tasks):
- Task 22: Permission UI (blocked by 20)
- Task 23 completion: Diff viewer in TUI (partial)
- Task 24: PTY E2E (blocked by 20, 22, 23)
- Task 25: CI PTY (blocked by 24)
- Task 27: Optional VCR (not started)
- F1-F4: Final verification (not started)

### Test Status:
- cargo test --workspace --all-features: ALL PASS

### Next Critical Path:
1. Complete Task 20: Coordinator integration in TUI
2. Complete Task 22: Permission prompt UI
3. Complete Task 23: TUI diff viewer
4. Implement Task 24: PTY E2E tests

