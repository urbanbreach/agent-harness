# Runtime architecture

The coordinator owns the run. CLI and TUI adapters submit intents; provider and
tool workers return results. Only the coordinator authorizes work, schedules it,
commits durable events and decides when cancellation or shutdown is complete.

## Crates

| Crate | Responsibility |
| --- | --- |
| `harness` | Clap routing, explicit `CliIo`/`CliDeps`, runtime configuration and command output |
| `harness-core` | Coordinator, permission policy, configuration, journal, projections, sessions and credentials |
| `harness-providers` | HTTP transports, request lowering, stream normalization and provider fixtures |
| `harness-tools` | Native tools, staged file edits, lazy LSP/MCP connections and process cleanup |
| `harness-tui` | Preserved terminal runtime, input, projection and rendering |
| `harness-testkit` | Shared temporary workspaces and preserved terminal test support |

## Turn loop

`coord/turn.rs` lowers the agent context into a provider request. It consumes the
normalized stream, publishes ephemeral progress and commits the completed response.
Tool calls return through the coordinator's permission and capacity checks. Their
ordered results join the context before the next provider request. A response
without tool calls completes the turn.

The loop uses the configured iteration and model budgets. Provider retries cover
classified transient failures; model fallback updates the active model and prompt
family. Unknown model limits remain explicit and use conservative admission.
Compaction checkpoints restore from durable history rather than replaying work.

There is one command inbox, a worker `JoinSet`, and bounded provider/tool capacity.
Accepted tool jobs, including those awaiting permission, cannot exceed the command
buffer plus tool concurrency. A spawn-capable tool releases execution capacity
while awaiting a child. Background children reserve room for parent notifications.
Idle work waits on channels, permits or cancellation; it does not poll.

Interactive runtimes keep journal I/O on the coordinator executor so the terminal
executor can continue handling input. Single-thread callers commit on their own
executor. Cancelling a run cancels descendants, joins workers, closes native
sessions and commits a terminal event before releasing journal writers.

## Durable history

`EventEnvelopeV1` supplies run identity, sequence, timestamp, actor and causal
links. `EventV1` records semantic messages, tool outcomes, permission decisions,
lifecycle transitions, edits and compaction. A journal has one writer and an
append-only JSONL stream. Metadata and artifacts use private atomic publication.

Provider text fragments, reasoning fragments and raw wire payloads are transient.
The coordinator redacts live envelopes before notifying subscribers. Incomplete
words and possible credentials wait in bounded tails until they can be safely
published; text, reasoning and tool input share a 4 MiB response limit. CLI and
TUI subscribers receive the same safe fragments.
Completed tool results contain redacted display text and structured fields. Live
provider context and resumed context use the same representation, including child
session IDs. Attachments use private blobs and digest-checked metadata.

Replay, readiness and inspection read history without executing tools, hooks or
network requests. Crash recovery repairs only a supported incomplete final record;
interior corruption fails closed. Resume reconstructs context and terminates
interrupted work before accepting new turns. Forks copy a validated prefix and
remap child identities; the source journals remain unchanged.

A child journal projects its own events from the authoritative root journal.
Missing or partial projections rebuild during parent resume. Parent ownership
prevents concurrent standalone child writers. Completed child conversation buffers
are released; continuation reloads the needed history. See
[session storage](sessions-and-replay.md) and [tasks](../operations/generic-agent-and-tasks.md).

## Permissions and effects

The agent's tool list is authoritative. Child profiles own their role permissions;
shared project policy remains a ceiling. Delegation does not copy the parent's
role restrictions into the child. A remembered approval cannot override a deny.
Selected files and paths outside the workspace use the same read/edit gates.

Native edits validate the current read fingerprint, prepare private staged output,
format it, then publish a checked replacement and bounded undo baseline. Undo
refuses concurrent editor changes. Shell commands use bounded output, filtered
environments, process groups and joined cancellation. Optional Linux filesystem
confinement is separate from application policy.

MCP and LSP connect after an approved request, reuse their session within the run,
and close on shutdown. Native tools cannot append events or approve themselves.
See [tools](../tools/native-tool-catalog.md), [permissions](../permissions/permissions.md),
and [hooks](../operations/hooks.md).

## Configuration and providers

The loader resolves explicit layers and injected environment values, validates
strict JSON/JSONC contracts, and builds runtime options. Startup catalog selection
is offline. Connected credentials augment missing subscription providers without
replacing configured models. The TUI's compatibility snapshots do not own runtime
configuration.

Provider boundaries normalize Chat Completions, Responses and Anthropic Messages
into the same stream vocabulary. Credential storage and refresh are shared;
refresh cannot resurrect a logged-out account. Current and rotated secrets remain
registered with the run's redactor. Support exports scan before publication and
fail without writing output after a secret finding.

## Finding the code

- Runtime transitions: `harness-core/src/coord/`
- Journal and recovery: `harness-core/src/store.rs` and `store/`
- Replay and catalog: `harness-core/src/session/`, `proj/`, `session_lineage/`
- CLI bootstrap and prompts: `harness/src/bootstrap.rs`, `runtime_catalog.rs`, `prompt/`
- Provider wire formats: `harness-providers/src/`
- Native execution: `harness-tools/src/`

### Terminal UI

[`harness-tui`](../../crates/harness-tui/src/lib.rs) renders the Ratatui interface:

It subscribes to coordinator events in live mode and reads recorded sessions in
replay mode. Permission dialogs collect decisions, diff views display file
changes, and the transcript groups related tool and provider output.

### Terminal presentation

Compaction streams on one status row: an animated accent-colored spinner, a reason label, an Escape
cancel hint, and the trailing summary preview. Previews are redacted live events, never journaled.
Generation identities reject late chunks, and replay never resurrects a spinner. Completed checkpoints
show `[compaction]` and the comma-separated prior token count. Ctrl+Alt+O expands/collapses the Markdown
summary; the existing Ctrl+O permission action retains its binding.

## Input-first TUI runtime scheduling

Pane focus transitions live in `app/focus.rs`; keyboard and overlay dispatch keep
their priority in `app/key_interaction.rs`. Startup transitions update the welcome
selection, and closing Help restores its saved focus. Replay consumes default Tab
keys before action dispatch, while a remapped reverse-focus action can reach a
visible terminal pane.

Interactive input has one producer: a terminal-reader thread feeds a bounded 128-event FIFO. The
runtime arbiter orders fatal writer failure, frame acknowledgement, quit/cancel, terminal input,
pacer and animation deadlines, then live provider updates. An input quantum is bounded to 16
terminal envelopes or 2 ms; fairness permits live progress without reordering input. Live work
retains a 16-update / 2 ms budget boundary. Input and provider bursts share a 4 ms default flush
cadence, configurable through `HARNESS_TUI_MIN_DRAW_MS` (1 to 100 ms). Resize coalescing also uses
4 ms. Fast visible motion follows the configured cadence; discrete spinner and background
glyphs retain their slower wall-clock periods. Scroll gesture classification retains its 80 ms
window. The writer keeps at most one frame in flight, and completed acknowledgements are
retired even when telemetry is disabled. Idle state without visible motion parks.

On Unix, the reader waits for terminal input, SIGWINCH or its shutdown wake pipe
without a periodic timeout. Shutdown signals the pipe and joins the owned thread
before restoring terminal state; dropping the reader uses the same path. A narrow
Crossterm patch exposes its existing parser and wake pipe without constructing an
`EventStream` worker. Partial input returns to the readiness wait, and EOF or
descriptor failures reach the runtime as typed reader errors. Other platforms
retain the timed reader path.

Runtime scheduling QA exercises typing, wheel input, disclosure open/close, resizes, and semantic
cancellation while live work remains pending. Its
Harness-only scheduling sidecar records decisions, depths, preemptions, deadlines, action IDs, and
cause IDs; it does not contain provider or terminal text. Both runtimes remain observable through
`external_pty_observed`; only Harness may claim `native_completed_write` after write and flush.

The session CLI and model-visible session tools both consume replay-derived
projections. Support export adds local-readiness evidence from doctor plus agent
catalog, native tool catalog, session-tool readiness, route metadata, artifact
index, redaction manifest, and secret-scan status so failures can be debugged
without exposing raw credentials.
