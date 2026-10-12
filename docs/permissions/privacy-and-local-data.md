# Privacy and local data

Harness stores sessions, artifacts, config, prompts, and skills locally. Live
provider requests, enabled MCP servers, network tools, and model-catalog refreshes
can send data over the network.

## Outgoing requests

| Action | Network behavior |
| --- | --- |
| Live provider turn | Sends the selected context to the configured provider. |
| MCP tool call | Connects lazily over the configured transport after permission checks. Stdio servers may make their own network requests. |
| `webfetch`, `websearch`, `codesearch`, GitHub tools | Sends a request to the selected URL or configured service under tool permission policy. |
| Model-catalog initialization | May download metadata from `https://models.dev/api.json`. |
| OAuth sign-in or credential refresh | Contacts the authentication endpoints for the selected provider. |
| Mock TUI model picker | Reads the embedded catalog without a network request. |
| Replay, session inspection, doctor, support export | Reads local data without provider or MCP requests. |

The model catalog uses a five-minute cache. It serves a valid stale cache while
refreshing in the background. Without a usable cache, it tries a download and
falls back to bundled metadata on failure.

Set `HARNESS_DISABLE_MODELS_FETCH=1` (or `true`, case-insensitive) to use only
the embedded catalog, even if a cache exists. `HARNESS_MODELS_URL` changes the
source; `HARNESS_MODELS_PATH` changes the cache location. Catalog fetching is
not a tool call and does not use tool permission approvals.

Provider context can include prompts, instructions, tool arguments and tool
results. Bash commands, eval code and lifecycle hooks can also use the network.
A local session directory does not make a live run offline.

## Storage

User files live in the Harness home, `<home>`: a nonempty `HARNESS_HOME` used
as-is, otherwise `$HOME/.harness`.

`<project-key>` is `v2-` followed by the full BLAKE3 hex digest of the canonical
absolute UTF-8 project path. Symlink aliases share storage; different paths stay
separate. Personal configuration and generated data share this root.
Older `--path--` directories are preserved but require
[explicit migration](../architecture/sessions-and-replay.md#legacy-storage).

| Data | Location |
| --- | --- |
| Runtime config | `<home>/harness.jsonc` or `harness.json`, explicit `HARNESS_CONFIG`, and project layers |
| Keyboard config | `<home>/tui.jsonc` or `tui.json`, explicit `HARNESS_TUI_CONFIG`, and project layers |
| Personal agents, commands and prompts | `<home>/agents/`, `<home>/commands/` and `<home>/prompts/` |
| Global skills | `<home>/skills` and `$HOME/.agents/skills` by default; configured by `skills.global_roots` |
| Last picked TUI model | `<home>/model.json`, unless `HARNESS_MODEL_SELECTION_STATE_FILE` overrides it |
| Cached model metadata | `<home>/models-cache.json`, unless `HARNESS_MODELS_PATH` overrides it |
| TUI prompt history | `<session-dir>/tui/prompt-history.json` |
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
Finalized agent snapshots keep exact provider context in memory only. Their
private disk artifacts contain semantic conversation state, redacted and
normalized tool arguments, bounded tool results, read-state hashes and usage
counts. They omit reasoning, raw tool results and complete provider requests.
Resuming after restart uses that semantic state and never repeats historical tools.
Support export redacts registered credentials and recognized secret patterns,
including API keys, bearer tokens, cookies and private-key blocks. It removes
provider stream and reasoning deltas, reasoning parts, thinking summaries and
signatures, and raw tool output JSON. The bundle includes local configuration,
metadata, a replay summary, tool readiness and cleaned events. It does not make
provider or MCP requests. It refuses the export if the final secret scan fails.
It does not promise to remove arbitrary private prose or project instructions.
Review the bundle before sharing it.

`harness trace` writes a local archive with a cleaned `events.jsonl`, `meta.json`
and `support.json`. It omits raw artifacts and uses the same redaction and secret
checks. Use [support export](../operations/sessions.md#export) or a trace archive
instead of sharing the raw session directory.

Review provider and MCP settings before live use. An approved shell command or
eval cell can perform host and network I/O. Permission approval is not OS
confinement, and output redaction does not limit what a provider receives.
