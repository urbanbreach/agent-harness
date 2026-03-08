# Agent Harness

Event-sourced multi-agent orchestration harness with append-only JSONL sessions, strict permissions, and atomic hashline edits.

Behavioral inspiration from [Oh My OpenCode](https://github.com/opencode-ai/opencode) and [Oh My Pi](https://github.com/can1357/oh-my-pi). No code or prompts were copied. See [License Hygiene](#license-hygiene).

## Overview

Agent Harness provides a deterministic, auditable foundation for running AI agents:

- **Append-only event log**: Every action recorded as JSONL with schema versioning
- **Single-authority Coordinator**: Central scheduler with concurrency gates and stale detection
- **Permission model**: Explicit allow/deny/ask for edits, shell, and network operations
- **Hashline edits**: Atomic file patches with content-addressed anchors (blake3)
- **Modern TUI**: Ratatui-based interface with live streaming and replay modes
- **Deterministic replay**: Same inputs produce identical JSONL digests

## Quickstart

### Installation

```bash
cargo build --release
```

### Launch the Interactive TUI

`harness` is now the normal config-backed daily launch. It resolves config from `--config`, `./harness.jsonc`, or your XDG config directory. If no config resolves, it fails closed with setup guidance and suggests `--mock`.

```bash
# Config-backed daily launch
cargo run -p harness --

# Explicit config path
cargo run -p harness -- --config configs/harness.example.jsonc

# Explicit deterministic demo/mock mode
cargo run -p harness -- --mock

# Compatibility alias for the interactive TUI
cargo run -p harness -- tui

# Compatibility alias with an explicit config path
cargo run -p harness -- --config configs/harness.example.jsonc tui

# Deterministic scenario path kept for PTY/live automation
cargo run -p harness -- tui --scenario golden_path_interactive --deterministic

# Replay a previous session
cargo run -p harness -- replay --session .agent-harness/sessions/<run_id>

# List all sessions
cargo run -p harness -- sessions list
```

### Demo / Mock Mode

Use `--mock` when you want the deterministic demo workspace without any config file:

```bash
# Bare launch in explicit mock mode
cargo run -p harness -- --mock

# Compatibility alias for explicit mock mode
cargo run -p harness -- tui --mock

# Run a specific scenario with mock provider
cargo run -p harness -- run --scenario golden_path --mock --deterministic
```

### Headless Scenarios (Testing)

For deterministic testing with predefined scenarios:

```bash
# Golden path scenario (deterministic, no human input)
cargo run -p harness -- run --scenario golden_path --deterministic

# TUI with a specific deterministic scenario
cargo run -p harness -- tui --scenario golden_path_interactive
```

### Headless Prompt (Non-TUI)

```bash
# Single prompt, headless execution (requires config)
cargo run -p harness -- prompt --text "Explain the code in src/main.rs" --config my-config.jsonc

# With explicit config
cargo run -p harness -- prompt --text "Hello" --config my-config.jsonc

# With mock provider (no config required)
cargo run -p harness -- prompt --text "Hello" --mock

# Output to file
cargo run -p harness -- prompt --text "Hello" --out response.jsonl
```

### Validate Configuration

```bash
# Print JSON Schema
cargo run -p harness -- schema

# Validate a config file
cargo run -p harness -- config validate --config configs/harness.example.jsonc
```

## Configuration

Config files use JSONC (JSON with comments). Resolution order:

1. `--config <path>` flag
2. `./harness.jsonc` in current directory
3. `${XDG_CONFIG_HOME:-~/.config}/harness/config.jsonc`

Normal interactive use is config-first: both `cargo run -p harness --` and `cargo run -p harness -- tui` require one of those config locations unless you explicitly opt into `--mock`.

Example with CLIProxy-style base URL:

```jsonc
{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "${OPENAI_API_KEY:-sk-zerolimit}"
    }
  }
}
```

See [docs/config.md](docs/config.md) for full documentation and [configs/harness.example.jsonc](configs/harness.example.jsonc) for a complete example.

## CLIproxyAPI Quickstart

Connect to a local CLIproxyAPI instance using the OpenAI Responses API:

```jsonc
{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "${OPENAI_API_KEY:-sk-zerolimit}",
      "api_mode": "responses",
      "models": {
        "gpt-5.3-codex": {
          "display_name": "GPT-5.3 Codex",
          "max_input_tokens": 128000,
          "max_output_tokens": 16384
        }
      }
    }
  }
}
```

Set your API key (optional when your local CLIProxy uses subscription auth and accepts a placeholder token like `sk-zerolimit`):

```bash
export OPENAI_API_KEY="your-api-key-here"
```

The `api_mode` setting controls which API endpoint to use:
- `"responses"` - Use `/v1/responses` (OpenAI Responses API with streaming)
- `"chat_completions"` - Use `/v1/chat/completions` (standard chat completions)
- `"auto"` - Try responses first, fall back to chat completions on 404/405

## Testing

The project uses a three-level testing pyramid:

### Unit Tests

```bash
cargo test --workspace
```

### Integration Tests

```bash
# With snapshot testing (requires INSTA_UPDATE=no in CI)
INSTA_UPDATE=no cargo test --workspace --all-features
```

### PTY E2E Tests

Deterministic end-to-end tests using portable-pty and vt100 parsing:

```bash
# Required environment for deterministic runs
export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8
export TERM=xterm-256color

# Run PTY tests (single-threaded for determinism)
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e

# Optional: write deterministic UI screenshots to a custom directory
HARNESS_VISUAL_ARTIFACT_DIR=/tmp/harness-visuals \
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

The PTY E2E suite now emits deterministic PNG checkpoints for the polished startup, prompt stream, permission modal, and run-finished states, and validates visual hashes for agent-visible UX regression testing.
In GitLab CI, these images are published from `target/pty-visual-artifacts/` by the `rust:pty_e2e` job.

For full pre-release signoff, pair the offline PTY suite with the live visual verifier:

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=configs/harness.example.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=default \
HARNESS_LIVE_PROXY_MODEL=gpt-5.3-codex \
cargo test -p harness-testkit live_proxy_e2e_visual_verifier -- --ignored --exact
```

See [docs/testing.md](docs/testing.md) for detailed testing documentation.

## Documentation

- [docs/architecture.md](docs/architecture.md) - Crate boundaries, event schema, coordinator design
- [docs/config.md](docs/config.md) - Configuration reference and JSON Schema
- [docs/testing.md](docs/testing.md) - Testing levels, deterministic environments, snapshots

## Project Structure

```
crates/
  harness/          # CLI binary (run, tui, replay, schema, config)
  harness-core/     # Event store, coordinator, permissions, projections
  harness-providers/# Provider abstraction, MockProvider, OpenAI-compatible proxy
  harness-tools/    # Built-in tools (fs.read, shell.run, edit.hashline_apply)
  harness-tui/      # Ratatui interface (live mode, replay mode, diff viewer)
  harness-testkit/  # Test helpers, fixtures, PTY E2E harness
```

## License Hygiene

This project draws behavioral inspiration from Oh My OpenCode and Oh My Pi:

- **Architecture patterns**: Event sourcing, hashline edits, permission models
- **User experience**: Terminal UI workflows, streaming output

No code, prompts, or proprietary implementations were copied. All code is original and independently authored.

**License notes**:
- MIT-licensed repositories (like Oh My OpenCode) are fine for inspiration
- Pi Agent Rust license is unclear; do not copy code from it

## License

MIT License - see LICENSE file for details.
