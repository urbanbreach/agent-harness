# Desktop GUI backend protocol

The desktop GUI (`gui/`) runs `harness gui-server` as a child process and talks to it
over stdin/stdout. The server owns everything Harness-specific: configuration,
credentials, sessions, coordinators, events and intents. The GUI process owns
generic workspace work (files, Git, terminals) and never writes Harness journals.

## Transport

- One JSON object per line, UTF-8, `\n` terminated. stderr carries diagnostics only.
- Request: `{"id": 7, "method": "sessions.list", "params": {...}}`. `id` is a
  positive integer chosen by the client; `params` may be omitted for `{}`.
- Response: `{"id": 7, "result": ...}` or
  `{"id": 7, "error": {"message": "...", "code": "invalid_params" | "not_found" | "failed"}}`.
- Notification (server to client, no `id`): `{"method": "session.update", "params": {...}}`.
- Requests are handled concurrently; responses can arrive out of order.
- End of stdin stops every live session (graceful `stop_run`) and exits with 0.
- Field names are camelCase. Harness types that already have a serde form
  (`EventEnvelopeV1`, `LiveEventEnvelope`, `SessionCatalogEntry`, file tags,
  `RewindPoint`) are embedded unchanged, so they keep their snake_case fields.
- Paths are absolute strings. Times are RFC 3339 strings or Unix milliseconds,
  named `...At` and `...Ms` respectively.

## Application

### `app.initialize` → `Initialize`
Params: `{ "cwd"?: string }`. Resolves configuration for `cwd` (or the server's
working directory) without creating files.

```ts
type Initialize = {
  version: string            // harness version
  home: string               // resolved Harness home
  guiStateDir: string        // <home>/gui (may not exist yet)
  defaultCwd: string         // server working directory
  mock: boolean              // started with --mock
}
```

### `state.read` / `state.write`
GUI-owned JSON documents under `<home>/gui/<name>.json`. `name` matches
`^[a-z][a-z0-9-]{0,31}$`.
- `state.read { name }` → the parsed JSON value, or `null` when absent.
- `state.write { name, value }` → `null`. Atomic replace; creates `<home>/gui/`
  on first write only.

## Projects and sessions

A project is a workspace directory. Its sessions live in the Harness session
directory resolved for that directory (`<home>/sessions/<key>/` unless configured).

- `sessions.list { cwd }` → `SessionRow[]`, newest first.
- `sessions.events { runDir }` → `EventEnvelopeV1[]` (durable journal, read-only).
- `sessions.search { cwds: string[], query, limit }` →
  `{ runDir, role: "user" | "assistant", snippet }[]`.
- `sessions.rename { runDir, title }` → `null`. Live sessions go through the
  coordinator; idle sessions append `session_title_updated` to their journal.
- `sessions.delete { runDir }` → `{ trashedTo: string }`.
- `sessions.pins { cwd }` → `string[]` run ids; `sessions.setPinned { cwd, runId, pinned }`.

```ts
type SessionRow = {
  runId: string
  runDir: string
  cwd: string
  catalog: SessionCatalogEntry   // core serde shape
  createdAt?: string
  live: boolean                  // a coordinator is running for it in this server
}
```

## Live sessions

`sessionId` is the run id. At most one live runtime exists per run.

- `session.create { cwd, selection?: ModelSelection, accessMode?: AccessMode,
  worktree?: { name?: string } }` → `{ sessionId, runDir, cwd }`.
- `session.continue { runDir }` → `{ sessionId, runDir, cwd }`. Fails with the
  resume-disabled reason when the journal cannot be continued.
- `session.close { sessionId }` → `null`. Stops the run.
- `session.intent { sessionId, intent: Intent }` → `null` once accepted by the
  session's intent handler. Outcomes arrive as `session.update` notifications.

```ts
type ModelSelection = { profile: string; model?: string; variant?: string }
type AccessMode = "supervised" | "autoEdits" | "auto" | "full"

type Intent =
  | { kind: "submit" | "followUp"; text: string; fileTags?: SelectedFileTag[];
      agentTags?: SelectedAgentTag[]; resourceTags?: SelectedResourceTag[];
      attachments?: Attachment[] }
  | { kind: "steer"; text: string; attachments?: Attachment[] }
  | { kind: "queued"; id: number; prompt: Intent }
  | { kind: "withdrawQueued" } | { kind: "withdrawAccepted" }
  | { kind: "interrupt"; taskIds: string[]; reason: "user" | "replace" | "task" | "sendNow" }
  | { kind: "resolvePermission"; permissionId: string;
      decision: "allow" | "deny"; reason?: string; grantScope?: "once" | "session" }
  | { kind: "setYolo"; enabled: boolean }
  | { kind: "setAccessMode"; mode: AccessMode }
  | { kind: "switchModel"; selection: ModelSelection }
  | { kind: "compact"; instructions?: string } | { kind: "handoff"; focus?: string }
  | { kind: "shake"; args?: string } | { kind: "cancelCompaction"; agentId: string }
  | { kind: "backgroundForeground" } | { kind: "demoteChild"; handleId: string }
  | { kind: "cancelSubagent"; sessionId: string }
  | { kind: "inspectCommand"; taskId?: string }
  | { kind: "loadRewindPoints"; generation: number; cancelTaskIds: string[] }
  | { kind: "rewind"; generation: number; requestId: string }
  | { kind: "revertWorkspace"; snapshotRequestId: string }
  | { kind: "updateTitle"; title: string }
  | { kind: "fork"; promptRequestId?: string }     // omitted = whole stable history
  | { kind: "clone" }
  | { kind: "runShell"; command: string }

type Attachment = { mime: string; dataBase64: string }
```

### `session.update` notification

`{ sessionId, update: Update }` where `Update` mirrors the TUI's `LiveUpdate`:

```ts
type Update =
  | { kind: "event"; delivery: "durable"; event: EventEnvelopeV1 }
  | { kind: "event"; delivery: "live"; event: LiveEventEnvelope }
  | { kind: "status"; text: string }
  | { kind: "notice"; level: "info" | "error"; message: string }
  | { kind: "modelNotice"; message: string }
  | { kind: "yolo"; enabled: boolean } | { kind: "yoloFailed" }
  | { kind: "compaction"; command: "compact" | "handoff" | "shake" | "idle";
      outcome: "done" | "warning" | "cancelled" | "failed"; message?: string;
      tokensAfter?: number; savedPath?: string }
  | { kind: "commandOutput"; snapshot: unknown }
  | { kind: "rewindPoints"; generation: number; points?: RewindPoint[]; error?: string }
  | { kind: "rewindComplete"; generation: number; point?: RewindPoint; error?: string }
  | { kind: "restoreQueued"; texts: string[] }
  | { kind: "queuedReceipt"; id: number; error?: string }
  | { kind: "forked"; runId: string; runDir: string; draft: string }
  | { kind: "exited"; error?: string }
```

Durable events are delivered once each, in `seq` order. After a lag the server
replays durable events from the journal before resuming; clients drop any `seq`
they already applied.

## Models, commands and completion

- `models.catalog { cwd }` → `{ current: ModelEntry | null, models: ModelEntry[],
  profiles: string[] }`.
- `commands.list { cwd }` → `{ name, description, argumentHint?, scope:
  "builtin" | "project" | "user", template? }[]`.
- `files.index { cwd, limit? }` → `{ path, dir }[]` (project relative, capped).

```ts
type ModelEntry = {
  profile: string; provider: string; providerLabel?: string; model: string;
  modelLabel?: string; variant?: string; variantLabel?: string; label: string;
  reasoningEffort?: string; contextWindow?: number; description?: string
}
```

## Providers and sign-in

- `providers.list {}` → `{ id, label, description, methods: string[], connected: boolean }[]`.
- `auth.start { args: string[], stdin?: string }` → `{ attempt: number }`. Progress
  arrives as `auth.update { attempt, kind: "notice" | "result", level?, success?, message }`.
- `auth.input { attempt, line: string | null }` → `null` (`null` cancels).
- `auth.logout { provider }` → `null`.

## Settings

- `settings.list { cwd }` → `{ id, surface, label, description, kind, value,
  defaultValue, editable, choices?: string[] }[]`.
- `settings.write { cwd, id, value }` → `null`; `settings.reset { cwd, id }` → `null`.
