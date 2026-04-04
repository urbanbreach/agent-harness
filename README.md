# agent-harness

`agent-harness` is a Rust workspace for an event-sourced agent runtime with a CLI entrypoint, coordinator/runtime core, provider adapters, built-in tools, a Ratatui TUI, and deterministic PTY/live verification.

## Session discovery and recovery

The CLI now supports both public session discovery and headless recovery for saved interactive runs:

```bash
# catalog saved runs with resumability, artifacts, child sessions, session path, and parent lineage
harness sessions list

# inspect a saved run's replay summary without opening the TUI
harness sessions inspect --run <run-id> --json

# inspect resumability, recent prompt context, child-session lineage, and tool artifacts
harness sessions reopen --session <run-id-or-path>
harness sessions reopen --session <run-id-or-path> --json

# continue a resumable interactive session from the shell
harness prompt --resume <run-id-or-path> --text "continue from the last stopping point"
```

Use `sessions inspect` when you want the replay/session summary, and `sessions reopen` when you need recovery-specific resume context before continuing a saved interactive session.

## Workspace map
- `crates/harness` — CLI entrypoint, startup/replay orchestration, interactive mode wiring
- `crates/harness-core` — events, coordinator, scheduler, permissions, projections, config
- `crates/harness-providers` — provider adapters, including OpenAI-compatible support
- `crates/harness-tools` — built-in tool registry and compat/native tool surfaces
- `crates/harness-tui` — Ratatui live/replay shell and rendering contracts
- `crates/harness-testkit` — secret scanning and deterministic PTY/live verification helpers

## Shipped starter skill pack
The repository now ships a built-in starter skill pack in `.agents/skills/` so a fresh checkout can exercise skill discovery immediately.

Included starter skills:
- `rust-best-practices`
- `issue-delivery`

The default project-root search order is:
1. `.opencode/skills`
2. `.claude/skills`
3. `.agents/skills`

Because the bundled pack lives in `.agents/skills`, you can override any shipped skill by creating a same-named skill earlier in the search order without editing the bundled files.

See `docs/starter-skills.md` for the override/extension workflow.

## Example config
A tracked example config now ships at `configs/harness.example.jsonc`.
It demonstrates:
- native and compat tool profiles
- the bundled starter skill roots
- lifecycle hook examples
- LSP server examples
- model variants used by the live tool-audit lane

The CLI does **not** auto-discover `configs/harness.example.jsonc`. For a fresh checkout,
either pass it explicitly:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc tui
```

or copy it to `./harness.jsonc` if you want it picked up by the default config search path.

## Verification shortcuts
Common focused commands:
```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p harness-tools skill_load_discovery
cargo test -p harness-testkit example_tool_audit_profile_covers_signoff_surface_and_gpt_5_4_mini_baseline -- --exact
```

For TUI/runtime shell work, also run:
```bash
cargo test -p harness-tui
```

For PTY/live helper changes, follow the documented order in `crates/harness-testkit/tests/README.live-proxy.md`.
