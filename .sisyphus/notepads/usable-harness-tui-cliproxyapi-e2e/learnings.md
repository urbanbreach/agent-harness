# Learnings and Conventions

## Project Conventions

### Rust Patterns
- Use serde for serialization with #[serde(default)] for optional config sections
- Event-sourced architecture with JSONL append-only logs
- Coordinator pattern for central scheduling

### Config Handling
- Config files use JSONC (JSON with comments)
- Env substitution: ${VAR} = required, ${VAR:-default} = optional with fallback
- Config resolution order: --config flag → ./harness.jsonc → $XDG_CONFIG_HOME/harness/config.jsonc

### Testing
- Unit tests: cargo test --workspace
- Integration: INSTA_UPDATE=no cargo test --workspace --all-features
- PTY E2E: RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e

## Critical Decisions from Plan

1. **Interactive UX**: Prompt input + streaming transcript + activity + inspector panes
2. **Provider**: CLIproxyAPI with Responses API support (Codex-aligned)
3. **Config expansion**: Add optional `ui`, `logging`, `keybindings` (additive)
4. **MVP constraint**: Single default provider only

## File Locations
- Config: crates/harness-core/src/config.rs
- Coordinator: crates/harness-core/src/coord.rs
- Events: crates/harness-core/src/event.rs
- Provider: crates/harness-providers/src/openai.rs
- TUI: crates/harness-tui/src/
- CLI: crates/harness/src/

## 2026-03-03 Schema Expansion Notes

- `HarnessConfig` now supports optional `ui`, `logging`, and `keybindings` sections behind `#[serde(default)]` for backward compatibility.
- `openai_compatible` provider config now includes `api_mode` enum (`responses`, `chat_completions`, `auto`) with default `chat_completions`.
- `keybindings` remains a simple `BTreeMap<String, String>` with fixed default action keys (quit/focus/palette/help/follow/submit/clear/scroll/tab actions).

## 2026-03-03 Live update backpressure notes

- Live TUI transport now uses `crossbeam_channel::bounded(2048)` to cap UI backlog and avoid blocking async event forwarding.
- Display-path `ProviderStreamDelta` events are coalesced per `request_id` for up to 16ms or 1024 characters before enqueue, reducing high-frequency token churn without touching persisted JSONL events.
- When the queue is saturated, delta updates are dropped first and an overload banner (`UI overloaded: dropped N deltas`) is rate-limited to once per second.

## 2026-03-03 OpenAI-compatible Responses streaming notes

- `OpenAiCompatibleProviderConfig` in `harness-providers` now carries `api_mode` with `responses | chat_completions | auto`; provider stores mode and resolves request path per call.
- Added dual endpoint builders for `{base}/chat/completions` and `{base}/responses` so CLIproxy-style `/v1` base URLs fan out correctly.
- Responses-mode request translation uses `model`, `input: [{role, content}]`, and `stream: true` (no chat `messages` payload in this mode).
- Responses SSE parser now accepts either SSE event-name or JSON `type` discriminator (`response.output_text.delta`, `response.completed`, `error`) and ignores keep-alive empty/comment frames.
- `api_mode=auto` now probes `/responses` first and falls back to chat completions only on 404/405, preserving existing chat behavior for compatible proxies.
- Offline wiremock coverage now includes deterministic Responses transcript parsing and auto-fallback path validation; live smoke defaults to Responses mode unless `HARNESS_LIVE_PROXY_API_MODE` overrides.

## 2026-03-03 Live update backpressure verification

- `LiveUpdateForwarder` keeps UI-path updates non-blocking by using `try_send` against a bounded queue and dropping deltas first under sustained pressure.
- Delta coalescing behavior is request-scoped (`request_id`) with dual flush thresholds (16ms window, 1024 chars), which cuts render churn while preserving event-log fidelity.
- Verification pass in worktree: `cargo test -p harness-tui` and `cargo build --workspace` both succeeded after the backpressure/coalescing path update.

## 2026-03-03 Config bootstrap + interactive coordinator wiring notes

- Added `crates/harness/src/bootstrap.rs` to centralize config loading and interactive coordinator construction from `HarnessConfig`.
- Interactive coordinator mapping now derives runtime fields from config: `session_dir`, permission policy from `PermissionPolicy::from_config`, `coordinator_registry(shell_allowlist)`, task/model concurrency, stale timeout, and category-based `AgentProfile` generation.
- Single-provider MVP is enforced in bootstrap: `providers.default` must exist and every category `model_ref` must parse to provider_id `default`.
- Added `crates/harness/src/logging.rs` with file-backed tracing init (`cfg.logging.file` override or `${artifacts_dir}/harness.log`) and level parsing from `cfg.logging.level`.
- `tui` now has a no-flag interactive path that uses `build_interactive_coordinator_config` (config provider path), while `--scenario` mode still uses `golden_path_provider()` and `golden_path_profiles()` unchanged.

## 2026-03-03 Coordinator control-plane turn request notes

- Added coordinator commands/handle methods for prompt-driven control: `spawn_agent_idle(...)` and `request_agent_turn(...)`.
- `spawn_agent_idle` now records `AgentSpawned` without auto-scheduling provider work; existing `spawn_agent` behavior remains auto-scheduled by reusing idle spawn plus scheduling.
- `RunState` now tracks `agent_profile_names: BTreeMap<agent_id, profile_name>` so later turn requests can resolve model/profile deterministically.
- `request_agent_turn` allocates `req_{:06}`, appends `UserMessageSubmitted` with `correlation_id=request_id` + `stream_key=agent:{agent_id}`, then schedules with the resolved profile model.
- Authorization for requested turns is explicit: `User|Supervisor` allowed; `Worker` emits `PolicyViolationDetected` and returns `PolicyViolation`.
- Verification in worktree: `cargo test -p harness-core coord` passed with new idle-spawn/request-turn coverage.

## 2026-03-03 Persisted user prompt event notes

- Added `EventV1::UserMessageSubmitted(UserMessageSubmittedEvent)` to persist replayable user transcript input with both redacted `content` and deterministic `content_digest`.
- `content_digest` follows existing digest12 convention (`blake3(content).to_hex().take(12)`), now centralized via shared `digest12` helper in `event.rs`.
- Projection/type labeling paths updated (`harness-core::proj::event_type_name`, `harness-tui::ui::event_variant_name`) so replay summaries and TUI labels include `user_message_submitted` consistently.
- Added coverage for JSONL serde roundtrip and redaction behavior (`sk-*` replaced with `[REDACTED_API_KEY]`) while retaining stable digest metadata.

## 2026-03-03 Task 16 live proxy E2E notes

- Added gated live CLIproxyAPI E2E test at `crates/harness-testkit/tests/live_proxy_e2e.rs`.
- Test is `#[ignore]` by default; runs only when `HARNESS_LIVE_PROXY=1` is set.
- Uses `HARNESS_LIVE_PROXY_CONFIG` override or falls back to `configs/harness.example.jsonc`.
- Asserts config mentions `api_mode: "responses"` before running.
- Executes `harness prompt --text "Say hello"` and validates event sequence:
  - `provider_request_started` with captured `request_id`
  - at least one `provider_stream_delta` matching that `request_id`
  - `provider_request_finished` matching that `request_id`
- Added pty-mcp evidence capture recipe documenting steps to produce:
  - `.sisyphus/evidence/task-16-live-tui.png`
  - `.sisyphus/evidence/task-16-live-tui-finished.png`
  - `.sisyphus/evidence/task-16-live-log.txt`

