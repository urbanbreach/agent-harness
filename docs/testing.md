# Testing

Agent Harness uses a three-level testing pyramid: unit, integration, and PTY E2E.

## Test Levels

### Unit Tests

Fast, isolated tests for individual modules:

```bash
cargo test -p harness-core clock
cargo test -p harness-core redact
cargo test -p harness-core hashline
cargo test -p harness-tools
```

Characteristics:
- No I/O or network
- Fake clock for deterministic time
- In-memory event store

### Integration Tests

Cross-module tests with real filesystem:

```bash
cargo test -p harness-core store
cargo test -p harness-core coord
cargo test -p harness-core perm
cargo test -p harness-tui
```

Characteristics:
- Temporary directories via `tempfile::TempDir`
- JSONL file store
- Snapshot testing with `insta`

### PTY E2E Tests

Full terminal UI tests using portable-pty:

```bash
export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_SEED=42
export TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8
export TERM=xterm-256color
export HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts

RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --test-threads=1
```

Relative `HARNESS_VISUAL_ARTIFACT_DIR` values are resolved from the repo root so `target/pty-visual-artifacts/` stays canonical even when Cargo runs test binaries from a crate subdirectory.

Characteristics:
- Spawns real `harness` binary in PTY
- vt100 terminal parsing
- Keystroke injection
- Pixel-level screenshot artifacts + visual hash assertions

### Agent-visible visual artifacts

PTY E2E renders terminal cells to deterministic PNG images at explicit lifecycle checkpoints:

- `pty_startup_launcher_ready.png`
- `pty_startup_command_palette.png`
- `pty_startup_continue_history.png`
- `pty_startup_replay_history.png`
- `pty_continue_rejected_active.png`
- `pty_continue_rejected_unrestorable.png`
- `pty_permission_overlay_parity.png`
- `pty_permission_requested.png`
- `pty_inline_completion_shell.png`
- `pty_continue_quiescent_session.png`
- `pty_session_shell_primary_live.png`
- `pty_session_shell_primary_replay.png`
- `pty_child_session_navigation.png`
- `pty_operator_sidebar_primary.png`
- `pty_operator_sidebar_session_native.png`
- `pty_native_tool_parity_task_row.png`
- `pty_native_tool_parity_fetch_row.png`
- `pty_native_tool_parity_dense.png`
- `pty_replay_read_only.png`

These PNGs, together with the manifest-backed families in `crates/harness-testkit/tests/support/visual_contracts.rs`, are the canonical offline parity evidence set. Older ad hoc local screenshots such as `pty_replay_diff_tab.png` or `pty_continue_live_diff_secondary.png` may still exist in a developer's `target/pty-visual-artifacts/` folder, but they are retired historical output and not current signoff proof.

The lane also keeps additive conversational coverage via:

- `pty_interactive_type_first_startup.png`
- `pty_interactive_prompt_stream.png`

Default output directory:

```bash
target/pty-visual-artifacts/
```

Override output directory (useful in CI/artifact upload scripts):

```bash
HARNESS_VISUAL_ARTIFACT_DIR=/tmp/harness-visuals \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --test-threads=1
```

Optional rendering controls for native-like text quality:

By default the renderer prefers an explicit font override, then the machine's configured `monospace` / `monospace:style=Bold` font via `fc-match`, and finally falls back to DejaVu Sans Mono.

```bash
# Use a specific monospace TTF to match your local terminal font
HARNESS_VISUAL_FONT_PATH="/path/to/your/terminal-font.ttf" \
  HARNESS_VISUAL_FONT_BOLD_PATH="/path/to/your/terminal-font-bold.ttf" \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e

# Toggle TTF anti-aliasing path (default: enabled)
HARNESS_VISUAL_TTF_ANTIALIAS=0 \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

The `.snap` files store marker checks plus `focus_pixels_blake3` digests for deterministic visual regression checks.

In GitLab CI, `rust:pty_e2e` exports `HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts`, keeps the deterministic env pinned (including `HARNESS_SEED=42`), and always publishes these PNG artifacts plus the snapshot corpus so agents can inspect real rendered frames offline.

### Screenshot-Driven UI Iteration Loop

The PTY E2E system enables an agent-driven visual QA workflow:

```bash
# Generate deterministic screenshots for the polished shell states
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
  RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e -- --test-threads=1

# Inspect generated PNGs for visual regressions
ls target/pty-visual-artifacts/*.png
```

**Iteration workflow:**

1. **Generate**: Run PTY E2E to produce deterministic startup, permission, post-run, continue-session, replay, child-session navigation, operator-sidebar parity, and dense transcript/tool checkpoints.
2. **Inspect**: Review `target/pty-visual-artifacts/*.png` for layout, spacing, transcript, overlay, sidebar, child-navigation, and read-only regressions. Prefer the manifest-backed inline transcript families, especially `pty_native_tool_parity_dense.png`, when judging transcript-first parity.
3. **Verify**: Check `.snap` files for marker presence and focus-region `focus_pixels_blake3` changes.
4. **Iterate**: Adjust UI code and repeat until both image inspection and hashes are stable.

This screenshot-driven approach allows agents to verify UI changes visually before committing code. In CI, artifacts are published from `target/pty-visual-artifacts/` for review.

### Live visual manifest helper checks

Run these non-live helper tests whenever the live visual pipeline changes:

```bash
cargo test -p harness-testkit live_visual_checkpoint_writes_png_and_manifest -- --exact
cargo test -p harness-testkit live_visual_external_png_checkpoint_supports_custom_namespace_and_prefix -- --exact
cargo test -p harness-testkit live_visual_run_retention_prunes_old_runs -- --exact
```

They validate that live visual checkpoints emit PNG + `manifest.json` + `manifest.jsonl` evidence and that retention only prunes manifest-backed run directories.

### Opt-in native terminal review lane

For a real terminal-rendered review pass, harness-testkit can mirror deterministic PTY output into a separate Konsole window and capture that window with Spectacle.

```bash
HARNESS_NATIVE_VISUAL=1 \
  cargo test -p harness-testkit pty_native_konsole_visual_review_lane -- --ignored --exact
```

This lane is additive to `pty_e2e` and is meant for intentional visual review, not cross-machine hash gating.

Current lane behavior:

- keeps the existing offline PTY/snapshot lane unchanged
- mirrors deterministic PTY bytes into a project-owned Konsole session
- captures real pixels with Spectacle under:

```text
target/pty-visual-artifacts/native-visual/pty_native_konsole_visual_review_lane/<run-id>/
```

- records the same `manifest.json` / `manifest.jsonl` contract used by live visual evidence
- emits two useful review checkpoints today:
  - `native_visual_run_finished.png` — dense replay transcript with thinking/tool rows from the native-tool-parity fixture
  - `native_visual_draft_visible.png` — continued session shell with an active editable composer draft

Platform assumptions and limitations:

- Linux only
- KDE desktop session expected (`XDG_CURRENT_DESKTOP=KDE`)
- requires `konsole`, `spectacle`, and `mkfifo`
- Spectacle still captures the active window, so keep focus on the spawned Konsole while the test runs
- treat the captured pixels as review/signoff artifacts, not as a portable hash oracle across machines

### Additive live visual signoff

Before releases, run the live visual verifier against a real provider/tool flow:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_e2e_visual_verifier -- --ignored --exact
```

This complements the offline `pty_e2e` suite by verifying the same polished startup and finished-state UX against live-provider screenshots and the captured visual manifest.

### Live chat-control signoff

For changes to chat-control tools and agent workflow helpers, run the live prompt lane against the
prepared live config so a real model actually chooses the native tools:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact
```

This lane exercises `todo.write`, `user.question`, and `skill.load` through `harness prompt`
with prepared live profiles. When the selected model exposes the documented `live_signoff`
variant, the prepared signoff profiles use it automatically so `gpt-5.4-mini` stays on the
low-reasoning Batch 1 signoff path. It is the fastest live-config signoff for non-visual
chat/tool-flow, skills, and question-routing changes.

### Live native tool-flow signoff

For CLI parity coverage of the same file/edit flow already exercised through the TUI lane, run the
native prompt tool-flow signoff:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_native_tool_flow -- --ignored --exact
```

This lane keeps the RB-04 signoff map tied to the existing `harness-testkit` live lanes while
adding CLI coverage for the same `fs.write` → `fs.read` → `edit.hashline_scan` →
`edit.hashline_apply` → `fs.read` path that the TUI live lane already proves visually.

Note: `live_proxy_preflight` is currently Linux-only because it validates the live TUI lane, and
`live_proxy_prompt_chat_tool_flow` seeds the repo-bundled `rust-best-practices` skill into its
temporary `.harness/skills` workspace, so a fresh checkout does not depend on an externally
installed skill.

### Batch 1 live parity signoff

For RB-04 / issue #71 signoff, prefer the composed wrappers around the shipped live lanes so the
highest-priority Batch 1 journeys run through both CLI and TUI entrypoints without inventing a new
verification taxonomy:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact
```

The CLI wrapper chains:

- `live_proxy_prompt_responses_smoke`
- `live_proxy_prompt_chat_tool_flow`
- `live_proxy_prompt_native_tool_flow`
- `live_proxy_prompt_compat_edit_flow`

The TUI wrapper chains:

- `live_proxy_preflight`
- `live_proxy_e2e_tui_prompt_responses_smoke`
- `live_proxy_e2e_tui_tool_flow`

Use the individual ignored tests while iterating on a single surface; use the composed wrappers for
issue/PR closeout when the acceptance claim is Batch 1 parity-signoff breadth.

## Canonical journey signoff expectations

For parity-critical issues, a passing test alone is not signoff. Treat the manifest-backed PTY PNGs/snapshots, live visual manifests, and captured transcript/event-log evidence as acceptance criteria. `live_proxy_preflight` is required before any live TUI lane.

### 1. First successful prompt run

- **Deterministic PTY baseline:** run `cargo test -p harness-testkit pty_e2e -- --test-threads=1`, then inspect the `startup_shell`, `startup_command_palette`, and `live_shell` artifacts, especially `pty_startup_home_primary.png`, `pty_startup_command_palette.png`, and `pty_interactive_type_first_startup.png`.
- **CLI expectation:** run `live_proxy_prompt_responses_smoke` when the default provider/model path or first prompt-success flow changes; prefer `live_proxy_prompt_parity_signoff` for issue/PR closeout so the first-run proof stays aligned with the wider Batch 1 live signoff set.
- **TUI expectation:** run `live_proxy_preflight`, then `live_proxy_e2e_tui_prompt_responses_smoke`; prefer `live_proxy_e2e_tui_parity_signoff` for issue/PR closeout, and add `live_proxy_e2e_visual_verifier` when the startup shell, composer, or finished-state visuals change.
- **Current gap:** no single shipped live lane yet covers the entire first-run journey from startup through permission handling and completion.

### 2. Transcript-first live session

- **Deterministic PTY baseline:** run `pty_e2e`, then review `transcript_shell` and `live_shell` artifacts such as `pty_native_tool_parity_dense.png`, `pty_native_tool_parity_task_row.png`, `pty_inline_completion_shell.png`, and `pty_interactive_prompt_stream.png`.
- **CLI expectation:** run `live_proxy_prompt_chat_tool_flow` for chat-control/state routing changes and `live_proxy_prompt_native_tool_flow` when the acceptance claim includes the native file/edit transcript path; prefer `live_proxy_prompt_parity_signoff` when the acceptance claim spans the Batch 1 signoff journeys instead of a single CLI surface.
- **TUI expectation:** run `live_proxy_preflight`, then `live_proxy_e2e_tui_tool_flow`; prefer `live_proxy_e2e_tui_parity_signoff` for Batch 1 signoff, and use `live_proxy_e2e_visual_verifier` when the visual transcript shell is part of the acceptance claim.
- **Current gap:** there is no shipped live dense-transcript oracle yet for disclosure depth, failure visibility, and transcript-heavy state changes.

### 3. Permission-handling flow

- **Deterministic PTY baseline:** run `pty_e2e`, then inspect `pty_permission_overlay_parity.png` and the corresponding snapshot markers/hashes.
- **CLI expectation:** no shipped live CLI permission lane exists today; do not claim live CLI signoff for permission changes without calling that gap out explicitly.
- **TUI expectation:** no shipped live TUI permission lane exists today; keep the PTY overlay evidence and report the missing live lane in issue/PR closeout notes.
- **Current gap:** live ask/allow/deny signoff is still missing for both headless and TUI flows.

### 4. Continue-session and recovery flow

- **Deterministic PTY baseline:** run `pty_e2e`, then inspect the `startup_session_history`, `continue_session`, `replay_shell`, `replay`, and `operator_sidebar` artifacts, especially `pty_startup_continue_history.png`, `pty_continue_quiescent_session.png`, `pty_session_shell_primary_replay.png`, `pty_child_session_navigation.png`, and `pty_replay_read_only.png`.
- **CLI expectation:** no shipped live reopen/continue/replay lane exists; if recovery behavior changes, document the missing live CLI oracle instead of silently waiving it.
- **TUI expectation:** same as CLI; rely on PTY artifacts for current signoff and report the absent live lane.
- **Current gap:** no live continue-session, replay, or artifact-discovery signoff lane exists yet.

### 5. Tool-heavy run inspection

- **Deterministic PTY baseline:** run `pty_e2e`, then inspect `transcript_shell`, `operator_sidebar`, and `live_shell` artifacts such as `pty_native_tool_parity_dense.png`, `pty_native_tool_parity_fetch_row.png`, `pty_operator_sidebar_primary.png`, and `pty_inline_completion_shell.png`.
- **CLI expectation:** run `live_proxy_prompt_chat_tool_flow` for chat-control/state routing changes, `live_proxy_prompt_native_tool_flow` for the native `fs.write`/`fs.read`/hashline path, and `live_proxy_prompt_compat_edit_flow` when the acceptance claim includes compat edit/read/apply coverage; prefer `live_proxy_prompt_parity_signoff` when closing out the Batch 1 parity map.
- **TUI expectation:** run `live_proxy_preflight`, then `live_proxy_e2e_tui_tool_flow`; prefer `live_proxy_e2e_tui_parity_signoff` for Batch 1 closeout, and add `live_proxy_e2e_visual_verifier` when screenshot-level transcript/tool-row parity is part of the claim.
- **Current gap:** live signoff still does not cover shell/search breadth, replay/artifact discovery, or the full dense-tool transcript surface.

If a change touches multiple journeys, run the union of the matching lanes. When the matrix above says coverage is missing, record the gap explicitly in the issue/PR closeout instead of claiming signoff by analogy.

## Deterministic Environment

For reproducible tests, set these environment variables:

```bash
export HARNESS_DETERMINISTIC=1        # Use fake clock, stable run_id
export HARNESS_DISABLE_ANIMATIONS=1   # Disable TUI animations
export HARNESS_SEED=42                # Deterministic seed (optional)
export TZ=UTC                         # Stable timezone
export LANG=C.UTF-8                   # Stable locale
export LC_ALL=C.UTF-8
export TERM=xterm-256color            # Stable terminal type
```

### Single-Threaded Testing

PTY tests must run single-threaded to avoid terminal interaction conflicts:

```bash
RUST_TEST_THREADS=1 cargo test pty_e2e
```

### Deterministic Run Verification

Golden path should produce identical JSONL across runs:

```bash
# Run twice and compare
HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --out run1.jsonl
HARNESS_DETERMINISTIC=1 cargo run -p harness -- run --scenario golden_path --deterministic --out run2.jsonl
diff run1.jsonl run2.jsonl  # Should be empty
```

## Snapshot Testing

The project uses `insta` for snapshot testing. Guidelines:

## Agent-run tool audit workflow

The shipped example config now includes a `tool_audit` profile for live, evidence-first harness
self-checks. It is meant for agents that need to verify real tool behavior, not just inspect the
registered schema, and it keeps the `gpt-5.4-mini` / `deterministic` baseline pinned for repeatable
audit runs.

Key properties:

- broad native tool surface enabled, including `fs.tree`, `edit.apply_patch`, `tool.batch`, and `agent.spawn`
- permissive local edit/shell/task/question policy for audit runs
- explicit system prompt that forbids claiming success without a matching tool call and observed
  postcondition, and that calls out skills, question flow, hooks evidence, LSP, subagent lineage,
  variants, and model metadata as first-class audit targets
- `tool_failure_mode: continue_as_tool_message`, so blocked or invalid tool calls become evidence
  inside the same run instead of aborting the entire audit turn

Recommended headless audit command:

```bash
HARNESS_QUESTION_ANSWERS='[["Yes"]]' \
  cargo run -p harness -- \
  --config configs/harness.example.jsonc \
  prompt \
  --profile tool_audit \
  --text "Audit the active tool surface in this workspace. Actually invoke tools, verify postconditions, capture skills, question flow, hooks evidence, LSP, subagent lineage, variants, and model metadata when available, and clearly separate succeeded, blocked, failed, and untested tools." \
  --out target/tool-audit-events.jsonl
```

Notes:

- `HARNESS_QUESTION_ANSWERS` lets headless prompt runs exercise `user.question` without opening the
  TUI, and the captured prompts are mirrored under `state/questions/<tool_call_id>.json` in the run
  directory.
- Network-capable tools remain deny-by-default in the example audit profile; that is intentional so
  agents can observe and report permission-limited surfaces without requiring external credentials.
- Because tool failures stay in-band for `tool_audit`, an agent can probe edge cases and continue
  collecting evidence across the rest of the tool surface in one run.

### Creating Snapshots

```bash
# Update snapshots during development
cargo test --workspace --all-features

# In CI, fail on snapshot mismatch
INSTA_UPDATE=no cargo test --workspace --all-features
```

### Redactions

Sensitive data must be redacted from snapshots:

```rust
insta::with_settings!({
    filters => vec![
        (r"sk-[A-Za-z0-9]{10,}", "[REDACTED_API_KEY]"),
    ]
}, {
    insta::assert_snapshot!(output);
});
```

### Snapshot Locations

- Inline snapshots: `insta::assert_snapshot!()`
- File snapshots: `insta::assert_snapshot!("name", output)`
- Stored in `src/snapshots/` or `tests/snapshots/`

## Test Organization

```
crates/
  harness-core/
    src/
      clock.rs          # Unit tests in #[cfg(test)]
      redact.rs         # Unit tests in #[cfg(test)]
    tests/
      coord.rs          # Integration tests
      store.rs          # Integration tests
  harness-tui/
    tests/
      snapshots/        # insta snapshots
  harness-testkit/
    tests/
      pty_e2e.rs        # PTY end-to-end tests
    fixtures/
      mock_provider/    # Mock provider responses
```

## Running Specific Test Categories

```bash
# All unit tests
cargo test --lib

# All integration tests
cargo test --test '*'

# Specific crate
cargo test -p harness-core

# Specific test pattern
cargo test hashline
cargo test permission
cargo test coordinator

# With all features enabled
cargo test --workspace --all-features

# Offline only (no live proxy tests)
cargo test --workspace

# Live proxy tests (explicitly gated)
# HARNESS_LIVE_PROXY_CONFIG defaults to configs/harness.example.jsonc
# HARNESS_LIVE_PROXY_PROVIDER defaults to "default"
# OPENAI_API_KEY is optional for local CLIProxy when placeholder key fallback is used
# HARNESS_LIVE_PROXY_MODEL can force a specific real provider model
# HARNESS_LIVE_PROXY_VARIANT overrides the shipped live_signoff/low-reasoning signoff default
# HARNESS_LIVE_PROXY_PROMPT customizes the smoke prompt text
# HARNESS_LIVE_PROXY_WAIT_TIMEOUT_MS controls live smoke wait timeout (default 120000)
# Native defaults live in `configs/harness.example.jsonc`, including native tool ids such as
# `fs.tree`, `edit.apply_patch`, and native skill roots under `.harness/skills`.
# Sign-off order for Batch 1 parity breadth:
#   1. live_proxy_prompt_parity_signoff
#   2. live_proxy_e2e_tui_parity_signoff
# Individual component lanes remain the right choice while iterating on a single surface.
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact

# Component live lanes for narrower iteration
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_chat_tool_flow -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_native_tool_flow -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_compat_edit_flow -- --ignored --exact

HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
HARNESS_VISUAL_ARTIFACT_DIR=target/pty-visual-artifacts \
cargo test -p harness-testkit live_proxy_e2e_tui_tool_flow -- --ignored --exact

# Optional: keep the prompt smoke around for plain non-tool provider checks
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_prompt_responses_smoke -- --ignored --exact

# Optional: use the TUI prompt smoke for first-run/default-path live coverage
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_e2e_tui_prompt_responses_smoke -- --ignored --exact

# Optional: run the whole ignored live proxy suite in one shot
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit --test live_proxy_e2e -- --ignored

### Live Proxy Visual Verification

For screenshot-driven regression testing against real providers:

```bash
# Run the live visual verifier against the captured live screenshots/manifest
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.4-mini \
cargo test -p harness-testkit live_proxy_e2e_visual_verifier -- --ignored --exact
```

This verifies the polished startup/draft-visible/run-finished checkpoints produced during a real-provider session.

# Provider-layer live smoke (optional, same env gate)
HARNESS_LIVE_PROXY=1 cargo test -p harness-providers openai -- --ignored
```

## CI Testing

The CI pipeline runs:

```bash
# Format check
cargo fmt --check

# Linting
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Unit and integration tests
INSTA_UPDATE=no cargo test --workspace --all-features

# PTY E2E (Linux only, single-threaded)
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

## Writing New Tests

### Unit Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature() {
        let clock = FakeClock::new();
        let result = do_something(&clock);
        assert_eq!(result, expected);
    }
}
```

### Integration Test Template

```rust
use tempfile::TempDir;
use harness_core::store::JsonlFileEventStore;

#[tokio::test]
async fn test_store_append() {
    let dir = TempDir::new().unwrap();
    let store = JsonlFileEventStore::new(dir.path()).await.unwrap();
    // Test implementation
}
```

### PTY E2E Test Template

```rust
#[test]
fn test_golden_path_interactive() {
    let pty = spawn_pty("harness tui --scenario golden_path_interactive --deterministic");
    
    // Wait for permission prompt
    pty.wait_for_text("PermissionRequested", Duration::from_secs(5));
    
    // Send approval
    pty.send_key('a');
    
    // Wait for completion
    pty.wait_for_text("RunFinished", Duration::from_secs(10));
    
    // Take snapshot
    insta::assert_snapshot!(pty.screen_contents());
}
```

## Debugging Failed Tests

### Snapshot Mismatches

```bash
# Review pending snapshots
cargo insta review

# Accept all pending
cargo insta accept

# Reject all pending
cargo insta reject
```

### PTY Test Failures

Enable debug output:

```bash
RUST_LOG=debug RUST_TEST_THREADS=1 cargo test pty_e2e -- --nocapture
```

Inspect rendered checkpoint PNGs:

```bash
ls target/pty-visual-artifacts
```

### Determinism Failures

Check for:
- Unordered HashMap iteration (use BTreeMap)
- Wall clock usage (use Clock trait)
- Random values without seeded RNG
- Async timing assumptions

## Test Fixtures

### Mock Provider Fixtures

Located in `crates/harness-testkit/fixtures/mock_provider/`:

```json
{
  "request_digest": "abc123...",
  "responses": [
    {"Start": {}},
    {"TextDelta": "Hello"},
    {"Done": {"usage": {"prompt_tokens": 10}}}
  ]
}
```

### Golden Path Fixtures

- `golden_path` - Headless scenario with auto-resolved permissions
- `golden_path_interactive` - Interactive scenario requiring TUI approval
