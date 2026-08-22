# Architecture

This document describes the high-level architecture of Agent Harness.

## Crate Boundaries

### harness (binary crate)

The CLI entry point. Contains subcommand implementations:

- `harness run` - Headless scenario execution
- `harness tui` - Interactive terminal UI
- `harness sessions inspect/export` - Replay-derived session inspection and support export
- `harness schema` - JSON Schema output
- `harness config validate` - Config validation
- `harness sessions list` - List recorded sessions

### harness-core (library)

Core runtime and domain logic:

- **event/** - Event schema v1, envelope types, event builder
- **store/** - Event store trait, in-memory and JSONL file implementations
- **coord/** - Coordinator actor (single scheduling authority)
- **sched/** - Scheduler with concurrency slots and stale detection
- **cron_schedule** - Cron schedule registry. Its public summary keeps `registered` and
  `executor_available` separate: registering a schedule never claims execution is available
  (`executor_available=false` until a product executor loop is wired).
- **perm/** - Permission engine (allow/deny/ask)
- **tool/** - Tool framework and capability gating
- **edit/** - Hashline edit engine
- **proj/** - Pure projections for run summary, resume planning, and session catalog state
- **transcript_projection** - Pure replay-derived transcript/session/message/part projection for resume, export, TUI, and debugging surfaces
- **agent/** - Minimal agent runtime
- **agent** - Provider-facing execution state for the singleton generic profile
- **config** - Configuration parsing and validation
- **clock** - Clock abstraction (real and fake for determinism)
- **redact** - Secret redaction before persistence

### Agent prompt assets and instructions

Interactive agent runtime settings come from structured config, while the prompt body is resolved separately from the shipped asset and project instructions:

- `.agent-harness/agents/default.md` supplies the generic coding prompt.
- inline `agent.system_prompt` replaces the shipped body.
- `AGENTS.md` is loaded as a separate project-instruction layer and composed into the final runtime system prompt.

This keeps config focused on structured behavior while allowing one prompt to adapt to model, workspace, project-instruction, and skill context.

### Prompt reference seam map

Reference prompt-system behavior is adopted only as user-observable Harness behavior, not by copying source architecture, package layout, or brand-specific terminology. Each adopted pattern maps to a concrete Harness seam or an explicit deferred seam:

| Reference pattern | Harness seam | V1 status |
|---|---|---|
| Generic coding prompt | `.agent-harness/agents/default.md`, `crates/harness/src/bootstrap.rs`, and `crates/harness/src/dynamic_prompt.rs` | Used by interactive execution without primary-role switching |
| Intent-gate before tool use | `crates/harness/src/dynamic_prompt.rs` (`intent_gate`) | Shipped for ambiguous requests before tool use |
| Named subagents | `.agent-harness/agents/{explore,general,librarian}.md` and the `task(subagent_type=...)` contract | Preserved as bounded child profiles |
| Structured delegation reminder | `crates/harness/src/dynamic_prompt.rs` (`delegation_reminder`), `docs/operations/generic-agent-and-tasks.md`, and the `task` native tool contract | Shipped as named subagent guidance |
| Category-specific routing and prompt appends | `harness_core::agent_catalog` and named profile configuration | Shipped as bounded named profiles, not a category router |
| Markdown-defined skills with progressive disclosure | `harness-tools::skill_catalog`, `.agent-harness/skills/*/SKILL.md`, and `docs/configuration/starter-skills.md` | Shipped for the V1 built-in skill set |
| Disableable built-in capabilities | `skills.disabled` config shape, `SkillCatalogStatus::Disabled`, doctor skill catalog metadata, `harness-core::extension_manifest`, `configs/extension-manifest.v1.schema.json`, and `docs/operations/extension-strategy.md` | Skills ship as runtime capabilities; typed extension manifests ship as descriptor-only metadata with runtime hosting post-V1 |
| Command/hook lifecycle maps | `docs/operations/extension-strategy.md` command/hook seam | Native lifecycle hooks ship; markdown command files and extension command-hook execution remain unsupported/post-V1 |

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
- `background_cancel` - Explicit coordinator-owned cancellation wrapper for background child requests
- `session_list` / `session_read` / `session_search` / `session_info` - Replay-derived model-visible session inspection tools
- `ast_grep_search` - Read-only ast-grep CLI structural search adapter with workspace path safety, hard caps, and artifact spill
- `ast_grep_replace` - Edit-permission structural rewrite adapter that defaults to dry-run, uses ast-grep JSON rewrite output only, and applies through Harness path checks, atomic writes, and diff artifacts
- `webfetch` / `websearch` / `codesearch` / `lsp` - Network and language-intelligence workflows

Hashline editing is the only normal file-changing route. Agent profiles expose `read`
and `edit`; low-level hashline scan/apply helpers are reserved for internal
compatibility and focused test lanes.

The active registry exposes a single native provider surface. Canonical ids such as
`read`, `edit`, `bash`, `webfetch`, `websearch`, `codesearch`, `question`, `batch`, `task`,
executors remain internal implementation details behind those tool ids.

`harness-tools::tool_catalog` mirrors the active registry as metadata: stable
canonical id, provider function name, aliases, description summary, capability,
permission kind, actor availability, supervisor-only status, schema status,
mutation/read-only classification, replay behavior, artifact behavior, and docs
status. Doctor and support export can read this metadata without starting MCP
servers or making network calls.

`ast_grep_replace` is present only as an edit-permission native tool. The
ast-grep process supplies JSON rewrite ranges but never mutates the workspace
directly; Harness validates byte ranges against current file contents, rejects
overlap/truncated apply, writes diff artifacts, and performs atomic workspace
writes through the same edit authority boundary.

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
- `TaskScheduled` - Includes `task_id`, `state` (queued or started), optional `queue_key`, and optional typed `metadata`; child agent turns record parent-tool/child-request lineage in `metadata.lineage` when scheduled so active lifecycle projections do not depend on terminal events
- `TaskCancelled` - Best-effort cancellation
- `TaskCompleted` - Normal completion
- `TaskResultLate` - Result arrived after cancellation
- `BackgroundTaskNotification` - Durable parent wakeup record for a `task(run_in_background=true)` child request after the child reaches a terminal state; carries parent/child ids, terminal status (`completed`, `cancelled`, `failed`, or `timed_out`), capped summary, terminal event id, and delivered parent turn request id. Replay projects this event only and must not schedule provider work.

**Progress and Staleness**
- `StaleDetected` - Task exceeded staleness timeout
- `UserMessageSubmitted` - User prompt accepted into the event stream
- `PromptAttachmentsSubmitted` - Prompt attachment metadata accepted into the event stream

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
- `SessionCompaction` - session-level compaction event; replaces the deprecated `CompactionRequested`/`CompactionWritten`/`CompactionApplied`/`CompactionFailed` sequence. Carries the compaction summary, token estimate before compaction, file lists, trigger reason, and hook provenance in a single event.
- `BranchSummary` - branch-level summary event for forked/child session context.

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
| Retry attempt counter and policy | Optional start `metadata.retry` with `{ attempt, max_attempts, delay_ms, category }` | Additive, serde-defaulted counter used for bounded retry before the final provider response is committed. Absent on old logs; the coordinator treats missing retry metadata as the first attempt. |
| Transient error server hint | Optional `retry_after_ms` in Error event metadata (provider-lifecycle finish events) | Records provider Retry-After header values in milliseconds when present. Advisory; scheduling falls back to exponential backoff when absent. Old logs without the field replay identically. |
| Thinking or reasoning signatures | Optional finish `metadata.thinking` | Store only summaries, digests, or signature ids. Never store raw hidden thinking text. |
| Provider payloads and secrets | Never durable | Raw requests, raw responses, auth headers, cookies, keys, and PEM blocks are excluded from event logs. Live event logs may include provider reasoning delta events as local session evidence; provider reasoning metadata stores only summaries, digests, or signature ids. |

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

**Workspace Snapshots**
- `WorkspaceSnapshot` - Captured working-tree state before a tool batch; stores a redacted map of relative paths to file contents and content digests in the artifact store. Dotenv-style secret files are omitted from snapshot artifacts.
- `WorkspaceReverted` - Restored the workspace from a prior snapshot; records restored paths, removed paths, and any failures without rewriting the event log.

**Team Membership**
Team membership events record the team role, dependency edges, and shutdown
state for child sessions. Members remain ordinary child agents: their
provider/tool work is represented by the same task and provider lifecycle
events as standalone agents, and event timestamps come from the enclosing
event envelope. `blocks` is a projection-derived inverse of `blocked_by`;
callers provide `blocked_by`, and replay recomputes `blocks`
deterministically. Shutdown approval can stop child sessions or cancel
scheduler tasks; non-lead member sessions stay pending until another active
member is shutdown-approved. Duplicate team membership events are rejected by
the coordinator, and projections keep first-seen state if old logs contain
duplicates.

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

Provider and tool exposure is selected by the singleton `agent.tools` list. The harness ships
a single native tool surface, so the generic agent opts in by naming canonical tool ids such as
`read`, `edit`, `bash`, `task`, and `background_output` directly. Named subagents have bounded prompt
and tool configurations; worker capability filtering, task permission checks, and direct-child ownership
remain coordinator-enforced. By default, `read` emits
`LINE#HASH|text` anchors and `edit` consumes hashline operations on that anchored view.

Model-visible session tools are part of this native surface but remain replay
readers only. They inspect stored session roots, reject traversal/out-of-root
selectors, redact by default, cap inline output, and spill large output to
artifacts. They never call `harness sessions`, execute providers/tools/hooks,
start MCP servers, or make network calls.

`background_cancel` is only a canonical wrapper around the existing coordinator
background cancellation path already used by `background_output(cancel=true)`.
The compatibility form remains supported, but task next-actions prefer
`background_cancel(request_id=...)` for explicit cancellation.

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

## Event-store crash-tail recovery

The JSONL event store is append-only, but opening a run with the writer lock held may repair an
interrupted final write before replay starts. The scanner accepts all complete contiguous events,
truncates one unterminated invalid final line back to the previous complete line boundary, and
normalizes one complete final event that is missing its newline terminator by appending the newline.
Already-terminated invalid JSON remains a hard parse error. Recovery never executes providers,
tools, hooks, MCP servers, shell commands, or replay side effects; it only repairs the event log
tail so prior complete events remain readable and the next append uses the expected sequence.

## Provider Context Compaction

Provider-visible conversation state is compacted without rewriting `events.jsonl`.

V1 compaction contract:

- Threshold policy: proactive and pre-prompt checks use provider/model context-window metadata when
  present; when metadata is absent, estimated trigger checks use
  `runtime.compaction.fallback_input_tokens` (default `32768`). Overflow-style provider failures may
  run one overflow retry when `runtime.compaction.auto_retry_overflow=true`.
- Retained recent turns: `provider_context_keep_recent_tokens` keeps roughly one quarter of the
  model input window, clamped between 2,000 and 8,000 tokens. Manual `/compact` preserves the latest
  completed turn verbatim; overflow/failed-response compaction may use summary-only or split-tail
  boundaries only when that safely reduces provider context.
- File/tool/skill/todo/plan context: checkpoint operational memory stores event-derived read-file
  and modified-file facts, generic tool operation facts, skill loads, todo updates, and plan-handoff
  references from compacted turns. These facts are redacted and capped before persistence.
- Todo/plan bridging: todo updates (`todowrite`/`todoread`) and plan-handoff references are
  summarized as compact operation facts so a resumed agent can continue without guessing which
  checklist or handoff was active.
- Post-compaction restoration hints: checkpoint summaries include source facts, relevant
  files/artifacts, operational memory, tail-boundary metadata, and a reminder that preserved recent turns plus the live user prompt take precedence over the lossy recap.

- `events.jsonl` remains append-only and stays the source of truth.
- Compaction appends a single `SessionCompaction` event with the generated summary, token estimate, file lists, and trigger reason. No checkpoint artifacts are written — the summary lives entirely in the event and the in-memory `ProviderContext`.
- The coordinator emits `SessionCompaction` for successful compactions, or `CompactionFailed` (deprecated) when a retry cannot shrink context safely.
- Resume restores provider context from the latest `SessionCompaction` event, then replays post-compaction deltas from `events.jsonl`.

Checkpoint payloads carry:

- a lossy summary of older turns,
- recent preserved turns kept verbatim,
- advisory `pruned_tool_artifacts` metadata for artifacts associated with compacted turns,
- structured source facts for compacted turns, relevant artifacts, touched files, and previous-checkpoint lineage,
- tail-boundary metadata describing whether the preserved suffix is whole-turn, oversized whole-turn, or summary-only,
- summary-source metadata recording hook overrides, optional model-backed summaries, and deterministic fallback,
- summary contract metadata, including the active contract version when the default Harness sections are enforced,
- replay-derived operational memory for read files, modified files, generic tool operations, skill
  loads, todo updates, and plan handoff/edit references,
- a first-class timeline entry that UIs/replay views can render without parsing prose.

Failed and aborted provider turns can be kept as recent turns when preserving them helps continuity.
Checkpoint artifacts carry their status, failure stage, and redacted reason, and replay/debug projections
surface the same incomplete-turn marker instead of displaying the partial assistant text as a completed answer.

The checkpoint recap injected back into provider requests is historical background only. It is intentionally not treated as a system instruction; preserved recent turns and the live user prompt take precedence.

Lifecycle hooks can observe `compaction_requested`. Critical hook failure cancels the checkpoint and records `CompactionFailed`. A successful hook may supply a custom summary by writing output beginning with `compaction_summary:`; hook summaries take precedence over optional model-backed summaries. Model-backed summary calls are disabled by default, run through the provider abstraction without emitting provider request/stream events, and must return the default Harness summary sections when `runtime.compaction.structured_summary_contract=true`. Empty, failing, overflowing, or invalid model summaries fall back to the deterministic rolling summary and record `summary_source.deterministic_fallback=true` on both the checkpoint artifact and `CompactionWritten` event so UI status surfaces can show the fallback path. Overflow-retry and failed-response compaction attempts are bounded to one recorded attempt for the triggering request.

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

## Input-first TUI runtime scheduling

Interactive input has one producer: a terminal-reader thread feeds a bounded 128-event FIFO. The
runtime arbiter orders fatal writer failure, frame acknowledgement, quit/cancel, terminal input,
pacer and animation deadlines, then live provider updates. An input quantum is bounded to 16
terminal envelopes or 2 ms; fairness permits live progress without reordering input. Live work
retains the 16 live / 8 ms budget boundary. The scheduler uses independent 16 ms flush, 80 ms lazy
scroll-gesture, and 33 ms animation clocks and keeps the one-frame acknowledgement rule.

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
