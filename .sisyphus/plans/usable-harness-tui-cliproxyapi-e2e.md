# Usable Agent Harness: Interactive TUI + CLIproxyAPI (Responses) + Config + E2E

## TL;DR
> **Summary**: Turn the current event-inspector TUI into an **interactive run workspace** (OpenCode/Pi-inspired) that can send prompts to a real provider via **CLIproxyAPI** using the **OpenAI Responses API**, with a fleshed-out config schema and deterministic + live-gated E2E proof.
> **Deliverables**:
> - Interactive TUI: prompt input → streaming transcript → inspector/activity panes + command palette
> - Real provider wiring: CLI uses config to talk to CLIproxyAPI via `/v1/responses` (with optional chat-completions fallback)
> - Expanded config: `ui`, `logging`, `keybindings` (additive) + updated schema/docs/example
> - Thorough verification: unit/integration + PTY E2E + gated live-proxy E2E + pty-mcp evidence captures
> **Effort**: Large
> **Parallel**: YES — 4 waves
> **Critical Path**: Responses provider → coordinator interactive commands → interactive TUI → PTY E2E → gated live E2E

## Context

### Original Request
- Make the bare-bones terminal UI and the harness itself usable.
- Make configuration and provider usable end-to-end with a real provider.
- Use **CLIproxyAPI** (OpenAI-compatible proxy; Codex-aligned) as the provider.
- Take UI/UX inspiration from OpenCode + Pi Mono + Pi Agent Rust.
- Flesh out config + JSON schema using those harnesses as inspiration.
- Add thorough E2E testing using PTY tooling (pty-mcp + existing PTY E2E) with proof/evidence.

### Interview Summary (decisions locked)
- UX MVP: **Interactive run workspace** (prompt input + streaming transcript + activity + inspector panes).
- Provider: **CLIproxyAPI with Responses API support** (Codex-aligned).
- Config expansion: **Add optional `ui`, `logging`, `keybindings`** (additive; keep existing required sections).

### Metis Review (gaps addressed)
- Lock MVP turn model: interactive UX requires a **new coordinator command** (can’t rely on “one-shot on spawn” only).
- Persist prompts for replay: add an explicit event (do not hide prompts only in digests).
- Resolve config env-substitution mismatch (`${VAR}` / `${VAR:-default}`) via code + tests + docs.
- Control scope: enforce **single default provider** for MVP; validate `model_ref` uses `default:<model>`.
- Prevent TUI stalls: replace/mitigate **unbounded live update channels** + add delta coalescing.
- Strengthen secret non-leak checks: scan JSONL/snapshots/artifacts for API key patterns.

## Work Objectives

### Core Objective
Provide a genuinely usable, OpenCode/Pi-inspired interactive TUI that runs real provider calls through CLIproxyAPI (Responses API) using a robust, validated config — backed by deterministic PTY E2E and gated live E2E proof.

### Deliverables
1. **Interactive TUI “Run” workspace**
   - Prompt input box + history
   - Streaming transcript
   - Activity list + inspector
   - Command palette + help overlay
2. **Real provider wiring**
   - Config-driven provider creation
   - Responses API streaming (`/v1/responses`) support
   - Optional fallback to chat completions (`/v1/chat/completions`) when configured
3. **Config schema expansion**
   - `ui` (theme/layout/animations/default profile)
   - `logging` (file/level/redaction toggles)
   - `keybindings` (override a small, fixed action set)
4. **E2E verification with evidence**
   - Offline deterministic PTY E2E covers interactive prompt submission + streaming
   - Gated live-proxy smoke covers CLIproxyAPI responses streaming
   - pty-mcp evidence captures for human-inspectable proof

### Definition of Done (verifiable)
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- [ ] `INSTA_UPDATE=no cargo test --workspace --all-features` passes offline.
- [ ] `RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e` passes offline (Linux).
- [ ] Interactive TUI works:
  - [ ] `cargo run -p harness -- tui` starts interactive run workspace (no scenario required)
  - [ ] typing a prompt + Enter triggers provider request events and transcript streaming
- [ ] CLIproxyAPI (Responses) live smoke works when gated:
  - [ ] `HARNESS_LIVE_PROXY=1 ... cargo test -p harness-providers -- --ignored` includes a Responses-path smoke test that reaches Start→TextDelta→Done
- [ ] Config schema includes new sections and validates example config:
  - [ ] `cargo run -p harness -- schema` includes `ui`, `logging`, `keybindings` definitions
  - [ ] `cargo run -p harness -- config validate --config configs/harness.example.jsonc` exits 0

### Must Have
- Preserve event-sourced auditability: prompts + provider stream events are persisted and replayable.
- No secrets in JSONL/snapshots/artifacts: redaction + regression scans.
- CLIproxyAPI integration uses `/v1/responses` streaming as first-class.
- Deterministic offline PTY E2E remains the primary regression suite.

### Must NOT Have (guardrails)
- Do **NOT** copy code from OpenCode/Oh-My-Pi/Pi-Agent-Rust; only mirror UX behaviors + schema ideas.
- Do **NOT** make live provider calls in default CI path; all live calls are **explicitly gated**.
- Do **NOT** attempt full OpenCode/Pi feature parity (session trees, MCP ecosystem, tool-call parsing) in this iteration.

## Verification Strategy
> ZERO HUMAN INTERVENTION — all verification is agent-executed.
- Test decision: **tests-after** for visual polish; **unit/integration tests** for provider parsing, coordinator commands, and config validation.
- E2E:
  - Offline deterministic: portable-pty + vt100 + insta + PNG artifacts
  - Live-gated: ignored tests requiring local CLIproxyAPI + env vars
- Evidence policy: each TODO writes logs/screenshots to `.sisyphus/evidence/task-{N}-{slug}.{ext}`.

## Execution Strategy

### Parallel Execution Waves
Wave 1 (Config + provider foundations)
Wave 2 (Coordinator interactive turn control plane + interactive mode wiring)
Wave 3 (TUI UX polish: theme/layout/palette/keybinds)
Wave 4 (PTY E2E + live-gated E2E + docs + security regression)

### Dependency Matrix (full, all tasks)

| Task | Wave | Depends On |
|------|------|------------|
| 1 | 1 | — |
| 2 | 1 | 1 |
| 3 | 1 | 1 |
| 4 | 1 | 1,2,3 |
| 5 | 1 | — |
| 6 | 2 | — |
| 7 | 2 | 6 |
| 8 | 2 | 5,6,7 |
| 9 | 2 | 4,7,8 |
| 10 | 2 | 4,7 |
| 11 | 3 | 1,8,9 |
| 12 | 3 | 1,11 |
| 13 | 3 | 11 |
| 14 | 4 | 11,12,13 |
| 15 | 4 | 9,14 |
| 16 | 4 | 3,9,10 |
| 17 | 4 | 14,15,16 |
| 18 | 4 | 1,3,9,10,11 |

### Agent Dispatch Summary (wave → task count → categories)
- Wave 1 (5): 2×`deep`, 2×`ultrabrain`, 1×`unspecified-high`
- Wave 2 (5): 3×`ultrabrain`, 2×`deep`
- Wave 3 (3): 3×`visual-engineering`
- Wave 4 (5): 2×`ultrabrain`, 2×`deep`, 1×`writing`

## TODOs
> Implementation + Test = ONE task. Never separate.
> EVERY task MUST have: Agent Profile + Parallelization + QA Scenarios.


- [ ] 1. Expand config schema: `ui`, `logging`, `keybindings` (+ provider `api_mode`) + update example + docs

  **What to do**:
  - Extend `HarnessConfig` in `crates/harness-core/src/config.rs` to add **optional** sections (all `#[serde(default)]` so existing configs keep working):
    - `ui: UiConfig`
    - `logging: LoggingConfig`
    - `keybindings: KeybindingsConfig`
  - Add `UiConfig` (MVP fields, keep small/fixed):
    - `theme` (string enum via serde): `"mono" | "opencode_dark" | "default"` (default: `mono`)
    - `layout`: `{ activity_width_pct: u8=25, inspector_width_pct: u8=25, input_height_rows: u16=3 }`
    - `default_profile`: string (default: `deep`) — which profile/category to spawn for interactive mode
    - `max_events_in_memory`: usize (default: 25_000)
    - `max_transcript_chars_in_memory`: usize (default: 200_000)
    - `disable_animations`: bool (default: false; still overridden by `HARNESS_DISABLE_ANIMATIONS=1`)
  - Add `LoggingConfig`:
    - `level`: `"error"|"warn"|"info"|"debug"|"trace"` (default: `info`)
    - `file`: optional string/path (default: null; when null, log to `<run_dir>/artifacts/harness.log`)
    - `redact`: bool (default: true)
  - Add `KeybindingsConfig` (fixed action set → key string), as `BTreeMap<String,String>`:
    - Actions (exact strings): `quit`, `focus_next`, `focus_prev`, `palette`, `help`, `toggle_follow`, `submit_prompt`, `clear_prompt`, `scroll_up`, `scroll_down`, `tab_run`, `tab_events`, `tab_diff`
    - Key string format (exact): `q`, `Tab`, `Shift+Tab`, `Ctrl+P`, `?`, `Enter`, `Esc`, `Up`, `Down`, `PageUp`, `PageDown`
  - Extend provider config schema to support Responses API selection:
    - In `OpenAiCompatibleProviderConfig` (`crates/harness-core/src/config.rs:87-99`), add `api_mode` (alias `apiMode`) enum:
      - `responses` | `chat_completions` | `auto`
      - **Default**: `chat_completions` (backward compatible)
  - Update `configs/harness.example.jsonc` to include:
    - provider `api_mode: "responses"` for CLIproxyAPI
    - new `ui`, `logging`, and minimal `keybindings` examples
  - Update `docs/config.md` to document these new sections + `api_mode`.

  **Must NOT do**:
  - Do not add a large/fully generic keybinding system; only the fixed action set above.
  - Do not introduce multiple config versions or migrations in this iteration.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: schema/types + docs + example config updates
  - Skills: [`rust-best-practices`] — keep serde/schemars idiomatic
  - Omitted: [`git-master`] — not required unless batching commits

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 2,3,4,11,12,18 | Blocked By: none

  **References**:
  - Existing config root: `crates/harness-core/src/config.rs:31-42`
  - Provider config struct: `crates/harness-core/src/config.rs:87-99`
  - Example config: `configs/harness.example.jsonc:1-79`
  - Current config docs: `docs/config.md:1-262`
  - Inspiration (schema ideas, not code):
    - OpenCode config schema: https://github.com/anomalyco/opencode/blob/98c75be7e1ab72c48985be033862d96209d4069b/packages/opencode/src/config/config.ts#L579-L931
    - Oh My Pi settings schema: https://github.com/can1357/oh-my-pi/blob/724c1f06f7444de5b3e42c90ccc8f1290a68fa95/packages/coding-agent/src/config/settings-schema.ts#L139-L1354

  **Acceptance Criteria**:
  - [ ] `cargo run -p harness -- schema > /tmp/harness.schema.json` succeeds
  - [ ] `/tmp/harness.schema.json` contains schema entries for `ui`, `logging`, `keybindings`, and provider `api_mode`
  - [ ] `cargo run -p harness -- config validate --config configs/harness.example.jsonc` exits 0

  **QA Scenarios**:
  ```
  Scenario: Schema contains new sections
    Tool: Bash
    Steps:
      - cargo run -p harness -- schema > /tmp/harness.schema.json
      - ( rg '"ui"' /tmp/harness.schema.json && rg '"logging"' /tmp/harness.schema.json && rg '"keybindings"' /tmp/harness.schema.json && rg 'api_mode|apiMode' /tmp/harness.schema.json ) |& tee .sisyphus/evidence/task-1-schema.txt
    Expected: all rg commands find matches
    Evidence: .sisyphus/evidence/task-1-schema.txt

  Scenario: Example config validates
    Tool: Bash
    Steps:
      - cargo run -p harness -- config validate --config configs/harness.example.jsonc |& tee .sisyphus/evidence/task-1-config-validate.txt
    Expected: exits 0 and prints "config valid:"
    Evidence: .sisyphus/evidence/task-1-config-validate.txt
  ```

  **Commit**: YES | Message: `feat(config): add ui/logging/keybindings and provider api_mode` | Files: `crates/harness-core/src/config.rs`, `configs/harness.example.jsonc`, `docs/config.md`


- [ ] 2. Fix env-substitution semantics (`${VAR}` + `${VAR:-default}`) + align docs/tests

  **What to do**:
  - Implement env substitution as documented in `docs/config.md:171-188`, by updating `resolve_env_reference` in `crates/harness-core/src/config.rs:203-217`:
    - `${VAR}` → **required**; if missing, return `ConfigError::MissingEnvironmentVariable(VAR)`.
    - `${VAR:-default}` → optional; if missing, substitute `default`.
    - Any other string → unchanged.
  - Apply to **all** provider secret-like strings that currently use env substitution:
    - `OpenAiCompatibleProviderConfig.api_key` (currently substituted in `HarnessConfig::apply_env_substitutions`: `config.rs:51-60`).
  - Update `configs/harness.example.jsonc` so it remains valid in environments without `OPENAI_API_KEY`:
    - Set `api_key: "${OPENAI_API_KEY:-DUMMY}"` and add a comment that live runs require a real key.
  - Add unit tests in `crates/harness-core/src/config.rs` (`#[cfg(test)]`) verifying:
    - `${MISSING_VAR}` fails deterministically with `MissingEnvironmentVariable`.
    - `${MISSING_VAR:-fallback}` resolves to `fallback`.
    - `${PATH}` resolves when set.
  - Update `docs/config.md` to match the exact behavior (required vs fallback).

  **Must NOT do**:
  - Do not make env substitution “best effort”; the goal is deterministic validation.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: parsing edge cases + test determinism
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 4,16 | Blocked By: 1

  **References**:
  - Current resolver: `crates/harness-core/src/config.rs:203-217`
  - Docs claim: `docs/config.md:171-188`
  - Existing (unused) error variant: `crates/harness-core/src/config.rs:25-27`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core config |& tee .sisyphus/evidence/task-2-config-tests.txt` exits 0
  - [ ] `cargo run -p harness -- config validate --config configs/harness.example.jsonc` exits 0 even when `OPENAI_API_KEY` is unset

  **QA Scenarios**:
  ```
  Scenario: Missing ${VAR} fails; ${VAR:-default} works
    Tool: Bash
    Steps:
      - cargo test -p harness-core config |& tee .sisyphus/evidence/task-2-config-tests.txt
    Expected: tests pass and cover both required + default env patterns
    Evidence: .sisyphus/evidence/task-2-config-tests.txt
  ```

  **Commit**: YES | Message: `feat(config): implement strict env substitution syntax` | Files: `crates/harness-core/src/config.rs`, `docs/config.md`, `configs/harness.example.jsonc`


- [ ] 3. Implement CLIproxyAPI **Responses API** streaming in provider layer + offline tests + gated live smoke

  **What to do**:
  - Extend `crates/harness-providers/src/openai.rs` (current chat-completions-only provider) to support OpenAI **Responses API** streaming:
    1) Add `OpenAiApiMode` enum: `responses | chat_completions | auto`.
    2) Add `api_mode` to provider config (`OpenAiCompatibleProviderConfig`) and store it on the provider.
    3) Implement endpoint builders:
       - chat: `{base_url}/chat/completions` (existing: `openai.rs:80-83`)
       - responses: `{base_url}/responses`
    4) Implement request translation for Responses API (per official docs):
       - POST `/v1/responses` JSON:
         - `model`: `CompletionRequest.model_id`
         - `input`: array of `{ role, content }` objects derived from `CompletionRequest.messages`
         - `stream`: `true`
       - Source: https://developers.openai.com/api/docs/guides/streaming-responses#enable-streaming
    5) Implement SSE consumption for Responses streaming events:
       - Ignore keep-alive comments.
       - Parse JSON `data` into `{ type, delta, ... }` shape.
       - Emit `ProviderStreamEvent::TextDelta(delta)` when:
         - SSE event name is `response.output_text.delta`, OR
         - JSON contains `{"type":"response.output_text.delta","delta":"..."}`.
       - Emit `Done` when:
         - event is `response.completed`, OR JSON `type` is `response.completed`.
       - Emit `Error` when:
         - event/type is `error` (extract a safe message; do not leak headers/api key).
    6) `api_mode=auto`: try `/responses` first; if non-2xx status is 404/405, fall back to chat completions.
  - Add **offline deterministic** tests for Responses SSE parsing:
    - Add a transcript fixture containing at minimum: `response.created`, `response.output_text.delta` (multiple), `response.completed`, plus keep-alive comments.
    - Assert output events are `Start → TextDelta+ → Done`.
  - Add a **gated live smoke** test (ignored) similar to existing `openai_compatible_live_proxy_config_file_smoke` (`openai.rs:415-479`), but exercising Responses mode:
    - Require `HARNESS_LIVE_PROXY=1`.
    - Use config providers map to find base_url/api_key.
    - Assert we see Start, at least one TextDelta, then Done.

  **Must NOT do**:
  - Do not add tool-call streaming support in this iteration (out of scope).
  - Do not log raw HTTP headers/bodies.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: streaming protocol parsing + robust fallbacks + security
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 4,15,16,17 | Blocked By: 1

  **References**:
  - Current chat endpoint + SSE loop: `crates/harness-providers/src/openai.rs:80-211`
  - Existing live proxy smoke (chat): `crates/harness-providers/src/openai.rs:415-479`
  - OpenAI Responses streaming request format + event types:
    - https://developers.openai.com/api/docs/guides/streaming-responses
  - CLIproxyAPI routes include `/v1/responses`:
    - https://github.com/router-for-me/CLIProxyAPI/blob/09fec34e1cdfd99ac79be458fff29f94b834dbcc/internal/api/server.go#L320-L333

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-providers openai |& tee .sisyphus/evidence/task-3-provider-tests.txt` exits 0
  - [ ] `HARNESS_LIVE_PROXY=1 cargo test -p harness-providers -- --ignored |& tee .sisyphus/evidence/task-3-live-smoke.txt` passes when CLIproxyAPI is running locally

  **QA Scenarios**:
  ```
  Scenario: Offline Responses SSE parsing
    Tool: Bash
    Steps:
      - cargo test -p harness-providers openai |& tee .sisyphus/evidence/task-3-provider-tests.txt
    Expected: tests include a Responses-mode fixture asserting TextDelta extraction and Done
    Evidence: .sisyphus/evidence/task-3-provider-tests.txt

  Scenario: Live CLIproxyAPI Responses smoke (gated)
    Tool: Bash
    Steps:
      - HARNESS_LIVE_PROXY=1 cargo test -p harness-providers -- --ignored |& tee .sisyphus/evidence/task-3-live-smoke.txt
    Expected: passes only when CLIproxyAPI reachable; otherwise skipped
    Evidence: .sisyphus/evidence/task-3-live-smoke.txt
  ```

  **Commit**: YES | Message: `feat(provider): add responses api streaming support` | Files: `crates/harness-providers/src/openai.rs`, `crates/harness-providers/src/lib.rs` (if needed), provider tests


- [ ] 4. Config → runtime bootstrap: build CoordinatorConfig/provider from HarnessConfig (interactive mode)

  **What to do**:
  - Add a single “bootstrap” module in the CLI crate to centralize config→runtime wiring:
    - New file: `crates/harness/src/bootstrap.rs`
    - Public API (exact):
      - `pub fn load_harness_config(path: &Path) -> Result<HarnessConfig, String>`
      - `pub fn build_interactive_coordinator_config(cfg: &HarnessConfig) -> Result<CoordinatorConfig, String>`
      - `pub fn interactive_profile_name(cfg: &HarnessConfig) -> String` (uses `cfg.ui.default_profile`)
  - Implement `build_interactive_coordinator_config` mapping:
    - `CoordinatorConfig.session_dir` ← `cfg.paths.session_dir` (`crates/harness-core/src/config.rs:173-185`)
    - `CoordinatorConfig.permission_policy` ← `PermissionPolicy::from_config(cfg)` (`crates/harness-core/src/perm.rs:55-78`)
    - `CoordinatorConfig.tool_registry` ← `harness_tools::coordinator_registry(cfg.permissions.shell_allowlist.clone())`
    - `CoordinatorConfig.tool_concurrency` ← `cfg.background_task.default_concurrency`
    - `CoordinatorConfig.provider_model_concurrency` ← `cfg.background_task.model_concurrency`
    - `CoordinatorConfig.stale_timeout_ms` ← `cfg.background_task.stale_timeout_ms`
    - `CoordinatorConfig.provider` ← instantiate from `cfg.providers["default"]`:
      - enforce: provider name must exist; else return error
      - enforce: provider type must be `openai_compatible` (for this iteration)
      - pass `api_mode` through to provider builder (Task 3)
    - `CoordinatorConfig.agent_profiles` ← derive from `cfg.categories`:
      - For each `(category_name, category_cfg)`:
        - Create an `AgentProfile` keyed by `category_name` with:
          - `name = category_name`
          - `category = category_name`
          - `model_ref = category_cfg.model_ref`
          - `system_prompt = format!("You are the {category_name} agent. {}", category_cfg.description)`
          - `toolset = category_cfg.tools.clone()`
      - Enforce MVP constraint: `AgentModelRef::parse(model_ref).provider_id == "default"` for all derived profiles (single-provider MVP).
  - Make `logging` config actually do something (minimal, file-based):
    - Add deps to `crates/harness/Cargo.toml`: `tracing`, `tracing-subscriber`, `tracing-appender`.
    - New module: `crates/harness/src/logging.rs` with:
      - `pub fn init_logging(cfg: &HarnessConfig, artifacts_dir: &Path) -> Result<(), String>`
      - Output file:
        - if `cfg.logging.file` is set → use it
        - else → `${run.artifacts_dir}/harness.log`
      - Level from `cfg.logging.level`.
    - Call `init_logging(...)` from interactive TUI and headless prompt after `start_run` returns `RunInfo`.
  - Ensure scenario mode remains unchanged (still uses `golden_path_provider()` / `golden_path_profiles()` for deterministic scenarios).

  **Must NOT do**:
  - Do not introduce a multi-provider router in this iteration. Fail fast if provider_id != `default`.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: cross-crate wiring + validation
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 9,10,16 | Blocked By: 1,3,2

  **References**:
  - CLI config loading currently only uses session_dir/shell_allowlist: `crates/harness/src/tui.rs:142-182`, `crates/harness/src/run.rs:104-144`
  - Coordinator config fields: `crates/harness-core/src/coord.rs:50-66`
  - Permission policy from config: `crates/harness-core/src/perm.rs:55-78`
  - Config categories: `crates/harness-core/src/config.rs:123-136`

  **Acceptance Criteria**:
  - [ ] `cargo build --workspace` succeeds
  - [ ] Running interactive TUI with a config that defines `default` provider no longer uses `golden_path_provider()` code path
    - Evidence via logs/events: ProviderRequestStarted shows `provider_id="default"` and model_id matches config

  **QA Scenarios**:
  ```
  Scenario: Interactive config wiring uses config provider
    Tool: Bash
    Steps:
      - rg 'build_interactive_coordinator_config' -n crates/harness/src |& tee .sisyphus/evidence/task-4-bootstrap-grep.txt
      - rg 'golden_path_provider\(\)' -n crates/harness/src/tui.rs |& tee -a .sisyphus/evidence/task-4-bootstrap-grep.txt
    Expected: interactive path uses bootstrap builder; scenario path may still reference golden_path_* only under --scenario
    Evidence: .sisyphus/evidence/task-4-bootstrap-grep.txt
  ```

  **Commit**: YES | Message: `feat(cli): bootstrap coordinator config from harness config` | Files: `crates/harness/src/bootstrap.rs`, `crates/harness/src/tui.rs`, (maybe) `crates/harness/src/main.rs`


- [ ] 5. Live-update backpressure: bounded channels + delta coalescing to keep TUI responsive

  **What to do**:
  - Add deps:
    - `crossbeam-channel` to `crates/harness/Cargo.toml` and `crates/harness-tui/Cargo.toml`.
  - Replace unbounded live-update channel between `harness` and `harness-tui` with a bounded channel and an explicit overload policy:
    - Current unbounded: `std_mpsc::channel::<LiveUpdate>()` in `crates/harness/src/tui.rs:233-235`.
  - Decision locked (implementation): use `crossbeam-channel` bounded channel:
    - `crossbeam_channel::bounded::<LiveUpdate>(2048)`
    - In sender: `try_send`.
    - If full:
      - drop **ProviderStreamDelta** updates first (coalesce),
      - send at most 1 status banner per second: `LiveUpdate::Status("UI overloaded: dropped N deltas")`.
  - Add ProviderStreamDelta coalescing in the forwarder loop (`forward_events_to_tui` in `crates/harness/src/tui.rs:378+`):
    - buffer deltas by `request_id` for up to 16ms or 1024 chars, then send a single synthetic `ProviderStreamDelta` event with concatenated delta.
    - Do **not** coalesce non-delta events.
  - Update `crates/harness-tui/src/lib.rs` to accept `crossbeam_channel::Receiver<LiveUpdate>` and drain with `try_recv`.

  **Must NOT do**:
  - Do not block the coordinator/event forwarder indefinitely on UI slowness.
  - Do not change persisted event logs; this is display-path only.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: concurrency/backpressure correctness
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 1 | Blocks: 8,11,15 | Blocked By: none

  **References**:
  - Unbounded channel creation: `crates/harness/src/tui.rs:233-235`
  - TUI draining loop: `crates/harness-tui/src/lib.rs:64-66,138-149`
  - Provider delta event type in UI: `crates/harness-tui/src/ui.rs:116-119`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui |& tee .sisyphus/evidence/task-5-tui-tests.txt` exits 0
  - [ ] PTY E2E (after Task 15) shows no hangs while streaming a long response (wiremock)

  **QA Scenarios**:
  ```
  Scenario: TUI crate tests still pass after channel refactor
    Tool: Bash
    Steps:
      - cargo test -p harness-tui |& tee .sisyphus/evidence/task-5-tui-tests.txt
    Expected: exits 0
    Evidence: .sisyphus/evidence/task-5-tui-tests.txt
  ```

  **Commit**: YES | Message: `perf(tui): bound live updates and coalesce provider deltas` | Files: `crates/harness/src/tui.rs`, `crates/harness-tui/src/lib.rs`


- [ ] 6. Event schema: add persisted user prompt event (replayable transcript input)

  **What to do**:
  - Extend event schema with a first-class prompt/user message event:
    - In `crates/harness-core/src/event.rs`, add:
      - `EventV1::UserMessageSubmitted(UserMessageSubmittedEvent)`
      - `pub struct UserMessageSubmittedEvent { message_id: String, agent_id: String, content: String, content_digest: String }`
    - Digest rule: `content_digest = blake3(content_bytes).to_hex().take(12)` (match existing `digest12` convention in `agent.rs:209-211`).
  - Update any exhaustive match sites:
    - `crates/harness-tui/src/ui.rs:event_variant_name` (currently enumerates all variants at `ui.rs:105-131`).
    - `crates/harness-core/src/proj.rs` event naming (if present).
  - Add tests:
    - `harness-core` unit test: JSONL serde roundtrip for `UserMessageSubmitted`.
    - Redaction test: ensure content gets redacted if it contains `sk-...` (use existing redactor behavior).

  **Must NOT do**:
  - Do not store prompts only as summaries/digests; interactive replay must preserve actual user input.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: schema evolution + projection compatibility
  - Skills: [`rust-best-practices`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 7,8,11,15 | Blocked By: none

  **References**:
  - Event enum list: `crates/harness-core/src/event.rs:78-105`
  - Existing UI intent event (not sufficient): `crates/harness-core/src/event.rs:298-303`
  - Variant naming in UI: `crates/harness-tui/src/ui.rs:105-131`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core event |& tee .sisyphus/evidence/task-6-event-tests.txt` exits 0

  **QA Scenarios**:
  ```
  Scenario: Event serde + redaction for user prompt
    Tool: Bash
    Steps:
      - cargo test -p harness-core event |& tee .sisyphus/evidence/task-6-event-tests.txt
    Expected: exits 0
    Evidence: .sisyphus/evidence/task-6-event-tests.txt
  ```

  **Commit**: YES | Message: `feat(core): persist user prompts as events` | Files: `crates/harness-core/src/event.rs`, `crates/harness-tui/src/ui.rs`, `crates/harness-core/src/proj.rs` (if needed)


- [ ] 7. Coordinator control plane: spawn idle agent + request agent turn (prompt-driven)

  **What to do**:
  - Add an “idle agent spawn” path so interactive mode can create an agent without automatically scheduling a turn.
    - Keep existing `spawn_agent()` semantics unchanged (tests rely on auto-schedule-on-spawn).
    - Add new coordinator command + handle method:
      - `CoordinatorHandle::spawn_agent_idle(actor, profile, parent_agent_id) -> Result<String, CoordinatorError>`
      - New `Command::SpawnAgentIdle { actor, profile, parent_agent_id, respond_to }`
      - Internal logic mirrors `spawn_agent_internal` up through `AgentSpawned`, but **does not** call `schedule_agent_turn`.
  - Add a prompt-driven turn scheduling API:
    - `CoordinatorHandle::request_agent_turn(actor, agent_id, prompt: String) -> Result<String, CoordinatorError>` returning `request_id`.
    - New `Command::RequestAgentTurn { actor, agent_id, prompt, respond_to }`.
    - Internal logic:
      1) Allocate `request_id = format!("req_{:06}", next_provider_request_id)`.
      2) Append `UserMessageSubmitted` event (Task 6) with:
         - `message_id = format!("msg_{:06}", next_message_id)` (add `next_message_id` counter to `RunState`).
         - `agent_id` and `content = prompt`.
         - `correlation_id = Some(request_id.clone())` and `stream_key = Some(format!("agent:{agent_id}"))`.
      3) Look up the agent’s profile name and profile config:
         - Add to `RunState`: `agent_profile_names: BTreeMap<String, String>`.
         - Populate it in both spawn paths.
      4) Build `AgentRequest { agent_id, prompt, model_ref: profile_cfg.model_ref.clone() }`.
      5) Call `schedule_agent_turn(...)` using existing provider/scheduler.
  - Authorization policy (decision locked):
    - `RequestAgentTurn` accepts `ActorKind::User` and `ActorKind::Supervisor`.
    - Reject `ActorKind::Worker` with `PolicyViolationDetected` event.
  - Add/extend tests in `crates/harness-core/tests/coord.rs`:
    - `coord_spawn_agent_idle_appends_agent_spawned_but_no_provider_events`
    - `coord_request_agent_turn_appends_user_message_and_provider_events`
    - Include assertion that provider events’ `correlation_id == request_id` (like existing test at `coord.rs:224-243`).

  **Must NOT do**:
  - Do not change existing `spawn_agent` behavior (it’s already covered by concurrency tests: `coord.rs:143-182`).
  - Do not add multi-turn conversation history (explicitly out of scope for this iteration).

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: coordinator API + invariants + concurrency interactions
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 8,9,10,15 | Blocked By: 6

  **References**:
  - Current spawn agent schedules an agent turn: `crates/harness-core/src/coord.rs:781-812`
  - Agent turn scheduling helper: `crates/harness-core/src/coord.rs:1754-1840`
  - Existing coordinator tests relying on spawn auto-run: `crates/harness-core/tests/coord.rs:143-243`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-core coord |& tee .sisyphus/evidence/task-7-coord-tests.txt` exits 0
  - [ ] New tests exist and pass for idle spawn + request turn

  **QA Scenarios**:
  ```
  Scenario: Coordinator supports prompt-driven turns
    Tool: Bash
    Steps:
      - cargo test -p harness-core coord |& tee .sisyphus/evidence/task-7-coord-tests.txt
    Expected: exits 0; new tests cover SpawnAgentIdle + RequestAgentTurn
    Evidence: .sisyphus/evidence/task-7-coord-tests.txt
  ```

  **Commit**: YES | Message: `feat(core): add idle spawn + prompt-driven agent turns` | Files: `crates/harness-core/src/coord.rs`, `crates/harness-core/src/event.rs`, `crates/harness-core/tests/coord.rs`


- [ ] 8. TUI interactive input + UiIntent plumbing (prompt submit → coordinator)

  **What to do**:
  - Generalize TUI → harness callbacks beyond permissions:
    - Replace `PermissionIntent` with `UiIntent` enum in `crates/harness-tui/src/app.rs`:
      - `ResolvePermission { permission_id, decision }`
      - `SubmitPrompt { text }`
      - `QuitRequested`
    - Replace `TuiOptions.on_permission_intent` with `TuiOptions.on_ui_intent` in `crates/harness-tui/src/lib.rs:35-39`.
  - Add prompt input state to `AppState` (`crates/harness-tui/src/app.rs:37-56`):
    - `prompt_buffer: String`, `prompt_cursor: usize`
    - `prompt_history: Vec<String>`, `prompt_history_index: Option<usize>`
    - Add focus target for prompt input (extend Focus enum or introduce `PaneFocus`).
  - Key handling rules (decision locked):
    - Global: `q` emits `QuitRequested`.
    - Focus cycling: `Tab` next focus, `Shift+Tab` prev.
    - Prompt focus:
      - `Enter` submits prompt if non-empty → emit `SubmitPrompt { text }` and clear buffer.
      - `Esc` clears buffer.
      - `Up/Down` browse history.
      - Printable chars insert; Backspace deletes.
  - Render minimal Run workspace scaffolding:
    - Add/rename a tab or view to include a prompt box + a transcript area that at least shows:
      - last `UserMessageSubmitted` content
      - streaming ProviderStreamDelta text
    - Full 3-pane polish is Task 11.
  - Update existing harness-tui tests that assert permission modal intents:
    - `permission_modal_a_emits_resolve_intent...` in `crates/harness-tui/src/lib.rs:256+` must now assert `UiIntent::ResolvePermission`.
    - Add a new unit test for `Enter` in prompt focus emitting `UiIntent::SubmitPrompt`.

  **Must NOT do**:
  - Do not implement multi-line prompt editing or search yet.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: TUI state/input wiring + render updates
  - Skills: [`terminal-ui-design`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 9,11,12,15 | Blocked By: 5,6,7

  **References**:
  - Current AppState key handling: `crates/harness-tui/src/app.rs:176-214`
  - Permission modal intent plumbing: `crates/harness-tui/src/app.rs:232-260`
  - TUI run loop + options: `crates/harness-tui/src/lib.rs:41-107`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui |& tee .sisyphus/evidence/task-8-harness-tui-tests.txt` exits 0
  - [ ] Snapshot tests updated/added for prompt box rendering

  **QA Scenarios**:
  ```
  Scenario: TUI emits SubmitPrompt intent
    Tool: Bash
    Steps:
      - cargo test -p harness-tui |& tee .sisyphus/evidence/task-8-harness-tui-tests.txt
    Expected: exits 0; new tests cover Enter → SubmitPrompt
    Evidence: .sisyphus/evidence/task-8-harness-tui-tests.txt
  ```

  **Commit**: YES | Message: `feat(tui): add prompt input and ui intent plumbing` | Files: `crates/harness-tui/src/{app.rs,lib.rs,ui.rs}`


- [ ] 9. `harness tui` interactive mode: default run workspace (keep replay + scenario modes)

  **What to do**:
  - Update CLI semantics in `crates/harness/src/tui.rs`:
    - Allow **no flags** → interactive mode.
    - Keep `--replay <run_dir>` (replay mode).
    - Keep `--scenario <name>` (scenario mode).
    - Add `--profile <name>` to select which derived profile/category to use for the interactive agent (default: `cfg.ui.default_profile`).
  - Implement interactive mode wiring:
    1) Resolve config path and load config (bootstrap from Task 4).
    2) Build coordinator config from harness config.
    3) Start run with name `interactive` and workspace root = current directory.
    3.1) Call `init_logging(&cfg, &run.artifacts_dir)` (Task 4).
    4) Spawn **idle** agent with selected profile.
    5) Start TUI in live mode.
    6) Handle UiIntents:
       - `ResolvePermission` → existing path (`handle_ui_intents`)
       - `SubmitPrompt {text}` → call `coordinator.request_agent_turn(user_actor, agent_id, text)`
       - `QuitRequested` → stop run and exit
  - Preserve scenario mode path (existing golden-path runner) and replay mode.

  **Must NOT do**:
  - Do not require scenario for `harness tui` anymore.
  - Do not break `harness tui --replay <run_dir>`.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: CLI orchestration + async task wiring + lifecycle correctness
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: NO | Wave 2 | Blocks: 11-16 | Blocked By: 4,7,8

  **References**:
  - Current tui arg group (requires replay/scenario): `crates/harness/src/tui.rs:35-57`
  - Current live mode bootstrapping: `crates/harness/src/tui.rs:184-304`
  - Permission intent handling: `crates/harness/src/tui.rs:251-260` + `handle_ui_intents(...)` further down

  **Acceptance Criteria**:
  - [ ] `cargo run -p harness -- tui --config configs/harness.example.jsonc` launches TUI and shows a prompt input box
  - [ ] Submitting a prompt triggers ProviderRequestStarted/ProviderStreamDelta events and transcript updates

  **QA Scenarios**:
  ```
  Scenario: Interactive TUI prompt sends provider request
    Tool: pty-mcp
    Steps:
      - Spawn: cargo run -p harness -- tui --config configs/harness.example.jsonc
      - Type: "Say hello" + Enter
      - Wait: transcript shows streamed output and activity shows ProviderRequestStarted
      - Quit: q
    Expected: no panic; provider stream visible; run stops cleanly
    Evidence: .sisyphus/evidence/task-9-pty-interactive.png
  ```

  **Commit**: YES | Message: `feat(cli): interactive tui mode with prompt submission` | Files: `crates/harness/src/{tui.rs,bootstrap.rs}`, `crates/harness-tui/src/*`


- [ ] 10. Headless prompt command (non-TUI) for smoke + automation

  **What to do**:
  - Add a new CLI subcommand to run a single prompt headlessly using config provider:
    - `harness prompt --text "..." [--profile deep] [--out <events.jsonl>] [--print-run-dir]`
  - Implementation plan:
    - Add module `crates/harness/src/prompt.rs`.
    - Update `crates/harness/src/main.rs` to include `Prompt(PromptCommand)`.
  - In `prompt::execute`:
    1) Load config + build coordinator config (Task 4)
    2) Start run name `prompt`
    2.1) Call `init_logging(&cfg, &run.artifacts_dir)` (Task 4).
    3) Spawn idle agent
      4) Call `request_agent_turn` with the provided text
      5) Wait for `ProviderRequestFinished` or `TaskCompleted` for the request_id by subscribing to the coordinator event store
      6) Stop run
      7) Optionally write `--out` by copying run events.jsonl (reuse `copy_events_file` helpers from `run.rs`)
  - Add an integration test that uses wiremock base_url (local) to ensure the CLI hits `/responses`.
    - Add `wiremock` as a **dev-dependency** for `crates/harness`.

  **Must NOT do**:
  - Do not require TUI for basic provider verification.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: CLI feature + event-store subscription + deterministic exit
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 2 | Blocks: 16 | Blocked By: 4,7

  **References**:
  - CLI structure: `crates/harness/src/main.rs:33-47`
  - Event store subscribe used by TUI: `crates/harness/src/tui.rs:378-420`
  - Existing file copy helper in run.rs: `crates/harness/src/run.rs:79-83` (and later)

  **Acceptance Criteria**:
  - [ ] `cargo run -p harness -- prompt --text "Hello" --config configs/harness.example.jsonc` exits 0
  - [ ] Events.jsonl includes `UserMessageSubmitted` and provider stream events

  **QA Scenarios**:
  ```
  Scenario: Headless prompt run produces provider events
    Tool: Bash
    Steps:
      - cargo run -p harness -- prompt --text "Hello" --config configs/harness.example.jsonc --print-run-dir |& tee .sisyphus/evidence/task-10-prompt.txt
    Expected: exits 0; output includes run dir; events.jsonl contains ProviderRequestStarted
    Evidence: .sisyphus/evidence/task-10-prompt.txt
  ```

  **Commit**: YES | Message: `feat(cli): add headless prompt subcommand` | Files: `crates/harness/src/{main.rs,prompt.rs,bootstrap.rs}`


- [ ] 11. TUI UX overhaul: Run workspace 3-pane layout + status + inspector (OpenCode/Pi-inspired)

  **What to do**:
  - Implement the final interactive “Run workspace” layout in `crates/harness-tui/src/ui.rs`:
    - Header (1 row): `Agent Harness | run=<run_id> | profile=<profile> | provider=default | model=<model>`
    - Main panes (horizontal split, percentages from config):
      - Left: **Activity** list
      - Center: **Transcript**
      - Right: **Inspector**
    - Footer (1 row): status + key hints (e.g., `Tab focus | Ctrl+P palette | q quit`)
    - Prompt input (height from config, default 3 rows) with title **Prompt** and a character count.
  - Decision locked (pane titles / PTY markers): the strings `Activity`, `Transcript`, `Inspector`, and `Prompt` must appear verbatim in the UI so PTY E2E can anchor.
  - Theme system (minimal, internal):
    - New file: `crates/harness-tui/src/theme.rs`
    - Define two palettes:
      - `mono` (Pi-like: neutral gray foreground, dark bg, single accent)
      - `opencode_dark` (higher-contrast accent colors)
    - Apply theme consistently to:
      - selected list row
      - borders/titles
      - status banner
      - permission modal
  - Ensure existing tabs still exist but default to Run view:
    - Tabs become: `Run`, `Events`, `Diff`, `Help`.

  **Must NOT do**:
  - No “cute” ASCII art; keep it clean and utilitarian.
  - No animations by default in deterministic mode.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: layout, typography (spacing), colors
  - Skills: [`terminal-ui-design`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 14,15,18 | Blocked By: 1,8,9

  **References**:
  - Current render entrypoint: `crates/harness-tui/src/ui.rs:15-50`
  - Current tabs widget: `crates/harness-tui/src/ui.rs:79-102`
  - Pi Agent Rust layout inspiration (do not copy code): https://github.com/Dicklesworthstone/pi_agent_rust/blob/4390667e3a878cb62238ec27b95c110c4d5eac37/docs/tui.md#L6-L31
  - OpenCode session view inspiration (behavior only): https://github.com/anomalyco/opencode/blob/98c75be7e1ab72c48985be033862d96209d4069b/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui |& tee .sisyphus/evidence/task-11-tui-tests.txt` exits 0
  - [ ] Run workspace renders the four pane titles (Activity/Transcript/Inspector/Prompt)

  **QA Scenarios**:
  ```
  Scenario: Run workspace renders expected panes
    Tool: Bash
    Steps:
      - cargo test -p harness-tui |& tee .sisyphus/evidence/task-11-tui-tests.txt
    Expected: exits 0; snapshots/strings include Activity/Transcript/Inspector/Prompt
    Evidence: .sisyphus/evidence/task-11-tui-tests.txt
  ```

  **Commit**: YES | Message: `feat(tui): run workspace layout and theming` | Files: `crates/harness-tui/src/{ui.rs,theme.rs,app.rs}`


- [ ] 12. Command palette + Help overlay + configurable keybindings (fixed action set)

  **What to do**:
  - Implement command palette overlay (OpenCode-like) in AppState + UI:
    - Toggle key: default `Ctrl+P` (configurable).
    - Palette supports fuzzy-ish prefix match over commands.
    - Commands (exact MVP set):
      - `help` (open Help)
      - `run` (switch to Run tab)
      - `events` (switch to Events tab)
      - `diff` (switch to Diff tab)
      - `toggle_follow`
      - `quit`
  - Implement `keybindings` overrides:
    - Parse `cfg.keybindings` in CLI (`crates/harness/src/tui.rs`) and pass into TUI via `TuiOptions`.
    - In `harness-tui`, define an `Action` enum matching Task 1’s action names.
    - Map `KeyEvent` → `Action` using the keymap, falling back to defaults.
  - Update Help tab/overlay to render:
    - current keybindings (resolved after overrides)
    - palette usage

  **Must NOT do**:
  - Do not implement arbitrary macro recording or leader-key chains.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: overlays + UX flow
  - Skills: [`terminal-ui-design`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 14,15 | Blocked By: 1,11

  **References**:
  - OpenCode leader/palette inspiration: https://github.com/anomalyco/opencode/blob/98c75be7e1ab72c48985be033862d96209d4069b/packages/web/src/content/docs/tui.mdx#L60-L67
  - Current help tab text: `crates/harness-tui/src/ui.rs:185-205`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui |& tee .sisyphus/evidence/task-12-palette-tests.txt` exits 0
  - [ ] New snapshot(s) cover palette open state and help overlay shows resolved keys

  **QA Scenarios**:
  ```
  Scenario: Palette opens and executes quit
    Tool: pty-mcp
    Steps:
      - Spawn: cargo run -p harness -- tui --config configs/harness.example.jsonc
      - Press: Ctrl+P
      - Type: quit + Enter
    Expected: TUI exits cleanly
    Evidence: .sisyphus/evidence/task-12-palette-quit.png
  ```

  **Commit**: YES | Message: `feat(tui): command palette and configurable keybindings` | Files: `crates/harness-tui/src/{app.rs,ui.rs,lib.rs}`, `crates/harness/src/tui.rs`


- [ ] 13. Transcript/activity refinements: grouping, error states, memory caps

  **What to do**:
  - Activity list:
    - Group entries by `correlation_id` (request_id) and show a compact row per turn:
      - `req_000123  gpt-5-codex  streaming…` / `done` / `error`
    - Selecting an activity row populates Inspector with:
      - ProviderRequestStarted JSON
      - Any errors
      - (Optional) request digest
  - Transcript:
    - Render user messages from `UserMessageSubmitted` events.
    - Render assistant text by accumulating ProviderStreamDelta for the same request_id.
    - On ProviderRequestFinished, mark the turn as completed.
  - Memory caps:
    - Enforce `cfg.ui.max_events_in_memory` by dropping oldest events and showing a banner `"trimmed N old events"`.
    - Enforce `cfg.ui.max_transcript_chars_in_memory` by trimming oldest transcript content.
  - Error states:
    - Provider errors show as a red banner and a transcript block `"[provider error] ..."`.

  **Must NOT do**:
  - Do not hide errors in logs only; UI must surface them.

  **Recommended Agent Profile**:
  - Category: `visual-engineering` — Reason: interaction details + info architecture
  - Skills: [`terminal-ui-design`]

  **Parallelization**: Can Parallel: YES | Wave 3 | Blocks: 14,15 | Blocked By: 11

  **References**:
  - Provider event payloads: `crates/harness-core/src/event.rs:177-199`
  - Current grouping helper exists: `crates/harness-tui/src/ui.rs` (search `correlation_groups`)

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-tui |& tee .sisyphus/evidence/task-13-transcript-tests.txt` exits 0
  - [ ] New snapshots verify grouped activity rows and transcript rendering

  **QA Scenarios**:
  ```
  Scenario: Streaming transcript appears
    Tool: Bash
    Steps:
      - cargo test -p harness-tui |& tee .sisyphus/evidence/task-13-transcript-tests.txt
    Expected: exits 0; snapshots include a streamed assistant delta in Transcript pane
    Evidence: .sisyphus/evidence/task-13-transcript-tests.txt
  ```

  **Commit**: YES | Message: `feat(tui): transcript/activity grouping and caps` | Files: `crates/harness-tui/src/{app.rs,ui.rs}`


- [ ] 14. Update ratatui snapshot tests for new UI + new fixtures

  **What to do**:
  - Update harness-tui unit/integration snapshots in `crates/harness-tui/src/lib.rs` (see current tests at `lib.rs:168-254`):
    - Replace “two pane layout” expectations with new Run workspace layout.
    - Add a fixture sequence that includes:
      - `RunStarted`
      - `AgentSpawned`
      - `UserMessageSubmitted`
      - `ProviderRequestStarted`
      - `ProviderStreamDelta` (1-2 chunks)
      - `ProviderRequestFinished`
    - Snapshot at least:
      1) Run workspace with prompt box empty
      2) Run workspace with prompt buffer filled
      3) Run workspace with streamed transcript visible
      4) Permission modal overlay still renders (existing test kept/updated)
  - Update snapshot names and ensure `insta` filters still apply (redactions).

  **Must NOT do**:
  - Do not rely on terminal font rendering here; only buffer snapshots.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: test fixture design + deterministic UI snapshots
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 15,17 | Blocked By: 11,12,13

  **References**:
  - Existing snapshot helper: `crates/harness-tui/src/lib.rs:168-185` and `assert_buffer_snapshot` (search in file)

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no cargo test -p harness-tui |& tee .sisyphus/evidence/task-14-tui-snapshots.txt` exits 0

  **QA Scenarios**:
  ```
  Scenario: UI snapshots are stable
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no cargo test -p harness-tui |& tee .sisyphus/evidence/task-14-tui-snapshots.txt
    Expected: exits 0
    Evidence: .sisyphus/evidence/task-14-tui-snapshots.txt
  ```

  **Commit**: YES | Message: `test(tui): update snapshots for run workspace` | Files: `crates/harness-tui/src/lib.rs`, snapshot files


- [ ] 15. Update PTY E2E: interactive prompt submission + streaming + refreshed PNG checkpoints

  **What to do**:
  - Update existing PTY E2E test `pty_e2e_tui_golden_path` (`crates/harness-testkit/tests/pty_e2e.rs:40-168`):
    - Replace the startup marker `"Tabs"` with `"Prompt"`.
    - Update tab-switch keys if needed (Run/Events/Diff).
    - Keep permission approval key as `a`.
    - Update screenshot focus anchors to match the new layout.
  - Add a new PTY E2E test for interactive prompt submission (offline deterministic via wiremock):
    - Add `wiremock` as a **dev-dependency** for `crates/harness-testkit`.
    - New test name: `pty_e2e_tui_interactive_prompt_streams_response`.
    - Start a local wiremock server that serves `POST /v1/responses` with SSE body:
      - includes `response.created`, two `response.output_text.delta` events, then `response.completed`.
    - Generate a temp config JSONC pointing `providers.default.base_url` to wiremock base_url and `api_mode: "responses"`.
    - Spawn `harness tui --config <temp_config>` in PTY.
    - Type `Hello from PTY` + Enter.
    - Assert screen contains `Hello world` (from fixture) and capture a PNG checkpoint.
  - Keep deterministic env vars (already in test): `HARNESS_DETERMINISTIC=1`, `HARNESS_DISABLE_ANIMATIONS=1`, `TZ=UTC`, `LANG/LC_ALL=C.UTF-8`.

  **Must NOT do**:
  - Do not hit external networks in PTY E2E; only local wiremock.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: PTY automation + stability + fixture server
  - Skills: [`rust-async-patterns`]

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 17 | Blocked By: 9,14

  **References**:
  - PTY harness patterns + screenshot checkpoints: `crates/harness-testkit/tests/pty_e2e.rs:88-158`
  - Deterministic env guidance: `docs/testing.md:99-131`

  **Acceptance Criteria**:
  - [ ] `INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e |& tee .sisyphus/evidence/task-15-pty-e2e.txt` exits 0
  - [ ] PNG artifacts exist in `target/pty-visual-artifacts/` (or configured dir)

  **QA Scenarios**:
  ```
  Scenario: PTY E2E passes and emits PNG checkpoints
    Tool: Bash
    Steps:
      - INSTA_UPDATE=no RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e |& tee .sisyphus/evidence/task-15-pty-e2e.txt
      - ls -1 target/pty-visual-artifacts |& tee .sisyphus/evidence/task-15-pty-artifacts.txt
    Expected: tests pass; artifact list includes new interactive prompt checkpoint PNG
    Evidence: .sisyphus/evidence/task-15-pty-e2e.txt
  ```

  **Commit**: YES | Message: `test(e2e): refresh pty snapshots and add interactive prompt test` | Files: `crates/harness-testkit/tests/pty_e2e.rs`, snapshots, (new) fixtures


- [ ] 16. Add gated live CLIproxyAPI E2E (responses) + pty-mcp evidence capture recipe

  **What to do**:
  - Add a gated end-to-end smoke test that exercises the full stack against a locally running CLIproxyAPI:
    - Preferred location: `crates/harness-testkit/tests/live_proxy_e2e.rs` (ignored by default).
    - Gate on `HARNESS_LIVE_PROXY=1`.
    - Use `configs/harness.example.jsonc` (or `HARNESS_LIVE_PROXY_CONFIG`) with `api_mode: "responses"`.
    - Run `harness prompt --text "Say hello"` and assert:
      - exit 0
      - event log contains ProviderRequestStarted + at least one ProviderStreamDelta + ProviderRequestFinished
  - Add an agent-driven pty-mcp evidence capture script (documented procedure) to produce:
    - `.sisyphus/evidence/task-16-live-tui.png` (screenshot after streaming begins)
    - `.sisyphus/evidence/task-16-live-tui-finished.png` (screenshot after completion)
    - `.sisyphus/evidence/task-16-live-log.txt` (command output)

  **Must NOT do**:
  - Do not run live tests in default CI; they must remain ignored + gated.

  **Recommended Agent Profile**:
  - Category: `deep` — Reason: real-world validation + evidence collection
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: 17,18 | Blocked By: 3,9,10

  **References**:
  - Existing gated live provider smoke in provider crate: `crates/harness-providers/src/openai.rs:415-479`
  - CLIproxyAPI docs: https://help.router-for.me/agent-client/codex

  **Acceptance Criteria**:
  - [ ] `HARNESS_LIVE_PROXY=1 cargo test -p harness-testkit live_proxy -- --ignored |& tee .sisyphus/evidence/task-16-live-e2e.txt` passes when CLIproxyAPI running
  - [ ] Evidence screenshots exist from pty-mcp run

  **QA Scenarios**:
  ```
  Scenario: Live proxy E2E (gated)
    Tool: Bash
    Steps:
      - HARNESS_LIVE_PROXY=1 cargo test -p harness-testkit live_proxy -- --ignored |& tee .sisyphus/evidence/task-16-live-e2e.txt
    Expected: passes or skips deterministically
    Evidence: .sisyphus/evidence/task-16-live-e2e.txt

  Scenario: Live interactive TUI evidence capture
    Tool: pty-mcp
    Steps:
      - Spawn: cargo run -p harness -- tui --config configs/harness.example.jsonc
      - Type: "Say hello" + Enter
      - Screenshot: after first delta
      - Screenshot: after completion
      - Quit: q
    Expected: screenshots show prompt + streamed output
    Evidence: .sisyphus/evidence/task-16-live-tui*.png
  ```

  **Commit**: YES | Message: `test(e2e): add gated live cli-proxy smoke` | Files: `crates/harness-testkit/tests/live_proxy_e2e.rs`, docs updates


- [ ] 17. Security regression: scan JSONL/snapshots/artifacts for secrets; ensure provider errors never leak auth

  **What to do**:
  - Add a reusable secret-scan helper in `harness-testkit`:
    - Scan a directory tree for forbidden substrings/regexes:
      - `sk-[A-Za-z0-9]{10,}`
      - `Authorization: Bearer`
      - `Bearer sk-`
    - Run it against:
      - temp session dirs produced by PTY E2E
      - `target/pty-visual-artifacts/`
      - `crates/**/snapshots/`
  - Add a test `secret_scan_does_not_find_api_keys_in_artifacts` that runs after PTY E2E in the same crate (or as a separate `cargo test -p harness-testkit secretscan`).
  - Ensure provider error tests continue to validate non-leak:
    - `openai_compatible_errors_do_not_leak_auth_secrets` (`crates/harness-providers/src/openai.rs:390-413`).

  **Must NOT do**:
  - Do not scan user home directories; only repo-local artifacts and temp dirs.

  **Recommended Agent Profile**:
  - Category: `ultrabrain` — Reason: security regression correctness + test determinism
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: Final Verification | Blocked By: 14,15,16

  **References**:
  - Existing redaction patterns: `crates/harness-core/src/redact.rs:14-29`
  - Existing provider non-leak test: `crates/harness-providers/src/openai.rs:390-413`

  **Acceptance Criteria**:
  - [ ] `cargo test -p harness-testkit secretscan |& tee .sisyphus/evidence/task-17-secretscan.txt` exits 0

  **QA Scenarios**:
  ```
  Scenario: Artifacts contain no secrets
    Tool: Bash
    Steps:
      - cargo test -p harness-testkit secretscan |& tee .sisyphus/evidence/task-17-secretscan.txt
    Expected: exits 0
    Evidence: .sisyphus/evidence/task-17-secretscan.txt
  ```

  **Commit**: YES | Message: `test(security): scan artifacts and snapshots for secrets` | Files: `crates/harness-testkit/src/*`, `crates/harness-testkit/tests/*`


- [ ] 18. Documentation refresh: README + config docs + CLIproxyAPI quickstart (responses mode)

  **What to do**:
  - Update `README.md`:
    - Interactive TUI launch: `cargo run -p harness -- tui`
    - Headless prompt: `cargo run -p harness -- prompt --text ...`
    - CLIproxyAPI config example using `/v1` base_url and `api_mode: "responses"`
  - Update `docs/config.md`:
    - Document `api_mode` and new `ui/logging/keybindings` sections.
  - Add a short “CLIproxyAPI quickstart” section:
    - base_url default: `http://127.0.0.1:8317/v1`
    - model example: `gpt-5-codex` (or configurable)
    - env var: `OPENAI_API_KEY`
  - Add a “License hygiene” note:
    - MIT repos are fine for inspiration; Pi Agent Rust license is unclear → do not copy code.

  **Must NOT do**:
  - Do not claim compatibility not tested (e.g., tool-calls).

  **Recommended Agent Profile**:
  - Category: `writing` — Reason: docs updates + clarity
  - Skills: []

  **Parallelization**: Can Parallel: YES | Wave 4 | Blocks: Final Verification | Blocked By: 1,3,9,10,11

  **References**:
  - Existing README quickstart: `README.md:20-70`
  - CLIproxyAPI docs: https://help.router-for.me/agent-client/codex
  - OpenAI Responses streaming guide: https://developers.openai.com/api/docs/guides/streaming-responses

  **Acceptance Criteria**:
  - [ ] Docs describe the new interactive TUI and Responses mode config accurately

  **QA Scenarios**:
  ```
  Scenario: Docs commands run
    Tool: Bash
    Steps:
      - cargo run -p harness -- --help |& tee .sisyphus/evidence/task-18-help.txt
      - cargo run -p harness -- schema > /tmp/harness.schema.json
    Expected: commands exit 0
    Evidence: .sisyphus/evidence/task-18-help.txt
  ```

  **Commit**: YES | Message: `docs: interactive tui + cliproxy responses quickstart` | Files: `README.md`, `docs/config.md`

## Final Verification Wave (4 parallel agents, ALL must APPROVE)
- [ ] F1. Plan Compliance Audit — oracle *(executed 2026-03-04; verdict: FAIL; see `.sisyphus/evidence/f1-plan-compliance-audit-2026-03-04.md`)*
- [ ] F2. Code Quality Review — unspecified-high *(executed 2026-03-04; verdict: FAIL; see `.sisyphus/evidence/f2-code-quality-review-2026-03-04.md`)*
- [ ] F3. Real Agent-Driven QA (pty-mcp + PTY E2E artifacts) — unspecified-high *(executed 2026-03-04; verdict: PASS with caveats; see `.sisyphus/evidence/f3-agent-driven-qa-review-2026-03-04.md`)*
- [ ] F4. Scope Fidelity Check — deep *(executed 2026-03-04; verdict: FAIL; see `.sisyphus/evidence/f4-scope-fidelity-check-2026-03-04.md`)*

### Final Verification Execution Notes (2026-03-04)

- Overall F-wave result: **NOT APPROVED**.
- Primary blocker themes:
  - implementation/evidence provenance mismatch across repo root vs orchestration worktree,
  - incomplete DoD satisfaction in the currently checked tree(s),
  - unresolved code-quality blockers in coordinator/task lifecycle paths.
- Consolidated audit: `.sisyphus/evidence/f-wave-2026-03-04-audit.md`.

## Commit Strategy
- Atomic commits per TODO (or paired TODOs when tightly coupled), using:
  - `feat(provider): ...`, `feat(tui): ...`, `feat(config): ...`, `test(e2e): ...`, `docs: ...`
- Keep snapshots and PTY artifacts updates in dedicated commits.

## Success Criteria
- Interactive TUI is pleasant and usable (OpenCode/Pi-inspired) and proven via PTY E2E + screenshots.
- Real provider calls work via CLIproxyAPI Responses API with gated live smoke.
- Config schema is expanded, documented, and validated; CLI uses it correctly.
- No secrets leak into persisted outputs.
