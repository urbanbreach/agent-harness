# agent-harness

## `task`

- `task` is the canonical child-delegation tool.
- `prompt` is the task body delivered to the child.
- `run_in_background` is required: `false` waits for the child result and does
  not emit a background wakeup; `true` returns ids immediately and later emits
  the background completion reminder.
- `load_skills` is required, even when empty.
- `skills` and `load_skills` are equivalent aliases for the same list.
- `command`, when provided, is prepended to the child prompt as delegation context.
- Listed skills are resolved before the child is spawned; missing or denied skills
  fail the task call, and loaded skill content is injected before the original task body.
- Task results include child runtime metadata and `next_actions` for status checks,
  waiting, cancellation, and continuation.
- `background_output(cancel: true, request_id: ...)` requests coordinator-owned
  cancellation for an authorized non-terminal background child task.

## Configuration

The current public integration surface is documented in [`docs/config.md`](docs/config.md).
Config-backed `mcp` servers are first-class: enabled MCP servers are
registered into the runtime tool registry, discovered server tools are exposed to
interactive profiles alongside the built-ins, and the generic
`mcp.<server>.tool.call` wrappers remain available for explicit discovery-oriented
flows.

This Rust workspace provides:

- a CLI entrypoint
- coordinator/runtime core
- provider adapters
- built-in native tools
- a Ratatui TUI
- native screenshot signoff plus deterministic PTY/live verification lanes

## Quick start

The default path is:

- provider: `default` (`openai_compatible`) via the local CLIProxy-compatible loopback endpoint
- default agent: `build`
- default model: `default/gpt-5.4`
- interactive model: `default/gpt-5.4-mini` (`high` reasoning preset)

Primary agents are discovered from `.agent-harness/agents/*.md` and use the
runtime config's `model` default:

- `build` — default implementation lane
- `plan` — stable read-only planning lane with runtime-enforced edits limited to the active `.agent-harness/plans/<run>.md` file, plus `plan_exit` to hand off to Build
- `discipline` — optional strict delivery lane for todo-driven autonomous work, focused delegation, and end-to-end verification
- `explore` — shipped read-only subagent profile for local codebase search via `task(subagent_type: "explore")`
- `general` — shipped focused implementation/research subagent profile via `task(subagent_type: "general")`
- category subagents — `visual-engineering`, `artistry`, `ultrabrain`, `deep`, `quick`, `unspecified-low`, `unspecified-high`, and `writing` route OMO-style `task(category: "...")` calls through ordinary toggleable profiles

Validate the shipped example config:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor
```

Shared runtime defaults can live at `$XDG_CONFIG_HOME/harness/harness.jsonc`
(fallback: `~/.config/harness/harness.jsonc`) or `$XDG_CONFIG_HOME/harness/harness.json`.
Project-local runtime config lives at `./harness.jsonc` or `./harness.json`.
TUI-only settings live separately in `./tui.jsonc` / `./tui.json` and the matching
XDG locations. When both global and local files exist, the harness merges global
defaults first and local files override them.

The older broad runtime shape plus `$XDG_CONFIG_HOME/harness/config.jsonc` still
load for compatibility, but `harness.json{,c}` and the matching XDG runtime paths
are the canonical public contract.

Launch the interactive harness with Build selected by default. Press `Tab` to
cycle primary agents, so the shipped profile set switches between Build, Plan,
and Discipline:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc
```

Plan mode is an operator workflow, not an experimental feature flag: switch to
Plan for analysis, let it create or update `.agent-harness/plans/<run>.md`, and
approve `plan_exit` when the plan is ready to continue in Build. Build can also
call `plan_enter` to ask whether complex work should switch into Plan first.
Plan uses native read/search/LSP tools for inspection, exposes `bash` only behind
permission prompts plus a read-only shell guard, and may delegate only to the
read-only `explore` profile under the current runtime policy. See the
[`docs/config.md` Plan operator workflow](docs/config.md#plan-operator-workflow)
for the step-by-step Build → Plan → Build approval flow.

Use the Discipline profile when you want OMO-style strictness without enabling a
new scheduler loop: it is a separate primary agent that prompts for todo hygiene,
focused delegation, and manual surface verification while leaving coordinator
scheduling, permissions, and event ownership unchanged.

Use category subagents when delegation should pick a domain-optimized lane
without adding scheduler state. `task(category: "visual-engineering")`,
`task(category: "ultrabrain")`, and the other shipped category names resolve to
matching subagent profiles first, then fall back to `general` when a category is
not configured. The shipped category profiles deny recursive task delegation by
default and can be toggled or retuned under `agent` like any other profile.

Run the harness headlessly from the terminal with the provider-backed `prompt` command:

```bash
cargo run -p harness -- prompt "Summarize the current workspace"
printf 'Review the changed files' | cargo run -p harness -- prompt --stdin
```

For tool-enabled headless stress tests, point `prompt` at a tool-capable config and
persist the event log for later inspection:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc \
  prompt --text "Use read on README.md and summarize it." \
  --out /tmp/harness-events.jsonl
```

The `build` profile continues after recoverable tool failures by
turning them into tool messages, so unsupported LSP/file probes can be surfaced to
the model without aborting the whole turn.

## Command-driven stress harness

Run the reusable stress suite directly from the terminal:

```bash
scripts/stress-harness.sh --mode offline
scripts/stress-harness.sh --mode live --config configs/harness.example.jsonc
scripts/stress-harness.sh --mode all --config configs/harness.example.jsonc
```

The script builds or reuses the harness binary, copies a fixture workspace from
`crates/harness-testkit/fixtures/stress_harness/`, and writes per-stage artifacts under
`target/harness-stress/run-*/` by default:

- `command.txt` — exact command that was executed
- `stdout.txt` / `stderr.txt` — captured terminal output
- `events.jsonl` — copied prompt/run event logs when a stage uses the harness event store
- `verification.txt` — simple invariant checks for the stage
- `summary.txt` — stage-by-stage PASS/FAIL rollup

Use `--harness-bin <path>` to skip the build step when a test runner already built the binary.
Every mode starts by validating the selected harness config, then `--mode offline` stays
deterministic and provider-free. `--mode live` and `--mode all` exercise the tool-enabled `prompt`
path against the configured provider, including best-effort LSP diagnostics, fail-open unsupported
LSP probes, and absolute-path workspace reads.

The shipped `default` provider points at the local CLIProxy-compatible bridge (`http://127.0.0.1:8317/v1`) and uses an explicit local placeholder bearer token so the default flow stays aligned between docs, config, and live signoff lanes without depending on `OPENAI_API_KEY`. Its catalog mirrors the configured CLIProxyAPI GPT family, including GPT 5.5, GPT 5.4, GPT 5.4 Mini, GPT 5.4 extended-context presets, GPT 5.3 Codex, GPT 5.2, and GPT 5.1/Codex variants.

The TUI exposes workflow slash commands for `/model`, `/status`, `/toggles`, `/resume`, `/new`, `/tree`, `/fork`, and `/clone`. `/model` switches the agent/model used for subsequent turns, `/status` opens the system status dialog, `/toggles` opens the session-local Toggles menu for configured agents, prompts, hooks, MCP servers, tools, skills, and YOLO menu state, `/resume` opens the saved-session picker, and `/new` starts a clean live run. `/tree` shows the Harness session lineage tree for saved sessions. `/fork` creates a child Harness session from the current session at an explicit stable event cutoff. `/clone` creates a child Harness session from the latest stable prefix of the selected source session.

The same lineage surface is available from the terminal through `harness sessions tree`, `harness sessions fork`, and `harness sessions clone`. `harness sessions tree` prints the saved Harness session lineage and accepts `--json`, `--root RUN_ID_OR_PATH`, and `--filter TEXT`. `harness sessions fork --source RUN_ID_OR_PATH --cutoff SEQ` writes a child session from a validated stable prefix. `harness sessions clone --source RUN_ID_OR_PATH` writes a child session from the latest stable completed prefix. Both write commands accept `--json`, reject active or writer locked sources, and print the child run id, source cutoff, event count, and copied artifact count when they succeed.

## Shipped workflow surfaces

- `configs/harness.example.jsonc` — canonical example config
- `configs/tui.example.jsonc` — canonical TUI config example
- `.agent-harness/agents/*.md` — built-in agent frontmatter and optional local prompt overrides/additions
- `crates/harness-testkit/tests/live_proxy_e2e.rs` — shipped config/signoff coverage

## Common commands

```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh all-deterministic
scripts/test-lanes.sh signoff-binary
```

See [`docs/testing.md`](docs/testing.md) for every lane mode, dry-run usage, env-gated live and
native signoff, stress lanes, and artifact expectations.
