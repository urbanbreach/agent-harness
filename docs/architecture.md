# Architecture

This document describes the high-level architecture of Agent Harness.

## Crate Boundaries

### harness (binary crate)

The CLI entry point. Contains subcommand implementations:

- `harness run` - Headless scenario execution
- `harness tui` - Interactive terminal UI
- `harness replay` - Session replay and inspection
- `harness schema` - JSON Schema output
- `harness config validate` - Config validation
- `harness sessions list` - List recorded sessions

### harness-core (library)

Core runtime and domain logic:

- **event/** - Event schema v1, envelope types, event builder
- **store/** - Event store trait, in-memory and JSONL file implementations
- **coord/** - Coordinator actor (single scheduling authority)
- **sched/** - Scheduler with concurrency slots and stale detection
- **perm/** - Permission engine (allow/deny/ask)
- **tool/** - Tool framework and capability gating
- **edit/** - Hashline edit engine
- **proj/** - Pure projections for run summary, resume planning, and session catalog state
- **transcript_projection** - Pure replay-derived transcript/session/message/part projection for resume, export, TUI, and debugging surfaces
- **agent/** - Minimal agent runtime
- **config** - Configuration parsing and validation
- **clock** - Clock abstraction (real and fake for determinism)
- **redact** - Secret redaction before persistence

### Agent prompt assets and instructions

Interactive agent runtime settings still come from structured config, but prompt bodies are now resolved separately from markdown assets and project instructions:

- Built-in `build` and `plan` use runtime-synthesized dynamic prompts when their shipped markdown assets contain only frontmatter.
- `.agent-harness/agents/<agent>.md` can provide a file-backed prompt body for custom agents or local overrides.
- inline `system_prompt` in config remains a compatibility override and wins over the markdown body.
- `AGENTS.md` is loaded as a separate project-instruction layer and composed into the final runtime system prompt.

This keeps config focused on structured behavior while allowing the built-in agent prompts to adapt to model, workspace, project-instruction, and skill context.

### harness-providers (library)

Provider abstraction for LLM completions:

- `Provider` trait for streaming completions
- `MockProvider` - Deterministic offline provider using request digest fixtures
- `OpenAiCompatibleProvider` - HTTP/SSE streaming to OpenAI-compatible endpoints

### harness-tools (library)

Built-in tool implementations:

- `read` / `list` / `glob` / `grep` - Safe workspace discovery and search
- `edit` - Hashline-first file creation, targeted edits, deletion, and rename
- `bash` - Execute shell commands with allowlist
- `task` / `background_output` / `batch` / `question` / `skill` - Control-plane and delegation workflows
- `team_create` / `team_status` / `team_send_message` / `team_task_*` / `team_shutdown_*` / `team_delete` - Event-sourced team coordination workflows
- `task_create` / `task_list` / `task_get` / `task_update` - Replay-projected persistent dependency tasks
- `interactive_bash` / `terminal_*` - Tmux-backed persistent terminal sessions with explicit dependency errors when tmux is unavailable
- `look_at` - Replay-safe media/text summary extraction with an explicit `multimodal-looker` route for visual interpretation
- `webfetch` / `websearch` / `codesearch` / `lsp` - Network and language-intelligence workflows

Hashline editing is the only normal file-changing route. Agent profiles expose `read`
and `edit`; low-level hashline scan/apply helpers are reserved for internal
compatibility and focused test lanes.

The active registry exposes a single native provider surface. Canonical ids such as
`read`, `edit`, `bash`, `webfetch`, `websearch`, `codesearch`, `question`, `batch`, `task`,
`background_output`, `team_*`, `terminal_*`, `look_at`, and `lsp` are the documented tool surface, while lower-level
executors remain internal implementation details behind those tool ids.

### harness-tui (library)

Ratatui-based terminal interface:

- Live mode: subscribe to coordinator events
- Replay mode: inspect recorded sessions
- Permission modal: interactive allow/deny
- Diff viewer: display hashline edit diffs
- Grouped streams: tool and provider event grouping

### harness-testkit (library)

Test utilities and fixtures:

- Mock provider fixtures
- Test helpers for deterministic runs
- PTY E2E harness (portable-pty + vt100)

## Event Schema v1

Events are the source of truth. All state is derived from events.

### Envelope Structure

```json
{
  "schema_version": 1,
  "event_id": "uuid",
  "seq": 42,
  "run_id": "uuid",
  "mono_ms": 12345,
  "ts": "2024-01-15T10:30:00Z",
  "actor": {"kind": "Supervisor", "agent_id": null},
  "correlation_id": "tool-call-uuid",
  "causation_id": "prev-event-uuid",
  "stream_key": "agent-1",
  "payload": {"RunStarted": {...}}
}
```

### Event Types

**Lifecycle**
- `RunStarted` / `RunFinished` / `RunFailed`
- `SessionTitleUpdated` - Harness-compatible generated session title persisted after the first real user prompt when a default title is still present
- `AgentSpawned` / `AgentStopped`

**Task Management**
- `TaskScheduled` - Includes state: queued or started
- `TaskCancelled` - Best-effort cancellation
- `TaskCompleted` - Normal completion
- `TaskResultLate` - Result arrived after cancellation
- `BackgroundTaskNotification` - Durable parent wakeup record for a `task(run_in_background=true)` child request after the child reaches a terminal state; carries parent/child ids, terminal status (`completed`, `cancelled`, `failed`, or `timed_out`), capped summary, terminal event id, and delivered parent turn request id. Replay projects this event only and must not schedule provider work.
- `PersistentTaskCreated` - Creates a durable dependency-aware work item for `task_create`
- `PersistentTaskUpdated` - Mutates persistent task status, owner, active form, dependencies, subject/description, and metadata after coordinator validation

**Progress and Staleness**
- `StaleDetected` - Task exceeded staleness timeout
- `UserMessageSubmitted` - User prompt accepted into the event stream

Background child-task completion wakeups are coordinator-owned. The child terminal
`TaskCompleted` / `TaskCancelled` event is written first, then the coordinator appends one
`BackgroundTaskNotification` per background child request and queues a parent
system-reminder turn through the same agent scheduling path used for normal turns. Sync child
tasks do not emit this notification because their result is returned directly through the `task`
tool response. Notification summaries are capped; full child history remains available through
`background_output` and the event/artifact log. `background_output` resolves lineage, status,
cancellation targets, and late-result markers through coordinator-owned replay projection rather
than a tool-local background manager or in-memory task handle, so the same child request remains
observable after coordinator resume.
Child-agent `TaskCompleted` metadata carries the resolved task route when the turn was spawned
through `task`: requested profile/category, resolved profile/category, catalog role/binding,
display order, effective model ref, redelegation capability, fallback marker, and loaded skills.
Replay and session inspection consume this metadata only; they do not re-run routing logic.
Replay and TUI surfaces render child-session next actions from the same projected ids: terminal
children point at `background_output(request_id=...)` for full details and `task(session_id=...)`
for deliberate continuation, while non-terminal children also show non-blocking/blocking status
checks without scheduling work during replay.

**Provider Streaming**
- `ProviderRequestStarted`
- `ProviderStreamDelta` - Text chunk
- `ProviderReasoningDelta` - Reasoning/thinking chunk
- `ProviderRequestFinished`
- `AssistantMessageFinished` - Assistant message committed before tool preflight/execution
- Provider tool-call deltas/completions are normalized before coordinator execution
- `CompactionRequested` / `CompactionWritten` / `CompactionApplied` / `CompactionFailed` - provider-context checkpoint lifecycle; written/applied events carry additive active-context estimate metadata so projections can separate active context from cumulative token spend.

### Provider lifecycle metadata contract

The durable provider lifecycle barriers are `ProviderRequestStarted` and `ProviderRequestFinished`.
Replay, resume, and audits may rely on their ordered presence, shared `request_id`, provider/model
ids, redacted prompt summary, request digest, finish reason, output digest, and aggregate usage.
`AssistantMessageFinished` is the separate durable assistant-message boundary: it is appended after
the coordinator commits the completed assistant response to provider-visible message state and before
tool preflight or execution begins. These barriers also accept optional metadata objects. Metadata
fields are additive, serde-defaulted for old logs, and ignored for semantic replay decisions except
where projections surface them as optional inspection data.

The following state is derived from the event stream, not stored as separate semantic barriers:

- reasoning display boundaries, from provider start/finish plus accumulated reasoning deltas,
- tool-call readiness, from normalized provider tool-call events before coordinator execution,
- loop continuation, from finished provider request, executed tool results, and guardrail state,
- provider stream chunk grouping, from adjacent delta events with the same `request_id`.

Provider metadata is optional and non-semantic for old logs. Missing metadata must not change replay
equivalence. When implementation needs provider metadata, add it to `ProviderRequestStarted` or
`ProviderRequestFinished` as optional redacted fields before adding any new event variant.

Field decisions:

| Metadata | Durable location | Contract |
|----------|------------------|----------|
| Provider call or response id | Optional start/finish `metadata.provider_call_id` or `metadata.provider_response_id` | Store only redacted ids useful for audit correlation. Never treat provider ids as coordinator scheduling keys. |
| Stable turn/request correlation | Existing envelope `correlation_id`, provider `request_id`, and optional `metadata.turn_id` | Durable. Use harness-owned ids for replay and resume. Provider ids are advisory only. |
| Provider session or cache key | Optional start/finish `metadata.provider_session_id` / `metadata.provider_cache_id` | Store redacted summaries or digests only when needed for cache inspection. Missing values are normal. |
| Stop reason | Existing `finish_reason`; optional finish `metadata.provider_stop_reason` | Durable as a summary string. Provider-specific raw finish payloads are omitted. |
| Usage and cache read/write counts | Existing `usage`; optional finish `metadata.cache_read_tokens` / `metadata.cache_write_tokens` | Durable aggregate accounting. Counts are advisory and must be safe to omit from old logs. |
| Assistant message barrier ids/digests | `assistant_message` on `AssistantMessageFinished`; compatibility-only mirror in optional finish `metadata.assistant_message` | Carries redacted message ids or text/reasoning digests for audit boundaries. New logs should use `AssistantMessageFinished` as the explicit assistant boundary; old logs may only have the provider-finish metadata mirror. |
| Thinking or reasoning signatures | Optional finish `metadata.thinking` | Store only summaries, digests, or signature ids. Never store raw hidden thinking text. |
| Runtime fallback telemetry | Optional start/finish `metadata.fallback_attempt`, `metadata.fallback_from_model_ref`, `metadata.fallback_reason_class`, and `metadata.fallback_retryable`; optional finish `metadata.provider_error_class` / `metadata.provider_error_retryable` | Records why a configured fallback target was tried. Replay still derives behavior from the event sequence and never re-runs fallback decisions. |
| Provider payloads and secrets | Never durable | Raw requests, raw responses, auth headers, and unredacted reasoning are excluded from event logs. |

**Tool Execution**
- `ToolCallRequested`
- `ToolCallStarted`
- `ToolCallFinished`

**Permissions**
- `PermissionRequested` - User intervention required
- `PermissionGrantRecorded` - Durable allow-always grant recorded for matching future requests in the event log
- `PermissionResolved` - Allow or deny decision recorded

**Editing**
- `EditProposed` - Edit prepared for review
- `EditApplied` - Edit successfully committed
- `EditRejected` - Edit failed (mismatch, denied, etc.)

**Artifacts and Policy**
- `ArtifactWritten` - File stored to session
- `PolicyViolationDetected` - Security rule triggered

**Persistent Tasks**

Persistent tasks are separate from scheduler tasks and team checklist tasks. The
`task_create`, `task_list`, `task_get`, and `task_update` compatibility tools append or
read `PersistentTaskCreated` / `PersistentTaskUpdated` events through the coordinator.
State is projected from the current run event log, so task state survives restart/resume
and session replay does not execute tools. Task payloads keep OMO/Claude-compatible
fields (`subject`, `description`, `status`, `active_form`, `blocked_by`, projected
`blocks`, `owner`, `metadata`, and `run_id` / `thread_id`). Callers provide `blocked_by`;
replay recomputes `blocks` deterministically. Coordinator validation rejects duplicate
ids, unknown dependencies, self-dependencies, dependency cycles, and moving a task to
`claimed`, `in_progress`, or `completed` while any blocker is incomplete. `task_list`
also returns pending unblocked `ready_task_ids` for orchestrators such as Atlas, Team
Mode, and continuation loops; execution remains coordinator-owned and is never started
by replay projection.

**Team Orchestration**
- `TeamCreated` - Creates an event-sourced team run from a typed team spec, explicit member roles, optional lead selector, and bounds
- `TeamMemberSpawned` - Links a team participant name to an ordinary coordinator-spawned child agent session; `member_name = "lead"` records the first-class lead runtime when configured
- `TeamMessageSent` - Appends a shared team message, announcement, or shutdown notice
- `TeamTaskCreated` - Adds a shared team checklist task, separate from scheduler `TaskScheduled` work
- `TeamTaskUpdated` - Mutates shared team task status, owner, and metadata after coordinator validation
- `TeamShutdownRequested` - Records a member shutdown request
- `TeamShutdownApproved` - Records approval for a member shutdown request
- `TeamShutdownRejected` - Records rejection and reason for a member shutdown request
- `TeamDeleted` - Marks a fully shutdown-approved team run deleted

Team orchestration state is replayed from these events by pure projections. The stable role model is
operator/supervisor, lead, write-capable member, and read-only research member. The team spec lead is
resolved and preflighted before `TeamCreated`; when present, it is spawned and projected separately
from ordinary members. The team member role defaults to `member`; `research` allows read-only
profiles to participate while coordinator validation denies team mailbox/task mutations for that
role. Members remain ordinary child agents and their provider/tool work remains represented by the
existing agent, task, tool, and background-notification events. The shared team task list is a
coordination checklist and must not be confused with scheduler task ids. Team message and checklist
timestamps are the enclosing event envelope timestamps. `blocks` is a projection-derived inverse of
`blocked_by`; callers provide `blocked_by`, and replay recomputes `blocks` deterministically.
Shutdown approval/deletion is a team coordination protocol; it does not by itself execute provider
work, stop child sessions, or cancel scheduler tasks.

Team bounds are runtime policy, not validation-only fields. `max_parallel_members` limits active
non-lead member sessions; pending members activate after another active member is shutdown-approved.
`max_member_turns` counts non-shutdown member writes from replayable team events and blocks further
mailbox/task work after the bound. `max_wall_clock_minutes` blocks non-shutdown team writes after the
deadline while still allowing shutdown and deletion cleanup. Duplicate team message ids and task ids
are rejected by the coordinator; projections keep first-seen state if old logs contain duplicates.

**Workflow Orchestration**
- `WorkflowStarted` - Starts a durable workflow run with stable workflow id, mode, owner, optional lane/title, and optional idempotency key.
- `WorkflowTransitionRecorded` - Records an accepted workflow status transition with previous/current status context, owner, reason, and optional policy/idempotency metadata.
- `WorkflowTransitionDenied` - Records denied workflow lifecycle evidence such as owner conflicts without mutating the active workflow projection.
- `WorkflowEvidenceRecorded` - Attaches typed verification or signoff evidence to a workflow, optionally linking a redacted artifact path/digest and acceptance reference.
- `WorkflowOperatorDecisionRecorded` - Records an operator decision with operator id, reason, and optional correlation id.
- `WorkflowCompleted` - Marks a workflow terminal with final outcome, reason, and owner.

Workflow state is replayed by the pure `workflow` projection. Old logs with no workflow events
project to empty workflow state. Start decisions are idempotent by idempotency key and by same-owner
duplicate workflow id; conflicting owners append denied-transition evidence instead of rewriting the
existing run. Workflow status, dossier, and replay readers consume projections only and must not
append events. Continuation events may carry optional workflow metadata so bounded continuation loops
can be associated with a workflow lane/iteration/stop reason without changing old-log semantics.
Context snapshots use this workflow evidence path: the coordinator writes a redacted/capped
`artifacts/context_snapshots/<snapshot-id>.json` artifact, appends `ArtifactWritten`, then appends
`WorkflowEvidenceRecorded` using the `evidence.context_snapshot` category with snapshot id, slug,
ambiguity score, artifact path, and digest metadata. Replay projects these refs without reading live
workspace files; artifact write failure prevents the workflow evidence event. The CLI write path
`harness workflow snapshot write` uses the same coordinator command. The workflow command
foundation also exposes `run`, `status`, `signoff`, `cancel`, `dossier`, `snapshot`, and `init`:
mutating commands append through coordinator command handlers, while status/dossier/snapshot reads
derive from event projections only and do not append events.

**Continuation**
- `ContinuationStarted` - Starts an explicit, bounded continuation loop from a slash command or tool action. The event records the stable continuation id, mode, originating command, and max iteration/wall-clock/provider/tool-call bounds.
- `ContinuationReminderQueued` - Records a persisted continuation reminder and iteration count. Replay renders the reminder but never schedules provider work.
- `ContinuationStopped` - Stops the active continuation through `/stop-continuation`, `/cancel-ralph`, user interruption, or done-marker detection.
- `ContinuationLimitReached` - Stops continuation after a configured bound is reached.

Continuation state is coordinator-owned and replay-derived. Resume projections restore the active
continuation id from the event stream so a restarted coordinator can show and stop an already-started
loop without relying on in-memory state.

**UI Intent**
- `UiIntentReceived` - Live UI intent recorded before coordinator handling

## Coordinator Invariants

The Coordinator is the single authority for:

1. **Event appending** - Only the Coordinator calls `EventStore::append`
2. **Task scheduling** - All background work goes through Coordinator commands
3. **Permission resolution** - Coordinator evaluates policies and emits resolution events
4. **State transitions** - Run and agent lifecycle managed centrally

### Concurrency Model

```
┌─────────────┐     Command (mpsc)     ┌─────────────┐
│   Clients   │ ───────────────────────> │ Coordinator │
│ (Agents/UI) │                        │             │
└─────────────┘                        │ ┌─────────┐ │
       │                               │ │  Slot   │ │
       │                               │ │  Gates  │ │
       │                               │ └─────────┘ │
       │                               │ ┌─────────┐ │
       │                               │ │  Task   │ │
       │                               │ │  Queue  │ │
       │                               │ └─────────┘ │
       │                               └──────┬──────┘
       │                                      │
       │  Event (broadcast)                     │ Spawn
       │<──────────────────────────────────────┘
       │
┌──────┴──────┐
│ EventStore  │
│ (JSONL)     │
└─────────────┘
```

### Key Behaviors

- **Cancellable tasks**: Every background job has a `CancellationToken`
- **Late results**: If a task reports after cancellation, record `TaskResultLate` and discard side effects
- **Slot gates**: Coordinator-managed counters avoid semaphore-in-select cancellation unsafety
- **Stale watchdog**: Periodic checks for unresponsive tasks based on progress heartbeats

## Permission Model

The native permission taxonomy is capability- and family-aware. The canonical public buckets are:

| Permission | Tool Capability | Policy Options |
|------------|-----------------|----------------|
| `edit` | `EditFs` | allow / deny / ask |
| `bash` | `Shell` | allow / deny / ask |
| `question` | interactive user question / confirmation flow | allow / deny / ask |
| `task` | task / agent orchestration flow, including team coordination tools | allow / deny / ask |
| `webfetch` | `webfetch` | allow / deny / ask |
| `websearch` | `websearch` | allow / deny / ask |
| `codesearch` | `codesearch` | allow / deny / ask |
| `lsp` | `lsp` / `lsp.rename` | allow / deny / ask |

Legacy `shell` and `network` names remain migration-only compatibility aliases. User-facing configs
should use the canonical public names above.

### Policy Resolution

1. Check global defaults from config
2. Check per-agent overrides
3. Apply decision:
   - `allow` - Proceed immediately
   - `deny` - Emit `PermissionResolved(deny)` and fail
   - `ask` - Check active coordinator-owned durable grants rebuilt from `PermissionGrantRecorded`; if none match, emit `PermissionRequested` and pause until a resolve command

Static configured `deny` is final and is checked before durable grants, so a replayed allow-always grant can satisfy future `ask` decisions but never overrides policy denial. Allow-always decisions record run-scoped grants by default, with explicit scope and optional expiry fields for future extension. Grant matchers persist only redacted-safe selectors: canonical/effective tool id, permission kind, a semantic shell command digest or workspace-relative edit path when available, and request-digest fallback for exact matching.

### Headless Mode

In headless scenarios, `ask` defaults to `deny` unless the scenario script explicitly sends `ResolvePermission(Allow)`.

### Anti-Footgun: No Redelegation

Workers cannot call direct coordinator spawn APIs. Only `ActorKind::Supervisor` may call `SpawnAgent`. Violations emit `PolicyViolationDetected`.

## Coordinator-owned Agent Turn Loop

Agent turns are coordinator-owned state machines. Provider helpers may transform context and stream
one assistant response, but they do not decide task scheduling, append events directly, or execute
tools on the production coordinator path. The turn loop runs through explicit phases:

1. **Turn start** - the coordinator records the running turn, lifecycle hook state, cancellation
   token, scheduler slot, and stable turn/request correlation id.
2. **Context projection and provider transform** - provider-visible messages are recomputed at
   provider-start time from event-derived context plus any applied checkpoint. Queued turns do not
   carry stale scheduled-time provider input.
3. **Provider stream** - the coordinator allocates a fresh provider-call id, invokes the single-call
   provider primitive, and receives provider lifecycle/text/reasoning/tool-intent events through
   coordinator commands.
4. **Assistant-message barrier** - `ProviderRequestFinished` closes provider streaming, then
   `AssistantMessageFinished` is appended and acknowledged after the assistant response is committed
   to coordinator message state. Its optional metadata carries non-semantic assistant-message digests
   for audit/debugging.
5. **Tool preflight and execution** - parsed tool intents are mapped back to canonical tool ids and
   re-enter the coordinator through `ExecuteAgentToolCall`, so permission checks, scheduler slots,
   artifacts, redaction, cancellation, and late-result handling stay on the same path as native tool
   calls.
6. **Tool-result projection** - completed tool results are appended to the next provider request as
   tool-role messages in assistant source order.
7. **Turn end** - the agent turn reaches a terminal task lifecycle event, freeing scheduler slots
   for any separately queued turns.

JSONL lifecycle events remain chronological append-time records. A parallel tool batch can therefore
emit `ToolCallFinished` events in completion order while the next provider request receives the
model-visible tool-result messages in the assistant's original source order. Replay and audits should
treat chronological JSONL order as the source of truth for what happened, and the pure conversation
projection as the source of truth for provider model context.

Prompt-mode completion follows the same lifecycle contract: a provider finish is only the assistant
message barrier, while the CLI waits for the correlated agent-turn `TaskCompleted` or `TaskCancelled`
terminal event before reporting completion. The `task` and `batch` tools also preserve coordinator
re-entry: child turns are requested or resumed through coordinator scheduling, never through a direct
agent/provider loop bypass.

Guardrails bound tool-heavy turns by total tool calls per turn, while provider phases continue until
the assistant completes, fails, is cancelled, or hits that explicit tool-call cap. Overflow-style
provider failures may trigger one coordinator compaction retry; the retry recomputes provider context
from the checkpoint without rewriting `events.jsonl`. Pre-prompt compaction uses the same coordinator
checkpoint path before provider request construction, with deterministic token estimates and a no-loop
guard when a checkpoint cannot reduce active context.

## Tool Surface Policy

Provider and tool exposure is selected per agent by its configured `tools` list. The harness ships
a single native tool surface, so profiles opt in by naming canonical tool ids such as `read`,
`edit`, `bash`, `task`, `background_output`, and `plan_exit` directly. The shipped `plan` profile
includes `edit` only for the active workspace-relative `.agent-harness/plans/<run>.md` file through
runtime permission rules, exposes `bash` only behind shell permission and an additional runtime
read-only inspection guard, may delegate read-only exploration only through the `explore` profile via
`task`/`background_output`, and uses `plan_exit` approval before the coordinator schedules a `build`
continuation with the active plan-file path. By default, `read` emits
`LINE#HASH|text` anchors and `edit` consumes hashline operations on that anchored view.

## Hashline Spec

Hashline provides atomic, content-addressed file edits.

### Line Anchor

```rust
struct LineAnchor {
    line: u32,       // 1-based line number
    hash: String,    // blake3(line_bytes), 12 hex chars
}
```

### Hash Computation

1. Split file on `\n`
2. For each line: strip trailing `\r`, hash bytes with blake3
3. Take first 12 hex characters

This normalizes CRLF to LF for hashing while preserving original line endings in output.

### Patch Operations

```rust
enum HashlineOp {
    InsertBefore { anchor: LineAnchor, lines: Vec<String> },
    InsertAfter { anchor: LineAnchor, lines: Vec<String> },
    Replace { expected: Vec<LineAnchor>, lines: Vec<String> },
    Delete { expected: Vec<LineAnchor> },
}
```

### Apply Algorithm

1. **Validate anchors**: All anchors must match current content at specified lines
2. **Detect overlaps**: Operations must not conflict (no two ops touch the same line)
3. **Apply bottom-up**: Process in descending line order to avoid index drift
4. **Atomic write**: Write to temp file, then rename

### Error Types

- `ANCHOR_MISMATCH` - Line content does not match expected hash
- `OUT_OF_RANGE` - Line number exceeds file bounds
- `OVERLAP` - Multiple operations conflict
- `EMPTY_PATCH` - No operations provided

### Diff Artifacts

On successful apply, a unified diff is written to `artifacts/edit-{edit_id}.diff` and referenced in the `EditApplied` event.

## Tool Output Persistence Policy

Tool results are persisted in two layers:

- Event summaries stay capped for JSONL stability.
- Redacted full outputs are written under `artifacts/toolcalls/<tool_call_id>/` and referenced with `ArtifactWritten` events.

Interactive question state for `user.question` is stored separately under
`state/questions/<tool_call_id>.json` inside the run root so headless flows and replay helpers can
inspect the native prompt/answer handoff without scraping tool artifacts.

## Provider Context Compaction

Provider-visible conversation state is compacted without rewriting `events.jsonl`.

- `events.jsonl` remains append-only and stays the source of truth.
- Compaction writes checkpoint artifacts under `artifacts/compactions/<agent_id>/<checkpoint_id>.json`.
- The coordinator emits `CompactionRequested`, `ArtifactWritten`, `CompactionWritten`, and `CompactionApplied` for successful checkpoints, or `CompactionFailed` when a retry cannot shrink context safely.
- Resume restores provider context from the latest applied checkpoint artifact, then replays post-checkpoint deltas from `events.jsonl`.

Checkpoint payloads carry:

- a lossy summary of older turns,
- recent preserved turns kept verbatim,
- advisory `pruned_tool_artifacts` metadata for artifacts associated with compacted turns,
- structured source facts for compacted turns, relevant artifacts, touched files, and previous-checkpoint lineage,
- tail-boundary metadata describing whether the preserved suffix is whole-turn, oversized whole-turn, or summary-only,
- summary-source metadata recording hook overrides, optional model-backed summaries, and deterministic fallback,
- summary contract metadata, including the active contract version when the default Harness sections are enforced,
- replay-derived operational memory for read files, modified files, and compact operation facts,
- a first-class timeline entry that UIs/replay views can render without parsing prose.

Failed and aborted provider turns can be kept as recent turns when preserving them helps continuity.
Checkpoint artifacts carry their status, failure stage, and redacted reason, and replay/debug projections
surface the same incomplete-turn marker instead of displaying the partial assistant text as a completed answer.

The checkpoint recap injected back into provider requests is historical background only. It is intentionally not treated as a system instruction; preserved recent turns and the live user prompt take precedence.

Lifecycle hooks are projected onto coordinator-owned typed hook phases (`tool_preflight`, `tool_result`, `provider_params`, `provider_context_transform`, `agent_turn_started`, `agent_turn_finished`, `session_idle`, `message_received`, and `compaction_requested`). Hook command output is redacted and capped before event metadata is persisted, and structured hook effects record allow/deny, context-transform, reminder, artifact, diagnostic, truncation, recovery, and notification intent. Disabled hooks record skipped metadata without executing. Lifecycle hooks can observe `compaction_requested`; critical hook failure or a typed `deny` effect cancels the checkpoint and records `CompactionFailed`. A successful hook may supply a custom summary by writing output beginning with `compaction_summary:` or by returning a `transform_context` effect; hook summaries take precedence over optional model-backed summaries. Model-backed summary calls are disabled by default, run through the provider abstraction without emitting provider request/stream events, and must return the default Harness summary sections when `runtime.compaction.structured_summary_contract=true`. Empty, failing, overflowing, or invalid model summaries fall back to the deterministic rolling summary and record `summary_source.deterministic_fallback=true`.

Operational memory remains event-derived. The coordinator gathers capped read-file facts, modified-file facts, and compact operation facts from the durable event/artifact stream between checkpoint boundaries, then stores summary counts and facts inside checkpoint metadata. Replay does not scan the workspace or execute tools to rebuild that memory.

### Manual `/compact`

Manual `/compact` means "write a checkpoint now, preserving the latest completed turn." It summarizes older turns, keeps the latest turn verbatim, and writes the same checkpoint artifacts and append-only compaction events as normal provider-context compaction. The checkpoint summary is lossy, and the command does not guarantee immediate provider context/token reduction; one-turn sessions no-op because there is no older completed turn to summarize. Automatic proactive compaction and overflow-retry compaction remain separate coordinator paths.

### Overflow retry behavior

After an overflow-style provider failure, the coordinator may compact and retry once. This behavior is enabled by default and can be disabled with `runtime.compaction.autoRetryOverflow=false`. Normal proactive compaction keeps recent turns verbatim and summarizes older turns. Overflow retry can also fall back to a summary-only checkpoint when a single preserved turn is itself too large, but only when the resulting checkpoint is strictly smaller than the active provider context. When `runtime.compaction.splitOversizedTurns=true`, an oversized latest turn can instead be split inside the checkpoint artifact: the earlier portion is summarized and a suffix remains provider-visible as recent context, without adding event variants or rewriting history.

### Session artifacts vs UI memory caps

Compaction is a provider-context persistence feature. It is separate from TUI/session presentation caps that trim or collapse on-screen history for usability. UI memory caps do not rewrite provider context, do not create compaction checkpoints, and should not be treated as compaction.

## Replay Contract

Replay is side-effect free. It:

1. Reads events from JSONL in `seq` order
2. Applies pure projections to rebuild run, resume, catalog, provider-context, and transcript/message/part state
3. Does not execute tools or make network calls
4. Produces the same final state as the live run

This enables:
- Post-hoc analysis of runs
- Deterministic test fixtures
- Session sharing without code execution
