# Privacy and local data

Harness stores sessions, artifacts, config, prompts, and skills locally. Live
provider requests, enabled MCP servers, network tools, and model-catalog refreshes
can send data over the network.

## Outgoing requests

| Action | Network behavior |
| --- | --- |
| Live provider turn | Sends the selected context to the configured provider. |
| MCP tool call | Uses the configured server and transport. |
| `webfetch`, `websearch`, `codesearch` | Sends a tool request under permission policy. |
| Live model-catalog initialization | May download metadata from `https://models.dev/api.json`. |
| Mock TUI model picker | Reads the embedded catalog without a network request. |
| Replay, session inspection, doctor, support export | Reads local data without provider or MCP requests. |

The model catalog uses a five-minute cache. It serves a valid stale cache while
refreshing in the background. Without a usable cache, it tries a download and
falls back to bundled metadata on failure.

Set `HARNESS_DISABLE_MODELS_FETCH=1` to use only the embedded catalog.
`HARNESS_MODELS_URL` changes the source; `HARNESS_MODELS_PATH` changes the cache
location.

## Storage

User files live in the Harness home, `<home>`: a nonempty `HARNESS_HOME` used
as-is, otherwise `$HOME/.harness`.

`<project-key>` encodes the canonical absolute project path: strip one leading
slash or backslash, replace slashes, backslashes and colons with dashes, and wrap
the result in `--`. For example, `/work/app` becomes `--work-app--`. Symlink aliases
therefore share a bucket. Personal configuration and generated data share this root.

| Data | Location |
| --- | --- |
| Runtime config | `<home>/harness.jsonc` or `harness.json`, explicit `HARNESS_CONFIG`, and project layers |
| Keyboard config | `<home>/tui.jsonc` or `tui.json`, explicit `HARNESS_TUI_CONFIG`, and project layers |
| Personal agents, commands and prompts | `<home>/agents/`, `<home>/commands/` and `<home>/prompts/` |
| Global skills | `<home>/skills` and `$HOME/.agents/skills` by default; configured by `skills.global_roots` |
| Last picked TUI model | `<home>/model.json`, unless `HARNESS_MODEL_SELECTION_STATE_FILE` overrides it |
| Cached model metadata | `<home>/models-cache.json` |
| Subscription bindings | `<home>/anthropic-subscription-bindings/` |
| Authored project agents, commands, skills and prompts | `<project>/.harness/agents/`, `.harness/commands/`, `.harness/skills/`, `.harness/prompts/`; project skills also load from `.agents/skills/` |
| User instructions | `<home>/AGENTS.md` |
| Project instructions | First existing of `AGENTS.md`, then `CLAUDE.md`, in each project directory |
| Workspace permission grants | Project `.harness/permission-grants.json` |
| Events and artifacts | `<home>/sessions/<project-key>/<run-id>`, unless session storage is explicitly overridden |
| Memory, code index, edit attribution and plans | `<home>/projects/<project-key>` |
| Managed Git worktrees | `<home>/worktrees/<project-key>` |
| Stored credentials | `<home>/credentials/<authProvider>.json` |

An empty `runtime.session_dir` selects managed storage automatically. A nonempty
config value or `--session-dir` overrides session storage only. Relative overrides
still resolve against the selected project. Authored project files and workspace
grants are not relocated. Old project-local sessions are not migrated; read them
with `--session-dir <old-session-dir>`.

Use `harness config sources` to find active configuration files. Successful
sign-in may create a minimal personal config without overwriting existing files;
see [config discovery](../configuration/config.md#discovery-and-precedence).
Credential files use restrictive permissions. Logout removes stored credentials,
but leaves configured environment and inline credential fallbacks in place.

## Redaction and sharing

The redactor lives in [`crates/harness-core/src/redact.rs`](../../crates/harness-core/src/redact.rs).
Live output is redacted before reaching CLI or TUI subscribers. Incomplete words
and possible credentials are held until safe, including credentials split across
provider fragments. Reasoning remains transient and shares the response byte limit.
Support export redacts API keys, bearer tokens, cookies, PEM blocks, provider
credentials, and hidden prompt or config instruction values. It includes a
redaction manifest and refuses to write a bundle if the final secret scan fails.
Use [support export](../architecture/sessions-and-replay.md#cli-inspection)
instead of sharing raw `events.jsonl`.

V1 has no telemetry, cloud analytics, billing, web sharing, or hosted
collaboration. Review provider and MCP settings before live use. An approved
shell command can perform host I/O; permission approval is not OS confinement.
