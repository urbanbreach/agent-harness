# Sessions and replay

Harness sessions are append-only event logs plus redacted artifacts. Replay and session inspection are side-effect free: they read JSONL events in `seq` order and derive projections without executing providers, tools, hooks, MCP servers, shell commands, or network calls.

## CLI inspection

The operator CLI remains available:

- `harness sessions list`
- `harness sessions inspect <run-id-or-path>`
- `harness sessions replay <run-id-or-path>`
- `harness sessions export <run-id-or-path>`
- `harness sessions tree|fork|clone`

Support export includes replay-derived session metadata, doctor JSON, config/provider summaries, agent catalog summary, native tool catalog summary, session-tool readiness, route metadata, artifact index, redaction manifest, and secret-scan status.

## Model-visible session tools

The V1 control plane adds native tools so a model can inspect prior Harness sessions without shelling out to the CLI:

| Tool | Purpose |
|---|---|
| `session_list` | Lists sessions from a workspace-safe session root with optional status/profile/resumable/filter/sort/limit fields. |
| `session_read` | Reads a bounded redacted event/message window by run id or safe path selector. |
| `session_search` | Searches safe replay-derived text such as user messages, assistant summaries, tool summaries, titles, and metadata. |
| `session_info` | Reports metadata, lineage, status, event counts, artifact summary, team projection summary, and recovery notes for one session. |

All four tools return structured JSON with `source: "event_replay"`, are redacted by default, cap inline output, spill large output to artifacts, and reject traversal or out-of-session-root selectors. Model tool calls cannot disable redaction unless a future operator-facing policy explicitly adds that ability.

`session_read` exposes independent event and message windows: `eventOffset`/`eventLimit` select replay event summaries, while `messageOffset`/`messageLimit` select user-message and assistant-message summaries from the same replay data. Assistant-message entries expose provider metadata/digests rather than raw assistant payloads.

## Failure boundaries

Corrupt events are reported as parse errors where lossy projection is safe. Missing or ambiguous sessions fail with actionable errors. Replay-only inspection does not prove provider authentication; use a live prompt or live signoff lane for transport/authentication claims.
