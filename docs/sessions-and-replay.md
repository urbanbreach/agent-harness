# Sessions and replay

Harness sessions are append-only event logs plus redacted artifacts. Replay and session inspection are side-effect free: they read JSONL events in `seq` order and derive projections without executing providers, tools, hooks, MCP, network, or CLI.

## CLI inspection

The operator CLI remains available:

- `harness sessions list`
- `harness sessions inspect <run-id-or-path>`
- `harness sessions continue <run-id-or-path>` when invoked through the interactive resume surface
- `harness sessions export <run-id-or-path>`
- `harness sessions tree|fork|clone`

Support export includes replay-derived session metadata, doctor JSON, config/provider summaries, agent catalog summary, native tool catalog summary, session-tool readiness, route metadata, artifact index, redaction manifest, and secret-scan status.

Typed extension manifests use the same replay boundary. V1 stores and renders
only static descriptor metadata (`extension.manifest.v1`): extension id,
capability ids, disabled capability ids, descriptor counts, and replay labels.
Replay never discovers manifests, loads extension code, registers tools,
executes commands, launches MCP servers, invokes provider decorators, or mutates
session state to render old extension metadata.

## Model-visible session tools

The V1 control plane adds native tools so a model can inspect prior Harness sessions without shelling out to the CLI:

| Tool | Purpose |
|---|---|
| `session_list` | Lists sessions from a workspace-safe session root with optional status/profile/resumable/filter/sort/limit fields. |
| `session_read` | Reads a bounded redacted event/message window by run id or safe path selector. |
| `session_search` | Searches safe replay-derived text such as user messages, assistant summaries, tool summaries, titles, and metadata. |
| `session_info` | Reports replay-derived metadata, lineage, status, event counts, artifact summaries, and recovery notes without dumping whole logs. |
| `session_title_update` | Renames a session to a meaningful operator-provided title; the title is replayed from the `UpdateSessionTitle` event without executing providers or tools. |

All five tools return structured JSON with `source: "event_replay"`, are redacted by default, cap inline output, spill large output to artifacts, and reject traversal or out-of-session-root selectors. The replay-derived session inspection tools do not execute providers, tools, hooks, MCP, network, or CLI. Model tool calls cannot disable redaction unless a future operator-facing policy explicitly adds that ability.

`session_read` exposes independent event and message windows: `eventOffset`/`eventLimit` select replay event summaries, while `messageOffset`/`messageLimit` select user-message and assistant-message summaries from the same replay data. Assistant-message entries expose provider metadata/digests rather than raw assistant payloads.

## Failure boundaries

Corrupt events are reported as parse errors where lossy projection is safe. Missing or ambiguous sessions fail with actionable errors. Replay-only inspection does not prove provider authentication; use a live prompt or live signoff lane for transport/authentication claims.

At the event-store boundary, crash-tail recovery is limited to the final JSONL line while holding the writer lock: a partial unterminated final line is truncated to the previous complete event, and a complete final event missing only the newline terminator is normalized before appends continue. Terminated invalid JSON still fails closed. Recovery reads and repairs the log only; it does not execute providers, tools, hooks, MCP servers, shell commands, or network calls.

Provider-context compaction fallback is observable. When optional model-backed summarization fails, overflows, or returns an invalid structured summary, Harness writes a deterministic checkpoint instead of looping. The checkpoint artifact and `CompactionWritten` event both record `summary_source.deterministic_fallback=true`, and the TUI compaction status includes deterministic fallback text. Overflow retry and failed-response compaction are bounded to one recorded attempt for the triggering request.

## Resume acceptance

Resume rebuilds the transcript, artifacts, pending permission state, todos, plan context, provider context, and meaningful title from append-only events. Recovery must be read-only until the next operator-approved turn starts. A session with a meaningful title should show that title in list/resume surfaces rather than only a run id. The `UpdateSessionTitle` event records operator-initiated renames; replay derives the current title from the latest title event and `session_title_update` allows a model to rename a session without executing providers or tools.

Workspace snapshots are captured automatically before each assistant tool batch and are stored as redacted artifacts. Dotenv-style secret files are omitted from snapshot artifacts and from the corresponding revert scan. `WorkspaceReverted` records that the runtime restored the workspace from a snapshot; it is appended during live execution and replay must not write files or rewrite `events.jsonl`.

The V1 resume acceptance scenario is a realistic interrupted coding session, not a
single empty run. The fixture records multiple user/provider turns. The guarded
anchors are: loaded skill context; todo checklist state; plan handoff context;
tool artifact references; resolved permission grants; post-resume provider turn.
Resume then restores the historical agent binding, transcript, artifact index,
completed tool state, reusable permission grant, todos, plan handoff, and provider
context before accepting the next turn. The replay and resume rebuild remains
side-effect free; the only new side effects are those requested after the resumed
turn starts.

## Lineage: tree, fork, clone

`tree` is a replay-derived view of parent/child lineage. `fork` materializes a child session at a stable source cutoff so in-flight provider/tool work is not copied. `clone` copies the latest stable prefix. Summaries, artifacts, and restored context keep their source cutoff semantics: existing artifact references stay tied to the source session, summaries describe the retained prefix, and new child events append only after the materialized boundary.

Fork/clone behavior is intentionally conservative. If the source cutoff is unstable or artifacts are missing, the command reports the failure instead of executing tools or providers during replay.

Lineage materialization follows the implementation contract in `harness_core::session_lineage`:

- fork materializes an explicitly validated stable prefix. The selected cutoff is recorded as `source_cutoff_seq` in child metadata.
- clone selects the latest stable completed prefix, then records the same `source_cutoff_seq` metadata for the copied boundary.
- Copied events preserve payloads, actors, timestamps, and monotonic times, but `event_id/run_id/seq are regenerated` for the child log. `correlation_id and causation_id are cleared`, and only run-scoped stream keys are rewritten.
- summaries and compaction checkpoints are copied only when copied source events reference them through `CompactionWritten`, `ArtifactWritten`, or tool metadata. Summary text still describes the source prefix that was copied; it is not reinterpreted as new child work.
- Referenced artifacts are `copied after byte and digest validation`. Artifact paths must stay under `artifacts/`, must not traverse symlinks, and missing or mismatched artifacts fail materialization instead of producing a partial child.
- `new child events append after the materialized boundary`. The child replay starts from the rewritten prefix, and future turns add ordinary child-local events after that prefix.
- `restored context is replay-derived from the child log`: resume, session tools, and TUI replay read the child JSONL plus copied artifacts. They do not execute source providers, tools, hooks, MCP servers, shell commands, or network calls to reconstruct fork/clone state.
