# agent-harness

## `task`

- `task` is the canonical child-delegation tool.
- `prompt` is the task body delivered to the child. For non-trivial delegation,
  structure it with `context`, `goal`, `downstream use`, `request`,
  `required tools`, `must-do`, and `must-not-do` sections so the child receives
  reviewable task context.
- `description` is optional; when omitted, a short label is auto-generated from
  the first few words of the prompt.
- `run_in_background` defaults to `false`: `false` waits for the child result
  and does not emit a background wakeup; `true` returns ids immediately and
  later emits the background completion reminder.
- `load_skills` defaults to an empty list; `skills` and `load_skills` are
  equivalent aliases for the same list.
- `command`, when provided, is prepended to the child prompt as delegation context.
- Listed skills are resolved in request order before the child is spawned;
  duplicate names load once at their first occurrence. Missing, denied, disabled,
  malformed, or symlink-unsafe skills fail the task call before child spawn.
- Loaded skill content is injected before optional command context and before the
  original task body. Task results report compact loaded-skill metadata, including
  stable id, status, source scope, and `body_loaded: false`, without echoing full
  skill bodies.
- Task results include child runtime metadata, `next_actions` for status checks,
  waiting, cancellation, and continuation, plus capped `result_summary` /
  `failure_summary` text and a structured `child_summary` object. The
  parent-visible child summary is redacted and capped at 1,200 characters; when
  truncated, it ends with `…` and records `truncated: true`, `max_chars`, and the
  observed `original_chars`.
- `background_cancel(request_id: ...)` is the canonical coordinator-owned
  cancellation tool for an authorized non-terminal background child task.
  `background_output(cancel: true, request_id: ...)` remains compatibility.

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

For a first source build from outside this repository, clone the workspace,
build the binary, and copy the canonical runtime config into the directory where
you want to run Harness:

```bash
git clone <repo-url> agent-harness
cd agent-harness
cargo build -p harness
mkdir -p /tmp/harness-first-run/.agent-harness
cp configs/harness.example.jsonc /tmp/harness-first-run/harness.jsonc
cd /tmp/harness-first-run
/path/to/agent-harness/target/debug/harness --help
/path/to/agent-harness/target/debug/harness --version
/path/to/agent-harness/target/debug/harness config validate
/path/to/agent-harness/target/debug/harness doctor
/path/to/agent-harness/target/debug/harness run --mock "Hello from PTY" \
  --out prompt.events.jsonl --print-run-dir
```

That run command is deterministic mocked execution. It proves the first prompt
path separately from `doctor`; it does not prove live provider authentication or
transport health.

For a real provider first run, use the shipped `openai-codex` provider and log in
with `harness auth login codex`, or set `OPENAI_API_KEY` as the documented
fallback. The provider keeps credential material out of config with
`authProvider: "codex"`; live prompts exercise the Codex OAuth-backed request
path. `doctor` checks that the named credential is available and redacted.
doctor does not prove live provider authentication or transport health.

This workspace's dogfood path in `harness.jsonc` is:

- provider: `umans-ai-coding-plan` (`openai_compatible`)
- default agent: `build`
- default model: `umans-ai-coding-plan/umans-kimi-k2.7` in this workspace's `harness.jsonc`
- interactive model: `umans-ai-coding-plan/umans-kimi-k2.7`
- category agents: high-effort lanes use `umans-ai-coding-plan/umans-kimi-k2.7`; quick/read-only/writing lanes use `umans-ai-coding-plan/umans-flash`

Primary agents and category subagents are discovered from `.agent-harness/agents/*.md` and use the
runtime config's direct model or named `model_profile` settings:

- `build` — default implementation lane
- `plan` — stable read-only planning lane with runtime-enforced edits limited to the active `.agent-harness/plans/<run>.md` file, plus `plan_exit` to hand off to Build
- `explore` — shipped read-only subagent profile for local codebase search via `task(subagent_type: "explore")`
- `general` — shipped focused implementation/research subagent profile via `task(subagent_type: "general")`
- category subagents — `visual-engineering`, `artistry`, `ultrabrain`, `deep`, `quick`, `unspecified-low`, `unspecified-high`, and `writing` route category-based `task(category: "...")` calls through ordinary toggleable profiles with category-specific model profiles and fallback metadata

Validate the shipped example config and inspect the merged effective result:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc config show --effective
cargo run -p harness -- --config configs/harness.example.jsonc config sources
cargo run -p harness -- --config configs/harness.example.jsonc config explain model
cargo run -p harness -- config settings
cargo run -p harness -- --config configs/harness.example.jsonc doctor
```

`config show --effective` prints redacted merged JSON with discovery layer paths
and the primary path. `config sources` lists those layers in merge order.
`config explain <path>` attributes one dotted key to the winning source layer.
`config settings` lists typed settings-registry metadata (ids/surface/sensitivity;
no secret values).
Secret-bearing fields such as `apiKey` are replaced with redaction markers; use
these commands for support/debug inspection without leaking credentials.

`doctor` proves local readiness only: config shape, provider/model metadata,
credential presence, resolved agent catalog metadata, native tool catalog
metadata, prompt asset status, permissions, session directory readiness, and MCP
registration without making provider or MCP network calls. Use `prompt`,
`signoff-live`, or a live stress lane when you need real provider execution
proof, and keep README/release claims scoped to the lane you actually ran.

For support evidence, export a completed or failed session instead of sharing raw
event logs directly:

```bash
/path/to/agent-harness/target/debug/harness sessions export \
  --session-dir <session-dir> \
  --output support-bundle.json \
  <run-id-or-directory-name>
```

The support export includes replay-derived session metadata, offline doctor JSON,
non-secret config/provider summaries, agent catalog summary, native tool catalog
summary, session-tool readiness, route metadata, artifact indexes, a redaction
manifest, and secret-scan status so a failure can be debugged without exposing
API keys, bearer tokens, cookies, PEM blocks, raw provider credentials, or hidden
prompt/config instruction secrets.

Model-visible session inspection is available through `session_list`,
`session_read`, `session_search`, and `session_info`; these tools read stored
event logs with `source: "event_replay"` and do not execute providers, tools,
hooks, MCP servers, network calls, or the `harness sessions` CLI. The V1 native
tool catalog is summarized in [`docs/native-tool-catalog.md`](docs/native-tool-catalog.md).

Troubleshooting starts with the local checks before live provider execution:

- Missing credentials: run `harness doctor`; it reports missing `apiKey` or
  unresolved `apiKeyEnv` names without printing secret values.
- Invalid credentials or rate limits: run a live `prompt`/stress lane; `doctor`
  does not make provider calls and cannot prove authentication.
- Base URL or provider mismatch: compare the provider `baseURL` and
  `authProvider` in `harness.jsonc` with the expected Codex OAuth setup and use
  the live prompt lane for transport proof.
- Missing MCP/LSP/tool prerequisites: `doctor` reports configured MCP readiness;
  tool failures are persisted as tool messages, and unsupported LSP probes stay
  recoverable in the prompt path.
- Unsupported tool calls or malformed provider streams: inspect `events.jsonl`,
  use session inspection, then export a support bundle.
- Session resume failure: use `harness sessions inspect` and the support export
  artifact index to locate the corrupt or missing session/artifact.
- Permission denial or timeout: the event log records `permission_resolved`,
  `edit_rejected`, and failed tool output while leaving the workspace unchanged.
- Terminal rendering issues: retry with `--mock`, use `/help` or `Ctrl+p` for the
  command palette, and attach the redacted support bundle plus terminal details.
- Mock provider fixture miss: if `harness run --mock "..."` reports a missing
  mock fixture, confirm the request shape matches a stored cassette, generate or
  copy the cassette with the same request digest, or fall back to a live prompt
  lane; the mock provider surfaces the missing digest and expected path so the
  cassette index can be updated without guessing.

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
cycle primary agents, so the shipped profile set switches between Build and Plan:

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

Use category subagents when delegation should pick a domain-optimized lane
without adding scheduler state. `task(category: "visual-engineering")`,
`task(category: "ultrabrain")`, and the other shipped category names resolve to
matching subagent profiles first, then fall back to `general` when a category is
not configured. The shipped category profiles deny recursive task delegation by
default and can be toggled or retuned under `agent` like any other profile.

Run the harness headlessly from the terminal with the command-driven `run` command:

```bash
cargo run -p harness -- run "Summarize the current workspace"
printf 'Review changed files' | cargo run -p harness -- run
cargo run -p harness -- run --model openai-codex/gpt-5.5 "hello"
```

`harness prompt` remains available as the lower-level compatibility surface used
by older scripts and focused prompt-path tests.

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

The shipped `openai-codex` provider uses `authProvider: "codex"` with an `OPENAI_API_KEY` fallback for Codex OAuth-backed first-run checks. Workspace dogfooding uses the `umans-ai-coding-plan` provider from `harness.jsonc`; the separate starter catalog intentionally defines only GPT 5.5 and GPT 5.4 Mini. Broader generated provider catalogs live in `configs/provider-catalog.reference.jsonc`.

The TUI exposes workflow slash commands for `/model`, `/toggles`, `/resume`, `/new`, `/tree`, `/fork`, `/clone`, and `/rename`. `/model` switches the agent/model used for subsequent turns, `/toggles` opens the session-local Toggles menu for configured agents, prompts, hooks, MCP servers, tools, skills, and YOLO menu state, `/resume` opens the saved-session picker, and `/new` starts a clean live run. `/tree` shows the Harness session lineage tree for saved sessions. `/fork` creates a child Harness session from the current session at an explicit stable event cutoff. `/clone` creates a child Harness session from the latest stable prefix of the selected source session. `/rename` (`/title`) renames the current session by emitting an `UpdateSessionTitle` event, which is replayed by session inspection tools.

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
