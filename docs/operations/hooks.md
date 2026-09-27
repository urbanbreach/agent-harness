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

Commands are argument vectors; shell syntax requires an explicitly allowed shell.
The default working directory is the workspace. A configured `cwd` must be
relative, remain inside the workspace after resolving symlinks, and satisfy any
configured `cwd_roots`.

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
| `compaction_requested` | Before requesting a summary |
| `compaction_written`, `compaction_applied` | In that order, before committing the summary to history and memory |
| `compaction_failed` | After a failed compaction attempt |
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
reject its result. Compaction completion hooks run before the shared durable and
in-memory commit so a rejection leaves the original context intact.

## Context and limits

Hooks receive `HARNESS_HOOK_CONTEXT_JSON` and matching `HARNESS_HOOK_*` variables
for available fields: ID, event, run, workspace, artifacts directory, cwd, actor,
agent, request, task, tool call, tool, provider, model, profile, parent agent,
permission, outcome, output summary, and failure reason. Missing values have no
environment variable. Text fields are redacted and capped at 4,096 characters.
Raw tool arguments, provider payloads, and reasoning fragments are not supplied.

The inherited environment contains only PATH, TERM, temporary-directory settings,
and locale settings. Add other values explicitly with `env`; these values join
the credential redactor before execution. `HARNESS_HOOK_*` names are reserved.

There are at most 64 configured hooks. Each command and its configured environment
may occupy at most 128 KiB. Timeouts range from 1 to 300,000 ms and default to
5,000 ms. The process runner captures at most 512 KiB per output stream and cleans
up the command's process group on exit or timeout. Task receipts retain the first
128 hook executions, with redacted summaries of at most 1,024 characters. Raw
command text and environment values are not written to the journal.

Hooks serialize coordinator transitions until they finish or time out. Keep them
short. Interactive runs execute this work off the UI executor; single-thread
callers wait inline. Allowlisting and cwd checks are policy checks, not an OS
sandbox for the command.

Replay, inspection, and historical recovery do not run hooks. Deterministic runs
suppress execution and mark retained task receipts as skipped. Resuming creates
a new run-start transition; it does not repeat historical tool or provider hooks.
