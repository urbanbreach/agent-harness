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
- **proj/** - Pure projections for UI and replay
- **agent/** - Minimal agent runtime
- **config** - Configuration parsing and validation
- **clock** - Clock abstraction (real and fake for determinism)
- **redact** - Secret redaction before persistence

### Agent prompt assets and instructions

Interactive agent runtime settings still come from structured config, but prompt bodies are now resolved separately from markdown assets and project instructions:

- `.agent-harness/agents/<agent>.md` provides the canonical file-backed prompt body for an agent.
- inline `system_prompt` in config remains a compatibility override and wins over the markdown body.
- `AGENTS.md` is loaded as a separate project-instruction layer and prepended to the final runtime system prompt.

This keeps config focused on structured behavior while moving prompt prose into dedicated workspace assets.

### harness-providers (library)

Provider abstraction for LLM completions:

- `Provider` trait for streaming completions
- `MockProvider` - Deterministic offline provider using request digest fixtures
- `OpenAiCompatibleProvider` - HTTP/SSE streaming to OpenAI-compatible endpoints

### harness-tools (library)

Built-in tool implementations:

- `read` / `list` / `glob` / `grep` - Safe workspace discovery and search
- `write` / `edit` - Default file editing workflows (`edit` is hashline-first)
- `bash` - Execute shell commands with allowlist
- `task` / `batch` / `question` / `skill` - Control-plane and delegation workflows
- `webfetch` / `websearch` / `codesearch` / `lsp` - Network and language-intelligence workflows

The active registry exposes a single native provider surface. Canonical ids such as
`read`, `write`, `bash`, `webfetch`, `websearch`, `codesearch`, `question`, `batch`, `task`,
and `lsp` are the documented tool surface, while lower-level executors remain
internal implementation details behind those tool ids.

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
- `AgentSpawned` / `AgentStopped`

**Task Management**
- `TaskScheduled` - Includes state: queued or started
- `TaskCancelled` - Best-effort cancellation
- `TaskCompleted` - Normal completion
- `TaskResultLate` - Result arrived after cancellation

**Progress and Staleness**
- `StaleDetected` - Task exceeded staleness timeout

**Provider Streaming**
- `ProviderRequestStarted`
- `ProviderStreamDelta` - Text chunk
- `ProviderRequestFinished`
- Provider tool-call deltas/completions are normalized before coordinator execution

**Tool Execution**
- `ToolCallRequested`
- `ToolCallStarted`
- `ToolCallFinished`

**Permissions**
- `PermissionRequested` - User intervention required
- `PermissionResolved` - Allow or deny decision recorded

**Editing**
- `EditProposed` - Edit prepared for review
- `EditApplied` - Edit successfully committed
- `EditRejected` - Edit failed (mismatch, denied, etc.)

**Artifacts and Policy**
- `ArtifactWritten` - File stored to session
- `PolicyViolationDetected` - Security rule triggered

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
| `task` | task / agent orchestration flow | allow / deny / ask |
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
   - `ask` - Emit `PermissionRequested`, pause until `ResolvePermission` command

### Headless Mode

In headless scenarios, `ask` defaults to `deny` unless the scenario script explicitly sends `ResolvePermission(Allow)`.

### Anti-Footgun: No Redelegation

Workers cannot call direct coordinator spawn APIs. Only `ActorKind::Supervisor` may call `SpawnAgent`. Violations emit `PolicyViolationDetected`.

## Multi-turn Tool Loop

Agent turns can iterate across provider output and tool execution:

1. Provider stream starts and emits text and/or structured tool calls.
2. Structured tool calls are mapped back to canonical tool ids for the current request.
3. The coordinator executes allowed tools, waits for completion, and reinjects tool results as tool-role messages.
4. The agent loop continues until a provider turn completes with no tool calls or a guardrail fails closed.

Guardrails bound the loop by iteration count and total tool calls per turn.

## Tool Surface Policy

Provider and tool exposure is selected per agent by its configured `tools` list. The harness ships
a single native tool surface, so profiles opt in by naming canonical tool ids such as `read`,
`write`, `edit`, `bash`, and `task` directly. By default, `read` emits
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

## Replay Contract

Replay is side-effect free. It:

1. Reads events from JSONL in `seq` order
2. Applies pure projections to rebuild state
3. Does not execute tools or make network calls
4. Produces the same final state as the live run

This enables:
- Post-hoc analysis of runs
- Deterministic test fixtures
- Session sharing without code execution
