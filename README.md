# agent-harness

## `agent.spawn` / `task`

- `agent.spawn` is the native child-delegation tool; `task` is its compat alias.
- `prompt` is the task body delivered to the child.
- `skills` and `load_skills` are equivalent aliases for the same list.
- `command`, when provided, is prepended to the child prompt as delegation context.
- Skill/command context is delivered as prompt instructions before the original task body.

Rust workspace for an event-sourced agent harness with:

## Configuration

The current public integration surface is documented in [`docs/config.md`](docs/config.md).
Config-backed `integrations.mcp.servers` are first-class: enabled MCP servers are
registered into the runtime tool registry, discovered server tools are exposed to
interactive profiles alongside the built-ins, and the generic
`mcp.<server>.tool.call` wrappers remain available for explicit discovery-oriented
flows.

- a CLI entrypoint
- coordinator/runtime core
- provider adapters
- built-in native + compat tools
- a Ratatui TUI
- native screenshot signoff plus deterministic PTY/live verification lanes

## Quick start

The current blessed default path is:

- provider: `default` (`openai_compatible`) via the local CLIProxy-compatible loopback endpoint
- default profile: `build`
- planning profile: `plan`
- default model: `gpt-5.4-mini`

Primary shipped agents:

- `build` — default implementation lane
- `plan` — restricted planning lane with `plan.exit` handoff to `build`

Secondary shipped agents:

- `tool_audit` — evidence/signoff lane
- `deep_compat` — compat-surface regression lane

Validate the shipped example config:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
```

Bootstrap a first-run local config in the auto-discovered location:

```bash
cargo run -p harness -- config init
```

Validate the generated local config:

```bash
cargo run -p harness -- config validate
```

Launch the interactive harness with the canonical plan -> build split:

```bash
cargo run -p harness --
```

The shipped config starts in the `build` agent by default. When you want a planning-first pass, launch `plan` explicitly and use `plan.exit` to hand off to `build` after approval.
The shipped `default` provider points at the local CLIProxy-compatible bridge (`http://127.0.0.1:8317/v1`) so the default flow stays aligned between docs, config, and live signoff lanes.

Inside the running harness, use `/model` or the command palette `switch_model` action to switch the active next-turn agent/profile between options such as `build` and `plan`.

## Shipped workflow surfaces

- `configs/harness.example.jsonc` — canonical example config
- `docs/plan-build-workflow.md` — first-run and handoff docs
- `docs/config.md` — shipped config surface for the blessed Plan/Build path
- `docs/testing.md` — canonical Plan/Build signoff map and known gaps
- `crates/harness-tools/tests/native_control_plane_tools.rs` — `plan.exit` behavior coverage
- `crates/harness-testkit/tests/live_proxy_e2e.rs` — shipped config/signoff coverage

## Common commands

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p harness --test config_schema_cli
cargo test -p harness-tools --test native_control_plane_tools
cargo test -p harness-tools --test opencode_compat_live
cargo test -p harness-testkit live_proxy_e2e
```
