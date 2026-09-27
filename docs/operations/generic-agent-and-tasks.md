# Agents and tasks

Harness uses one generic parent agent for interactive turns. It can delegate
bounded work to `explore`, `general`, or `librarian`. There is no primary-role
picker, category router, or separate planning agent.

Title generation and context compaction are internal coordinator operations.
Their prompts receive no tools and do not appear in the agent catalog.

## Generic execution configuration

The top-level `model` selects the parent's model. `agent.default` can override
its prompt, variant, sampling, tools, permissions, iteration budget, and tool
failure behavior. Named subagent entries configure their own prompts and tools.

Events identify the parent as `default` and children by their selected subagent
ID. Configuration changes do not rewrite historical profile strings.

## Permission and toolset boundaries

The coordinator checks each agent's own toolset and permissions. A parent's
`task` permission controls whether it can start or continue a child. Its other
role restrictions do not transfer to that child. For example, a parent denied
native editing can delegate implementation to `general` if shared policy allows
it.

For child actions, the coordinator combines shared project policy with the
child's role policy. Deny wins, then ask, then allow. An existing grant can
satisfy an ask but cannot override a deny. Calls inside `batch` follow the same
checks. Primary-agent permission precedence is unchanged.

```mermaid
flowchart TD
    Parent[Parent requests a task] --> Gate{Parent has task access?}
    Gate -->|No| Block[Reject the task]
    Gate -->|Yes| Child[Prepare the selected child]
    Child --> Action[Child requests a tool]
    Action --> Tools{Tool in the child's toolset?}
    Tools -->|No| Reject[Reject the call]
    Tools -->|Yes| Policy[Combine shared and child policy]
    Policy -->|Deny| Reject
    Policy -->|Ask| Approval[Wait for approval]
    Policy -->|Allow| Execute[Coordinator executes the tool]
    Approval -->|Approved| Execute
    Approval -->|Denied| Reject
```

| Role | Default tools |
| --- | --- |
| `explore` | `read`, `glob`, `grep`, `list`, `ast_grep_search`, `webfetch`, `websearch`, `session_list`, `session_read`, `session_search`, `session_info`, `batch`, `bash`, `lsp`, `skill` |
| `librarian` | Explore's tools plus `codesearch` |
| `general` | Librarian's native tools except `skill`, plus `edit`, `write`, `apply_patch` |
| `default` | Parent tools, including task delegation and skill loading |

Research roles deny native editing, questions, delegation, and todo mutation.
They can use bash, LSP queries, and skills. Their prompts require research, but
bash and MCP can still mutate files. An edit deny therefore does not confine the
filesystem. `lsp.rename` remains unavailable to research roles.

MCP discovery adds concrete configured tools to `default`, `explore`, and
`librarian`, even with customized native tool lists. General requires exact MCP
IDs in its tool list. Stdio MCP uses the `bash` capability; HTTP MCP uses network
policy. Both shared and role policy apply. Discovery does not add generic MCP
gateway tools.

Skills provide instructions and cannot grant tools. The `skill` tool uses read
permission and the per-skill load policy. General receives skills through the
parent's `load_skills` argument. Task results report the child's prepared toolset;
individual arguments can still trigger an ask or deny.

Resume prepares children from current configuration, using the same checks as a
new spawn. It does not restore old policy snapshots. See the
[permission guide](../permissions/permissions.md) for the limits of these checks.

## Structured delegation body

New `task` calls require `subagent_type`, `prompt`, `run_in_background`, and
`load_skills`. Continuations identify the child with `task_id` or `session_id`.
For work that needs context, include these details in the prompt:

| Detail | What to tell the child |
| --- | --- |
| Context | Relevant files, modules, constraints, and prior work |
| Goal | The decision or artifact to produce |
| Downstream use | How the parent will use the result |
| Request | The work and expected output format |
| Required tools | Tools to use or avoid |
| Required checks | Tests or other evidence the task needs |
| Scope limits | Files, actions, and capabilities outside the task |

Duplicate `load_skills` names load once, at their first occurrence. Missing,
denied, disabled, malformed, or unsafe symlinked skills fail the call before the
child starts. The skill catalog reports `body_loaded: false` until activation.

Synchronous `task` calls and `background_output` return the full redacted child
report. The separate `child_summary` preview and completion notifications are
capped. Both modes use the same request-scoped lifecycle; continuing a child
session creates a new request. Blocking output checks accept waits up to five
minutes (larger values are clamped), and wait expiry leaves children running.

## Enforcement boundary

The coordinator owns event appends, scheduling, child ownership, cancellation,
permissions, and tool execution. Prompt text and TUI labels cannot grant access
or bypass those checks.

## Background history

`background_output` accepts `full_session`, `message_limit` (0–200),
`since_message_id`, `from_end`, and `include_tool_results` for one owned child.
`since_message_id` must identify that child's message; the result starts after it.
`from_end` returns the newest events and messages first. Each result includes
counts and truncation flags. Event summaries share the session-inspection redactor
and omit tool arguments and reasoning.

History queries inspect at most a 64 MiB parent journal. A returned page holds at
most 1,000 event summaries and 2 MiB of serialized events; message and tool-result
subsets keep the combined history below 4 MiB. Large tool results use the normal
private artifact path. `include_thinking` returns an explicit unavailable result:
reasoning is not retained. `thinking_max_chars` is accepted for compatibility.

Blocking reads report `timed_out` and accept either `timeout_ms` or `timeout`.
Multiple selectors require `wait_mode: "any"` or `"all"`; history options require
a single child. A wait timeout leaves the child running. Inspection after resume
reads committed history and does not call the provider again.

Background work initiated by an agent reserves one slot in the parent's prompt
queue until its completion notification is queued. New prompts and manual
compactions respect those reservations. A background launch or demotion is
rejected when no slot remains. This keeps the queue bounded without dropping
notifications from accepted children.

Outstanding tool requests, including permission waits and orchestration tools,
are capped at `command_buffer + tool_concurrency`. Further calls return a queue
error before being scheduled. Cancellation releases that capacity after cleanup;
the execution semaphore separately bounds active I/O.

The child concurrency limit counts active work. Completed children do not consume
execution capacity; their small ownership records remain available for inspection
and continuation. Completed conversation buffers are released, and immutable
profiles are shared between children. Continuing a child reloads its history.

Each child also has an `events.jsonl` and private artifacts in its own session
directory. The coordinator publishes committed child events there for TUI history,
inspection, and standalone continuation. It keeps the child's writer locked until
the parent run closes. Parent and sibling conversation text is excluded.
Compaction positions are translated to the child's event sequence. Resume fills
missing records from the parent journal without repeating tools, hooks, or provider
calls; an incomplete final write is preserved separately before repair.
