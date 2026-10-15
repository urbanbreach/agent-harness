# Lifecycle hooks

Hooks run configured commands at coordinator transitions. They are opt-in and
execute in configuration order. An executable must match an entry in
`permissions.shell_allowlist.executables`, even when ordinary Bash permission
uses command patterns.

```jsonc
{
  "permissions": {
    "shell_allowlist": { "executables": ["/usr/bin/printf"] }
  },
  "hooks": {
    "lifecycle": [
      {
        "id": "finished",
        "event": "tool_call_finished",
        "command": ["/usr/bin/printf", "tool completed\n"],
        "timeout_ms": 1000,
        "critical": false
      }
    ]
  }
}
```

Only `event` and `command` are required. `id` is optional; omitted IDs use the
event name and one-based configuration index, such as `tool_call_finished_01`.
`critical` defaults to false, `env` defaults to an empty map, and `timeout_ms`
defaults to 5,000 (30,000 for the five compaction events). `name` and `timeoutMs`
are accepted aliases for `id` and `timeout_ms`.

Commands are argument vectors; shell syntax requires an explicitly allowed shell.
The default working directory is the actor's effective execution directory. A
configured `cwd` is relative to that directory, must remain inside the run
workspace after resolving symlinks, and must satisfy any configured `cwd_roots`.
A child isolated outside the run workspace cannot use hooks there.

## Stages and failures

| Event | Boundary |
| --- | --- |
| `run_started` | After opening and recording a new run, including a resumed run |
| `run_finished`, `run_failed` | After active work and native servers stop, before the terminal run event |
| `agent_turn_started` | Before preparing and dispatching a turn |
| `agent_turn_finished` | Before recording the turn result |
| `tool_call_started` | After permission and capacity checks, before calling the tool |
| `tool_call_finished` | Before publishing the tool result |
| `provider_request_started` | Before sending a provider request |
| `provider_request_finished` | After recording provider completion, before accepting its answer |
| `compaction_requested` | Before the pass summarizes: it can cancel the pass or supply the compaction |
| `compaction_summarizing` | Before a summary request is sent: it can add context, replace the prompt and keep data |
| `compaction_written` | Before the compaction entry is appended |
| `compaction_applied` | After the entry is appended; it cannot undo it |
| `compaction_failed` | After a compaction pass fails or is cancelled |
| `subagent_spawned` | Before recording and creating a child agent |
| `subagent_finished` | After the child's turn-finished hooks, before its terminal event |
| `permission_requested` | Before exposing an approval request |
| `permission_resolved` | Before recording the decision or saving a grant |

A noncritical failure emits a warning and execution continues. A critical failure
stops that hook batch and fails the operation. Rejected approval hooks record a
denial and never save an approval grant. Failed turn and tool hooks leave the run
available for further work. A failed run-start or run-finish hook records
`RunFailed` and releases the writer.

Completion hooks cannot undo a tool's completed file or network effects. They can
reject its result. `compaction_written` runs before the shared durable and
in-memory commit, so a critical failure there leaves the original context intact.
`compaction_applied` runs after the commit and can only report.

## Compaction hooks

The five compaction events take part in the pass, and two of them can answer. A
handler that exits 0 and prints exactly one JSON object on stdout (surrounding
whitespace is ignored) returns a result. Anything else is a receipt only: other
stdout, a JSON value that is not an object, output cut at the 512 KiB capture
limit, a nonzero exit, a timeout, a blocked command or a failed start. Such a
handler contributes nothing and the chain goes on. Only a hook marked `critical`
stops it, and the pass then ends with a hook error. A failing hook never vetoes a
compaction by itself; a veto is the explicit `cancel` result below.

Handlers of an event run in configuration order. Each result replaces the previous
one as a whole, so a later `{}` clears an earlier result, while a handler that
printed no object leaves it in place. Handlers run off the coordinator thread, so
a slow one does not stall other agents, and cancelling the pass stops the running
handler. Their default timeout is 30 seconds (`timeout_ms` unset); a set
`timeout_ms` applies as written, from 1 to 300,000 ms.

`compaction_requested` runs before the pass prepares a summary. Its input adds
`session_id`, `reason` (`manual`, `threshold`, `overflow`, `incomplete` or `idle`),
`preparation`, `branch_from_seq`, `branch_through_seq`, `previous_summary` and
`custom_instructions`. `preparation` holds `history_messages`, `prefix_messages`,
`retained_messages`, `is_split_turn`, `tokens_before`, `keep_recent_tokens`,
`reserve_tokens`, `window_tokens`, `first_kept_seq` and the `read_files` and
`modified_files` sets, each as `{ "paths": [...], "count": N }` with at most 200
paths listed. `custom_instructions` is the public focus given to `/compact`; internal
guidance is never passed. Its result may contain:

- `cancel`: when truthy (JavaScript rules, so `false`, `0`, `""` and `null` are
  not), the chain stops, no summary is requested and no entry is written. The pass
  ends as a cancellation that is a hook veto, not a user interrupt, and `cancel`
  wins over a `compaction` in the same object.
- `compaction`: `{ "summary", "short_summary"?, "first_kept_seq", "tokens_before",
  "details"?, "preserve_data"? }`. Harness commits it as supplied, with no summarizer
  call: the cut falls at the first entry whose sequence is at or after
  `first_kept_seq`, `tokens_before` is recorded as given, and the entry is marked
  `from_hook`. It needs a nonblank `summary` string, an integer `first_kept_seq`
  and an integer `tokens_before`; without all three the object counts as an
  ordinary result with nothing to commit. `preserve_data` is redacted and kept to
  64 KiB serialized.

While a `compaction_requested` hook is configured, background summaries are not
prepared ahead of time, because the hook must see each pass.

`compaction_summarizing` runs before a summary request goes out, for the plain
summary, the native Anthropic and custom-endpoint paths and a background summary.
It also runs before a notes rollover, where only `preserve_data` matters. Its input
adds `session_id`, `messages_text` (the summarized and prefix messages as text,
cut at 56 KiB on a character boundary) and `messages_truncated`. Its result may
contain:

- `context`: an array of strings. Harness renders them as `- line` items in an
  `<additional-context>` block ahead of the instructions, unescaped and in hook
  order. Non-string entries are dropped.
- `prompt`: a string that replaces the base summary prompt. An empty string means
  the default prompt. The public focus is still appended.
- `preserve_data`: an object merged into the entry's preserved data. When a key
  is also set by the engine, the engine's value wins.

`compaction_applied` reports the saved entry: `session_id`, `seq`, `first_kept_seq`,
`summary`, `short_summary`, `method`, `tokens_before`, `tokens_after` and
`from_hook`. Its output is ignored, and a failure leaves only a receipt and a live
warning. `compaction_written` and `compaction_failed` keep their receipt-only
behavior; `compaction_failed` also runs for a vetoed or cancelled pass.

`messages_text`, `previous_summary` and `custom_instructions` appear only in
`HARNESS_HOOK_CONTEXT_JSON`, truncated at a character boundary (56, 24 and 8 KiB).
They have no separate environment variable, and the JSON as a whole stays under
120 KiB.

## Context and limits

Hooks receive `HARNESS_HOOK_CONTEXT_JSON` and matching `HARNESS_HOOK_*` variables
for available fields: ID, event, run, workspace, artifacts directory, cwd, actor,
agent, request, task, tool call, tool, provider, model, profile, parent agent,
permission, outcome, output summary, and failure reason. Missing values have no
environment variable. Text fields are redacted and capped at 4,096 characters.
Raw tool arguments, provider payloads, and reasoning fragments are not supplied.

The inherited environment contains only PATH, TERM, HOME, USER, LOGNAME,
SSH_AUTH_SOCK, temporary-directory settings and locale settings. Add other values
explicitly with `env`; these values join the credential redactor before
execution. `HARNESS_HOOK_*` names are reserved. Hooks run in their own session
without a controlling terminal, so a command that prompts on `/dev/tty` fails
instead of waiting for input.

There are at most 64 configured hooks. Each command and its configured environment
may occupy at most 128 KiB. Timeouts range from 1 to 300,000 ms; `timeout_ms`
defaults to 5,000 ms for most events and 30,000 ms for the compaction events
(`effective_timeout_ms`). The process runner captures at most 512 KiB per output
stream and cleans up the command's process group on exit or timeout. Task receipts
retain the first 128 hook executions, with redacted summaries of at most 1,024
characters. Raw command text and environment values are not written to the journal.

Most hooks serialize coordinator transitions until they finish or time out. Keep
them short; the compaction handlers above are the exception. Interactive runs
execute this work off the UI executor; single-thread
callers wait inline. Allowlisting and cwd checks are policy checks, not an OS
sandbox for the command.

Replay, inspection, and historical recovery do not run hooks. Deterministic runs
suppress execution and mark retained task receipts as skipped. Resuming creates
a new run-start transition; it does not repeat historical tool or provider hooks.
