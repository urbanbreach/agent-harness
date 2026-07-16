# Onboarding Terminal Migration PRD

**Status:** Active implementation PRD for moving provider onboarding out of the TUI wizard and into the terminal, with a lightweight non-blocking TUI dialog for post-setup provider management.

**Audience:** One autonomous implementation agent working in this repository until the strict end-state goal below is true.

**Product authority:** Onboarding should happen in the terminal on first run (like `harness auth login`), not as a blocking 13-step TUI wizard. After first setup, the TUI should offer a lightweight non-blocking dialog for adding/changing providers (like Harness's `/connect`).

---

## 0. Strict end-state goal

This PRD is complete only when all of the following are true at the same time:

1. When a user runs `harness` (no subcommand) with no stored credentials for any configured provider, the terminal-based `harness auth login` interactive flow runs BEFORE the TUI launches. The user can complete auth or cancel; either way the TUI launches afterward.
2. When a user runs `harness` with valid credentials already stored, the TUI launches directly with no onboarding gate, no onboarding overlay, and no onboarding state machine.
3. The 13-step `OnboardingStep` state machine, its `OnboardingState` struct, its `screen_for()` renderer, its keyboard handling, and its startup lifecycle rendering are fully removed from the codebase. No dead code, no feature-gated remnants, no commented-out blocks.
4. The TUI has a `/connect` slash command (distinct from the existing `/auth`) that opens a lightweight provider-connection dialog. This dialog is non-blocking (dismissable with `Esc`) and delegates to the same `harness auth login` backend via the existing `spawn_tui_auth_backend_task()` mechanism.
5. When no providers are connected and the user is in the TUI, a status banner or hint directs them to `/connect` or to run `harness auth login` in a terminal. The TUI does not block, crash, or become unusable.
6. `harness auth login`, `harness auth list`, and `harness auth logout` CLI commands work exactly as before — no behavioral change, no API change, no prompt change.
7. All existing deterministic tests pass. Tests that exercised the old onboarding wizard are updated or replaced to test the new terminal gate and `/connect` dialog. No test is deleted to "pass" — it is replaced with equivalent coverage of the new behavior.
8. No config files (`harness.jsonc`, `tui.jsonc`) are written or modified by onboarding or `/connect`. Credentials remain in the OS keyring only.

If any item above is false, continue implementing. Do not stop at a partial migration, a hidden wizard, or a `/connect` stub that doesn't actually connect.

---

## 1. Background and current state

### 1.1 Current onboarding architecture

The harness currently runs a 13-step onboarding wizard as a TUI overlay inside the Ratatui app:

- **State machine**: `crates/harness-tui/src/app/onboarding.rs` (284 lines) — `OnboardingState` struct with `visible`, `step`, `selected`, `skipped_for_launch`, `auth_in_progress`, `secret_input` fields. The `OnboardingStep` enum has 13 variants: `StartSplash`, `ProviderPick`, `AuthMethodPick`, `CopilotTargetPick`, `CodexBrowser`, `CodexDevice`, `CopilotPublicDevice`, `CopilotEnterpriseDevice`, `ApiKeyEntry`, `LoginSuccess`, `LoginErrorTimeout`, `SkipConfirmation`, `SkillSelection`.
- **Trigger**: `onboarding_required_for_runtime()` in `crates/harness/src/tui/auth_backend.rs:13-37` checks whether any configured auth provider lacks stored credentials, env vars, or inline keys. The flag is passed to the TUI via `TuiMode::Startup { onboarding_required, ... }`.
- **Keyboard handling**: `crates/harness-tui/src/app/key_interaction.rs:455-676` — onboarding selection, auth step execution, hidden text input.
- **Rendering**: `crates/harness-tui/src/ui_lifecycle.rs:137-215` — `render_onboarding_screen()` renders an elevated panel over the startup logo.
- **Auth backend delegation**: `crates/harness/src/tui/auth_backend.rs:65-117` — `spawn_tui_auth_backend_task()` spawns a background thread that calls `execute_auth_backend_args_with_io()` — the same `harness auth` CLI entry point.

### 1.2 Existing terminal auth CLI (already complete)

The harness already has a full terminal-based auth CLI that mirrors Harness's `harness auth login`:

- **Entry point**: `crates/harness/src/auth_cmd.rs` — `execute_login()` (line 301) dispatches to `execute_interactive_login()` (line 365) when no provider is specified.
- **Interactive flow**: `execute_interactive_login()` calls `clack_intro` → `prompt_auth_provider` → `prompt_login_method` → `interactive_enterprise_url` → `execute_login_selection()`.
- **Prompt UI**: `crates/harness/src/auth_cmd/prompt_ui.rs` — Clack-style terminal prompts: `prompt_pick` (searchable select), `prompt_input` (text/password), `clack_intro`, `clack_outro`, `clack_log_info/success/error`.
- **Credential store**: OS keyring via `CredentialStore`.
- **Commands**: `harness auth login [provider] [--method] [--enterprise-url]`, `harness auth list`, `harness auth logout [provider]`.

### 1.3 Existing TUI auth slash command

The TUI already has an `/auth` slash command:
- `crates/harness-tui/src/app/session_navigation.rs:301-309` — parses auth args, emits `UiIntent::OpenAuthManager { args, stdin }`.
- `crates/harness-tui/src/app/lifecycle.rs:124` — `OpenAuthManager` variant in `UiIntent` enum.
- `crates/harness-tui/src/app/session_slash.rs:55-72` — `auth_slash_args_from_prompt()` parses `/auth login`, `/auth list`, etc.
- The `OpenAuthManager` intent is handled by `spawn_tui_auth_backend_task()` which runs the same `harness auth` CLI backend.

### 1.4 How Harness does it (reference)

Harness has two independent paths:
- **CLI**: `harness providers login` (alias `harness auth`) — terminal flow using `@clack/prompts` (`select`, `autocomplete`, `password`, `spinner`). Writes to `auth.json`.
- **TUI dialog**: `DialogProvider` component — auto-shows when `sync.data.provider.length === 0`. Also accessible via `/connect` slash command. Non-blocking (dismissable with `esc`). Handles auth via SDK/HTTP calls to the server backend.

Key Harness properties the harness should adopt:
- Onboarding can happen entirely in the terminal before the TUI launches.
- The TUI dialog is non-blocking and simple (provider list → auth method → credential entry), not a 13-step wizard.
- The TUI dialog is accessible after first setup via a slash command.

---

## 2. Goals

1. Move first-run onboarding to the terminal — run `harness auth login` interactive flow before TUI launch when credentials are missing.
2. Replace the 13-step TUI onboarding wizard with a lightweight non-blocking `/connect` dialog.
3. Remove all onboarding wizard code (state machine, rendering, keyboard handling).
4. Keep the existing terminal auth CLI (`auth_cmd.rs`, `prompt_ui.rs`) unchanged.
5. Keep credentials in the OS keyring — never write to config files.

---

## 3. Non-goals

1. **Do NOT** change the `harness auth login/list/logout` CLI commands, their prompts, their arguments, or their behavior.
2. **Do NOT** change the credential store mechanism (OS keyring).
3. **Do NOT** write credentials or provider configuration to `harness.jsonc`, `tui.jsonc`, or any config file.
4. **Do NOT** add new crate dependencies. The terminal prompts (`prompt_ui.rs`) and TUI auth backend (`spawn_tui_auth_backend_task()`) already exist.
5. **Do NOT** implement OAuth/browser-auth flows natively in the TUI. The TUI dialog delegates to the CLI backend, same as today.
6. **Do NOT** change the `harness auth` subcommand structure or add new subcommands.
7. **Do NOT** touch the coordinator, event schema, provider transport, or permission system.
8. **Do NOT** change the model switcher, toggles menu, or status dialog.
9. **Do NOT** remove the `/auth` slash command. It stays as-is for `auth list` and `auth logout`. The new `/connect` is a separate command for the provider connection dialog.

---

## 4. Architecture decisions

### 4.1 Terminal first-run gate (before TUI launch)

**Decision**: When `harness` is launched with no subcommand and credentials are missing, run the existing `execute_interactive_login()` terminal flow before entering the TUI.

**Rationale**: The terminal flow already exists and works. Reusing it avoids duplication. The user gets a Clack-style terminal prompt (provider pick → method → credential entry), exactly like `harness auth login`.

**Flow**:
```
harness (no subcommand)
  → load config
  → check onboarding_required_for_config()
  → if true:
      → print "No provider credentials found. Let's connect one."
      → run execute_interactive_login() (terminal prompts)
      → regardless of success/cancel: launch TUI
  → if false:
      → launch TUI directly
```

### 4.2 TUI `/connect` dialog (replaces onboarding wizard)

**Decision**: Add a `/connect` slash command that opens a lightweight provider-connection dialog. This dialog is non-blocking and delegates to the same `spawn_tui_auth_backend_task()` mechanism.

**Rationale**: Mirrors Harness's `/connect` dialog. Users can add/change providers after first setup without leaving the TUI. The existing `OpenAuthManager` intent and `spawn_tui_auth_backend_task()` mechanism already handle the auth backend execution.

**Dialog behavior**:
- `/connect` opens a simple overlay: provider list → auth method selection → credential input (if API key) or OAuth device code display.
- The dialog delegates to `UiIntent::OpenAuthManager` with `args = ["login", ...]` — the same mechanism the old onboarding wizard used.
- `Esc` dismisses the dialog. The TUI remains usable.
- When no providers are connected, a status banner hints: "No provider connected. Use /connect or run `harness auth login` in a terminal."

### 4.3 No config file writes

**Decision**: Onboarding and `/connect` write only to the OS keyring (credential store). Config files are never modified.

**Rationale**: This matches the current behavior and Harness's separation of credentials (`auth.json`) from config (`harness.json`). The harness already separates credentials (keyring) from config (`harness.jsonc`).

---

## 5. Implementation spec

### Phase 1: Terminal first-run gate

#### 5.1 Modify `crates/harness/src/tui.rs`

**Current**: `run_interactive_mode()` (around line 290) calls `onboarding_required_for_runtime()` and passes the flag to `TuiMode::Startup { onboarding_required, ... }`.

**Change**: Before entering the TUI, if `onboarding_required_for_config()` returns true AND not in demo/mock/replay mode:
1. Print a message to stderr/stdout: "No provider credentials found. Let's connect one." (or similar)
2. Call the existing `execute_interactive_login()` flow (or `execute_auth_backend_args_with_io()` with `["login"]` args) using the terminal I/O (not TUI I/O).
3. Do NOT block on the result — whether the user completes auth, cancels, or encounters an error, proceed to launch the TUI.
4. Remove the `onboarding_required` flag from `TuiMode::Startup`. The TUI no longer needs to know about onboarding state.

**Guardrail**: The terminal auth flow must use the existing `execute_interactive_login()` or `execute_auth_backend_args_with_io()` — do NOT reimplement prompt logic. Do NOT create a new function that duplicates `prompt_auth_provider` / `prompt_login_method` / `prompt_input`.

**Guardrail**: The gate must NOT fire in these cases (same as current `onboarding_required_for_runtime()`):
- `--mock` / demo mode
- `--scenario` / golden path scenarios
- `replay` mode
- No config file at all (returns `false` when config is `None`)

#### 5.2 Modify `crates/harness/src/tui/auth_backend.rs`

**Current**: `onboarding_required_for_runtime()` (lines 13-37) checks config and demo mode. `spawn_tui_auth_backend_task()` (lines 65-117) spawns the auth backend.

**Change**:
- Keep `onboarding_required_for_config()` (or the function it delegates to in `auth_cmd/support.rs`) — it's still needed for the terminal gate.
- Remove `onboarding_required_for_runtime()` — the TUI no longer needs this flag. The terminal gate in `tui.rs` calls the check directly.
- Keep `spawn_tui_auth_backend_task()` — it's still used by the `/connect` dialog and `/auth` slash command.

### Phase 2: Replace TUI onboarding wizard with `/connect` dialog

#### 5.3 Add `/connect` slash command

**Files to modify**:
- `crates/harness-tui/src/app/session_navigation.rs` — add `"connect"` to the slash command match arms. When triggered, emit `UiIntent::OpenAuthManager { args: vec!["login".to_string()], stdin: None }`. This reuses the existing auth backend spawning mechanism.
- `crates/harness-tui/src/app/session_slash.rs` — add `"connect"` to `auth_slash_args_from_prompt()` if needed, or handle it as a separate command.
- `crates/harness-tui/src/keybindings.rs` — register `/connect` in the slash command list with description "Connect a provider".

**Behavior**: `/connect` triggers the same `OpenAuthManager` intent that the old onboarding wizard's auth steps used. The auth backend runs in a background thread (via `spawn_tui_auth_backend_task()`), and the result is applied via `apply_auth_backend_result()`.

**Guardrail**: Do NOT create a new `UiIntent` variant. Reuse `OpenAuthManager`. Do NOT create a new overlay type. The auth backend runs in the background and reports results via `LiveUpdate` — same as today.

#### 5.4 Add "no provider connected" status banner

**File to modify**: `crates/harness-tui/src/app.rs` or `crates/harness-tui/src/app/lifecycle.rs`

**Change**: When the TUI starts and no providers have credentials, set a status banner: "No provider connected. Use /connect or run `harness auth login` in a terminal." This banner clears when the user connects a provider or dismisses it.

**Guardrail**: This is a status banner, not a blocking overlay. The TUI must remain usable — the user can type, browse sessions, etc.

### Phase 3: Remove onboarding wizard

#### 5.5 Remove `crates/harness-tui/src/app/onboarding.rs`

Delete the entire file. This removes:
- `OnboardingState` struct
- `OnboardingStep` enum (13 variants)
- `screen_for()` function
- All onboarding screen definitions

#### 5.6 Remove onboarding field from `AppState`

**File**: `crates/harness-tui/src/app.rs`

Remove:
- The `onboarding: OnboardingState` field from `AppState`
- `set_onboarding_required()` method
- `onboarding_screen()` method
- `apply_auth_backend_result()` — **WAIT**: `apply_auth_backend_result()` is also called from `runtime.rs:674` for the `/auth` and `/connect` flows. Do NOT remove it. Instead, keep it but remove its coupling to `OnboardingState`. It should update a status banner or refresh the provider catalog, not transition onboarding steps.

**Guardrail**: `apply_auth_backend_result()` must still work for `/auth` and `/connect`. Trace all callers before removing anything. If `apply_auth_backend_result()` only updates onboarding state, refactor it to update a status banner + trigger provider catalog refresh instead.

#### 5.7 Remove onboarding keyboard handling

**File**: `crates/harness-tui/src/app/key_interaction.rs`

Remove lines 455-676 (the `onboarding.visible && focus == Focus::List` branch):
- `move_onboarding_selection()`
- `execute_onboarding_selection()`
- Hidden text input for `ApiKeyEntry` / `CopilotEnterpriseDevice`
- The `OnboardingStep` match arms

**Guardrail**: Do NOT remove the `OpenAuthManager` intent emission from other code paths (e.g., `/auth` slash command in `session_navigation.rs`). Only remove the onboarding-wizard-specific keyboard handling.

#### 5.8 Remove onboarding rendering

**File**: `crates/harness-tui/src/ui_lifecycle.rs`

Remove:
- `render_onboarding_screen()` (lines 188-215)
- The onboarding check in `render_startup_lifecycle_flow()` (lines 137-153) that early-returns when onboarding is visible
- The onboarding check in `crates/harness-tui/src/ui.rs` (lines 362-364) that hides the bottom dock during onboarding

#### 5.9 Remove onboarding from TUI runtime

**File**: `crates/harness-tui/src/runtime.rs`

Remove:
- `onboarding_required` from `TuiMode::Startup` (lines 160-165, 209-226)
- The code that passes `onboarding_required` to `AppState::set_onboarding_required()`

#### 5.10 Remove onboarding from TUI entry point

**File**: `crates/harness/src/tui.rs`

Remove:
- The `onboarding_required` variable (line 290)
- The `onboarding_required` field in `TuiOptions` / `TuiMode::Startup` (lines 507-520)
- The call to `onboarding_required_for_runtime()` (line 290)

Replace with the terminal gate from Phase 1 (§5.1).

### Phase 4: Update tests

#### 5.11 Update onboarding tests

**Files to modify**:
- `crates/harness-tui/src/app/exact_tests.rs` (lines 269-556) — these test the onboarding wizard. Replace with tests for:
  - Terminal gate fires when credentials are missing
  - Terminal gate does NOT fire in mock/replay/no-config mode
  - `/connect` slash command emits `OpenAuthManager` intent
  - Status banner appears when no providers are connected
  - `apply_auth_backend_result()` updates status banner and refreshes catalog
- `crates/harness-tui/src/runtime.rs:923` — `drain_live_updates_applies_auth_backend_result_to_onboarding` — rename and update to test the new behavior (status banner update, not onboarding step transition).
- `crates/harness-tui/tests/pty_e2e.rs` — update PTY e2e tests that exercise the onboarding wizard to test the terminal gate + `/connect` flow instead.

**Guardrail**: Every old test that tested a specific onboarding behavior must have a replacement test that covers the equivalent new behavior. Do NOT delete tests without replacement. Do NOT weaken assertions.

#### 5.12 Update snapshot tests

**Files to modify**:
- `crates/harness-tui/src/snapshots/` — any snapshots that include onboarding screens must be updated or removed.
- `crates/harness-tui/tests/snapshots/` — same.

**Guardrail**: Use `cargo insta review -p harness-tui --accept` only after intentionally updating the behavior. Document which snapshots changed and why.

---

## 6. Guardrails (anti-gaming)

### 6.1 No hidden wizard

The onboarding wizard must be fully removed, not hidden behind a feature flag, a `cfg` attribute, or a runtime toggle. Searching for `OnboardingStep`, `OnboardingState`, `onboarding_required`, `onboarding_screen`, `set_onboarding_required`, `render_onboarding_screen`, `move_onboarding_selection`, `execute_onboarding_selection` must return ZERO results in the codebase after implementation.

### 6.2 No auth logic duplication

The terminal gate must call the existing `execute_interactive_login()` or `execute_auth_backend_args_with_io()`. Do NOT write a new terminal prompt function. Do NOT copy `prompt_auth_provider` / `prompt_login_method` / `prompt_input` into a new module. If you need to call the existing functions from a new location, import them — do not reimplement them.

### 6.3 No config file writes

Onboarding and `/connect` must not write to `harness.jsonc`, `tui.jsonc`, or any file in the config path. Credentials go to the OS keyring via `CredentialStore`. Verify this by searching the diff for `write`, `create_dir`, `fs::write`, `File::create` in any new code — none of these should appear in onboarding/connect code paths.

### 6.4 No new dependencies

Do NOT add crates to `Cargo.toml` for any harness crate. The terminal prompts (`prompt_ui.rs`), TUI rendering (ratatui), and auth backend (`spawn_tui_auth_backend_task()`) already exist. If you believe a new dependency is needed, stop and document why — do not add it.

### 6.5 No type suppressions

No `as any`, `#[allow(...)]`, `#[ts-ignore]`, or equivalent in any new or modified code. Existing suppressions in unchanged code stay as-is; do not add new ones.

### 6.6 No dead code

After implementation, `cargo build -p harness -p harness-tui` must produce no dead-code warnings for onboarding-related symbols. If the compiler warns about unused code, remove it — do not suppress the warning.

### 6.7 No test deletion without replacement

Every test file that exercises onboarding behavior must be updated, not deleted. If a test tested "onboarding wizard shows StartSplash", replace it with a test that "terminal gate runs when credentials missing" or "/connect emits OpenAuthManager intent". The replacement test must cover the same user-facing behavior (first-run auth setup).

### 6.8 No behavioral change to auth CLI

`harness auth login`, `harness auth list`, `harness auth logout` must work identically before and after this PRD. Run `harness auth list` before and after — same output. Run `harness auth login --help` before and after — same output. Do NOT modify `auth_cmd.rs` command structs, argument parsing, or prompt sequences.

### 6.9 No widening scope

Do NOT refactor `auth_cmd.rs`, `prompt_ui.rs`, the credential store, the provider transport, the coordinator, or the event schema. Do NOT add new auth methods, new providers, or new OAuth flows. Do NOT change the model switcher, toggles menu, or status dialog. If you find something that "could be improved" along the way, document it as a follow-up note — do not change it in this PRD.

### 6.10 No breaking the TUI shell contract

The TUI must remain usable when no providers are connected:
- The composer must accept input.
- The slash command menu must work.
- `/connect` must be available.
- Session list, replay, and model switcher must work (model switcher may show "no models available" — that's fine).
- The TUI must not crash, panic, or hang.

---

## 7. Invariants that must hold

1. **Events are the source of truth** — onboarding does not emit events. This does not change.
2. **Coordinator is the only event append authority** — onboarding does not append events. This does not change.
3. **Permission checks precede tool execution** — onboarding does not bypass permissions. This does not change.
4. **Provider metadata is redacted** — onboarding/auth must never persist raw requests, responses, auth headers, cookies, keys, PEM blocks, or hidden reasoning text. This does not change.
5. **Config files are not written by onboarding** — credentials stay in the OS keyring.
6. **Replay is side-effect free** — onboarding does not execute during replay. The terminal gate must NOT fire in replay mode.
7. **Mock/demo mode bypasses onboarding** — the terminal gate must NOT fire in `--mock` mode.

---

## 8. Testing requirements

### 8.1 Unit tests (deterministic)

| Test | What it verifies |
|---|---|
| `terminal_gate_fires_when_credentials_missing` | `onboarding_required_for_config()` returns true when no credentials stored; terminal gate calls `execute_interactive_login` |
| `terminal_gate_skipped_in_mock_mode` | Gate does NOT fire when `demo_mode` is true |
| `terminal_gate_skipped_in_replay_mode` | Gate does NOT fire in replay mode |
| `terminal_gate_skipped_when_no_config` | Gate does NOT fire when config is `None` |
| `terminal_gate_skipped_when_credentials_present` | Gate does NOT fire when credentials are stored |
| `connect_slash_command_emits_open_auth_manager` | `/connect` emits `UiIntent::OpenAuthManager { args: ["login"] }` |
| `connect_slash_command_available_in_session` | `/connect` appears in slash command list during live session |
| `no_provider_banner_shown_when_disconnected` | Status banner appears when no providers connected |
| `apply_auth_backend_result_updates_banner` | `apply_auth_backend_result(true)` clears the "no provider" banner and refreshes catalog |
| `apply_auth_backend_result_failure_shows_error` | `apply_auth_backend_result(false)` shows an error banner |

### 8.2 Integration tests

```bash
# Deterministic TUI tests
cargo nextest run -p harness-tui
cargo nextest run -p harness-tui --test deterministic_render_test
cargo nextest run -p harness-tui --test tui_signoff_manifest_test

# Auth CLI tests (must pass unchanged)
cargo nextest run -p harness --test auth

# Config validation
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor

# Full deterministic lane
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh all-deterministic
```

### 8.3 PTY e2e tests

```bash
RUST_TEST_THREADS=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1
```

PTY tests that exercised the old onboarding wizard must be updated to test:
- Terminal gate runs when credentials are missing (PTY captures the Clack-style prompts)
- `/connect` opens auth flow from within the TUI
- TUI is usable without credentials (can type, open slash commands, etc.)

### 8.4 Manual verification

```bash
# 1. Verify terminal gate fires with no credentials
# (Clear keyring or use fresh environment)
cargo run -p harness -- --config configs/harness.example.jsonc
# Expected: terminal prompts for provider selection before TUI launches

# 2. Verify terminal gate skips with credentials
harness auth login codex --method api-key  # store a credential
cargo run -p harness -- --config configs/harness.example.jsonc
# Expected: TUI launches directly, no terminal gate

# 3. Verify /connect works in TUI
# In TUI: type /connect
# Expected: auth backend spawns, status banner updates

# 4. Verify mock mode skips gate
cargo run -p harness -- --config configs/harness.example.jsonc run --mock "hello"
# Expected: no terminal gate, runs directly

# 5. Verify auth CLI unchanged
harness auth list
harness auth login --help
harness auth logout --help
```

---

## 9. File change summary

### Files to modify

| File | Change |
|---|---|
| `crates/harness/src/tui.rs` | Replace `onboarding_required_for_runtime()` gate with terminal `execute_interactive_login()` call before TUI launch. Remove `onboarding_required` from `TuiOptions`/`TuiMode::Startup`. |
| `crates/harness/src/tui/auth_backend.rs` | Remove `onboarding_required_for_runtime()`. Keep `spawn_tui_auth_backend_task()`. Keep the `onboarding_required_for_config` check function (or its delegate in `auth_cmd/support.rs`). |
| `crates/harness-tui/src/app.rs` | Remove `onboarding: OnboardingState` field, `set_onboarding_required()`, `onboarding_screen()`. Refactor `apply_auth_backend_result()` to update status banner + refresh catalog instead of onboarding state. |
| `crates/harness-tui/src/app/key_interaction.rs` | Remove onboarding keyboard handling (lines ~455-676). Keep `OpenAuthManager` intent emission from other paths. |
| `crates/harness-tui/src/app/lifecycle.rs` | Remove onboarding coupling from `render_startup_lifecycle_flow()`. Keep `UiIntent::OpenAuthManager` variant. |
| `crates/harness-tui/src/ui_lifecycle.rs` | Remove `render_onboarding_screen()` and onboarding check in `render_startup_lifecycle_flow()`. |
| `crates/harness-tui/src/ui.rs` | Remove onboarding check that hides bottom dock (lines ~362-364). |
| `crates/harness-tui/src/runtime.rs` | Remove `onboarding_required` from `TuiMode::Startup`. Remove `set_onboarding_required()` call. Update `drain_live_updates_applies_auth_backend_result_to_onboarding` test. |
| `crates/harness-tui/src/app/session_navigation.rs` | Add `"connect"` slash command handler that emits `UiIntent::OpenAuthManager { args: ["login"], stdin: None }`. |
| `crates/harness-tui/src/app/session_slash.rs` | Add `/connect` command parsing if needed. |
| `crates/harness-tui/src/keybindings.rs` | Register `/connect` in slash command list with description "Connect a provider". |
| `crates/harness-tui/src/app/exact_tests.rs` | Replace onboarding wizard tests with terminal gate + `/connect` tests. |
| `crates/harness-tui/tests/pty_e2e.rs` | Update onboarding PTY tests for terminal gate + `/connect`. |
| `crates/harness-tui/src/snapshots/` | Update or remove onboarding screen snapshots. |

### Files to delete

| File | Reason |
|---|---|
| `crates/harness-tui/src/app/onboarding.rs` | Entire 13-step wizard removed. |

### Files that must NOT change

| File | Reason |
|---|---|
| `crates/harness/src/auth_cmd.rs` | Auth CLI commands unchanged. |
| `crates/harness/src/auth_cmd/prompt_ui.rs` | Terminal prompts unchanged. |
| `crates/harness/src/auth_cmd/login.rs` | Credential writing unchanged. |
| `crates/harness/src/auth_cmd/support.rs` | `onboarding_required_for_config()` stays (used by terminal gate). |
| `crates/harness-core/` | No coordinator/event/permission changes. |
| `crates/harness-providers/` | No provider transport changes. |
| `crates/harness-tools/` | No tool surface changes. |
| `configs/` | No config file changes. |
| `.agent-harness/` | No runtime asset changes. |

---

## 10. Verification checklist

Before declaring this PRD complete, verify ALL of the following:

- [ ] `grep -r "OnboardingStep\|OnboardingState\|onboarding_required\|onboarding_screen\|set_onboarding_required\|render_onboarding_screen\|move_onboarding_selection\|execute_onboarding_selection" crates/` returns zero results.
- [ ] `crates/harness-tui/src/app/onboarding.rs` does not exist.
- [ ] `cargo build -p harness -p harness-tui` succeeds with no warnings.
- [ ] `cargo nextest run -p harness-tui` passes (all updated tests).
- [ ] `cargo nextest run -p harness-tui --test deterministic_render_test` passes.
- [ ] `cargo nextest run -p harness-tui --test tui_signoff_manifest_test` passes.
- [ ] `RUST_TEST_THREADS=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1` passes.
- [ ] `scripts/test-lanes.sh fast` passes.
- [ ] `scripts/test-lanes.sh quality-gates` passes.
- [ ] `scripts/test-lanes.sh all-deterministic` passes.
- [ ] `harness auth list` works identically to before.
- [ ] `harness auth login --help` works identically to before.
- [ ] Terminal gate fires when credentials are missing (manual test).
- [ ] Terminal gate does NOT fire when credentials are present (manual test).
- [ ] Terminal gate does NOT fire in `--mock` mode (manual test).
- [ ] `/connect` works in the TUI (manual test).
- [ ] TUI is usable without credentials (manual test — can type, open slash commands, etc.).
- [ ] No new dependencies in any `Cargo.toml`.
- [ ] No `#[allow(...)]` or type suppressions in new/modified code.
- [ ] No config files written by onboarding or `/connect` (verify by code inspection).

---

## 11. Implementation order

1. **Phase 1**: Terminal first-run gate in `tui.rs` + `auth_backend.rs` (§5.1, §5.2)
2. **Phase 2**: `/connect` slash command + status banner (§5.3, §5.4)
3. **Phase 3**: Remove onboarding wizard code (§5.5–§5.10) — do this AFTER Phase 1 and 2 work, so the TUI is never left in a broken state
4. **Phase 4**: Update tests (§5.11, §5.12)
5. **Verify**: Run full verification checklist (§10)

**Critical ordering rule**: Phase 3 (removal) must NOT begin until Phase 1 (terminal gate) and Phase 2 (`/connect`) are working and tested. The TUI must never be left in a state where onboarding is removed but the terminal gate and `/connect` don't work.

---

## 12. Follow-up notes (out of scope)

Document any findings during implementation that suggest improvements:
- Better terminal prompt UX (colored output, progress indicators)
- `/connect` dialog with inline provider list (instead of delegating to CLI backend)
- Auto-detection of available providers from config
- Model selection after provider connection (like Harness's `DialogModel`)

These are NOT part of this PRD. Document them in the PRD progress file and leave them for future work.
