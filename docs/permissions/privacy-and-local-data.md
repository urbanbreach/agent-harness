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

| Data | Location |
| --- | --- |
| Runtime config | XDG or project `harness.json` and `harness.jsonc` files |
| Keyboard config | XDG or project `tui.json` and `tui.jsonc` files |
| Project prompts and skills | `.agent-harness/agents` and `.agent-harness/skills` |
| Events and artifacts | The configured session directory and its per-run directories |
| Stored credentials | `credentials/<authProvider>.json` under the platform data directory |

Use `harness config sources` to find active configuration files. Credential files
use restrictive permissions. Logout removes stored credentials, but leaves
configured environment and inline credential fallbacks in place.

## Redaction and sharing

The redactor lives in [`crates/harness-core/src/redact.rs`](../../crates/harness-core/src/redact.rs).
Support export redacts API keys, bearer tokens, cookies, PEM blocks, provider
credentials, and hidden prompt or config instruction values. It includes a
redaction manifest and refuses to write a bundle if the final secret scan fails.
Use [support export](../architecture/sessions-and-replay.md#cli-inspection)
instead of sharing raw `events.jsonl`.

V1 has no telemetry, cloud analytics, billing, web sharing, or hosted
collaboration. Review provider and MCP settings before live use. An approved
shell command can perform host I/O; permission approval is not OS confinement.
