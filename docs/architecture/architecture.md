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
| `harness-eval` | Persistent language kernels, cell lifecycle, output retention and host callbacks |
| `harness-tui` | Preserved terminal runtime, input, projection and rendering |
| `harness-testkit` | Shared temporary workspaces and preserved terminal test support |

## Turn loop

`crates/harness-core/src/coord/turn.rs` queues turns and starts workers.
`coord/turn/request.rs` builds provider requests; `coord/turn/worker.rs` runs
the request/tool loop. `coord/streaming.rs` and `coord/streaming/` consume the
normalized stream through coordinator calls, publish ephemeral progress and
commit the completed response. Tool calls return through the coordinator
permission and capacity checks. Results join the context in call order before
the next provider request. A response without tool calls completes the turn.

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

The `monitor` tool prepares a command or file watch in
`crates/harness-tools/src/shell/monitor.rs`. The coordinator owns its background
task and wakeups in `crates/harness-core/src/coord/commands/monitor.rs`. Command
monitors use a pseudo-terminal; file monitors poll a path. Matching lines become
bounded `monitor_event` reminders through `coord/reminders.rs`, subject to
`runtime.behavior.command_notifications`. Repeated unproductive wakeups pause
command monitors until they are rearmed or the user submits input.

Eval sessions live in `crates/harness-eval/src/session.rs`. Native integration
is in `crates/harness-tools/src/eval.rs` and `eval/`; nested tool and provider
callbacks return through `crates/harness-core/src/coord/eval.rs` and `coord/eval/`.
Kernels do not authorize host operations or append journal events.

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

`edit.mode` and `edit.model_variants` select `apply_patch`, `replace` or
`hashline` editing for each model. `auto` resolves a model-family preference.
Selection lives in `crates/harness-core/src/tool/edit_mode.rs` and respects the
agent editing tools. Tool definitions and read output use the selected mode;
publication still crosses the coordinator edit boundary.

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

HTTP providers normalize Chat Completions, Responses and Anthropic Messages
into the same stream vocabulary. The Claude subscription provider runs a local
Claude Code subprocess and can own its conversation and compaction. The
`Provider::manages_context` contract disables Harness automatic compaction and
input-budget stopping for those requests. Credential storage and refresh are shared;
refresh cannot resurrect a logged-out account. Current and rotated secrets remain
registered with the run's redactor. Support exports scan before publication and
fail without writing output after a secret finding.

## Finding the code

- Runtime API and transitions: `crates/harness-core/src/coord.rs`, `crates/harness-core/src/coord/`
- Event contracts: `crates/harness-core/src/event.rs`, `crates/harness-core/src/event/`
- Configuration: `crates/harness-core/src/config.rs`, `crates/harness-core/src/config/`
- Journal and recovery: `crates/harness-core/src/store.rs`, `crates/harness-core/src/store/`
- Replay and catalog: `crates/harness-core/src/session/`, `crates/harness-core/src/proj/`, `crates/harness-core/src/session_lineage/`
- CLI bootstrap and prompts: `crates/harness/src/bootstrap.rs`, `crates/harness/src/runtime_catalog.rs`, `crates/harness/src/prompt/`
- Provider contract and events: `crates/harness-providers/src/lib.rs`, `crates/harness-providers/src/types.rs`
- Native execution and registry: `crates/harness-tools/src/lib.rs`, `crates/harness-tools/src/`
- Code kernels: `crates/harness-eval/src/`
- Shared test workspaces: `crates/harness-testkit/src/workspace.rs`

### Terminal UI

[`harness-tui`](../../crates/harness-tui/src/lib.rs) renders the Ratatui interface:

It subscribes to coordinator events in live mode and reads recorded sessions in
replay mode. Permission dialogs collect decisions, diff views display file
changes, and the transcript groups related tool and provider output.

`AppState` is defined in `crates/harness-tui/src/app.rs`. Concern-specific
modules in `app/` update its state. `render_app` in `crates/harness-tui/src/ui.rs`
composes a frame without submitting intents. `run_tui_with_options` in
`crates/harness-tui/src/runtime.rs` owns terminal setup, input, live-update
ingestion, frame output and teardown. CLI live adapters translate UI intents
into coordinator calls.

### Terminal presentation

Compaction streams on one status row with an animated spinner, a reason label,
a Ctrl+C cancel hint and the trailing summary preview. Previews are redacted live
events, never journaled. Generation identities reject late chunks, and replay
does not restore an active spinner. Completed checkpoints show `[compaction]`
and the prior token count. Ctrl+Alt+O expands or collapses the Markdown summary.
The Ctrl+O permission action retains its binding.

The coordinator prices each provider request as it finishes, from the model
catalog and the request's start time, and saves the amount as
`cost_picodollars` on `ProviderRequestFinished`. Session usage in
`crates/harness-tui/src/app/session_usage.rs` adds saved amounts by agent and
reprices only requests recorded without one, with today's catalog. It displays
live token speed, the main agent's spend and its subagents' spend; a `+` suffix
marks usage without a known price and `(sub)` marks an OAuth subscription
session. `configs/pricing-overrides.json` holds peak schedules models.dev cannot
express. Each applies, while the catalog is parsed, only while models.dev still
lists the price it was checked against; otherwise it adds a catalog diagnostic.

## Input-first TUI runtime scheduling

Pane focus transitions live in `crates/harness-tui/src/app/focus.rs`; keyboard
and overlay dispatch live in `app/key_interaction.rs`. Startup transitions
update the welcome selection, and closing Help restores its saved focus. Replay
consumes default Tab keys before action dispatch, while a remapped reverse-focus
action can reach a visible terminal pane.

A terminal-reader thread feeds a bounded 128-event FIFO. The runtime handles
shutdown, writer failure and reader failure before draining a bounded input
batch. Each batch contains at most 16 terminal envelopes or 2 ms of input work.
Live events and due frames get a turn after that batch; submit, resize and click
events form a paint barrier before provider work. Live updates also have a
16-update / 2 ms budget. Input and provider bursts share a 4 ms default flush
cadence, configurable through `HARNESS_TUI_MIN_DRAW_MS` (clamped to 1 to 100 ms).
The writer keeps at most one frame in flight. The runtime retires frame
acknowledgements even when telemetry is disabled and waits on channels or the
next deadline when no work is ready.

On Unix, the reader waits for terminal input, SIGWINCH or its shutdown wake pipe
without a periodic timeout. Shutdown signals the pipe and joins the owned thread
before restoring terminal state; dropping the reader uses the same path. A narrow
Crossterm patch exposes its existing parser and wake pipe without constructing an
`EventStream` worker. Partial input returns to the readiness wait, and EOF or
descriptor failures reach the runtime as typed reader errors. Other platforms
retain the timed reader path.

Local scheduling telemetry records runtime decisions without provider or
terminal text. Presentation telemetry records frame acknowledgements. During
shutdown, the runtime stops and joins the reader, drains pending frame output
and joins the writer before restoring terminal state.

The session CLI and model-visible session tools consume replay-derived
projections. Support export includes configuration, provider and agent summaries,
the native tool catalog, session-tool readiness and secret-scan status. Its
readiness scope is local configuration, not proof of provider execution. It omits
raw tool results and scans for secrets before publication.
