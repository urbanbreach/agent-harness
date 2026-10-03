# Agents and tasks

Harness uses one interactive parent named `default`. Children resolve their
definitions through the configured CLI, project, user, plugin, and bundled
sources. The default child type is `general-purpose`. Title generation and
context compaction are internal operations, not selectable agents.

See [subagent configuration](../configuration/config.md#generic-agent-and-subagents)
for definition discovery, model selection, inheritance, and concurrency settings.

## Starting work

`spawn_subagent` requires `prompt` and `description`. It accepts an optional
`subagent_type` and defaults to `background: true`. A background call returns a
`subagent_id`. With `background: false`, the caller waits for completion; reaching
the foreground timeout leaves the child running in the background.

Include the relevant files, constraints, expected output, and required checks in
the prompt. `isolation: "worktree"` creates an isolated working tree. Alternatively,
`cwd` selects an existing directory; it cannot be combined with worktree isolation.

`resume_from` creates a distinct child from a completed child's finalized
conversation. Ownership, retained-state availability, and context limits are
checked before admission. This differs from waking the same child through a
message. A wake retains that child's identity and prepared definition. Missing
or incompatible retained state returns an error rather than reconstructing a
conversation by repeating tools or provider calls.

Retained conversations preserve tool errors and missing provider usage exactly.
Missing usage keeps accounting marked incomplete; it does not prevent resuming
an otherwise complete conversation. The native terminal `tokens_used` counter
tracks the last provider context total, starting from the child's initial
conversation estimate when no usage has arrived. It is separate from cumulative
billable usage; an estimated context count does not make missing billing data
complete.

Prompt turns and executed model tool calls accumulate for the child's identity,
including message wakes and restart. A distinct child created with `resume_from`
starts its own counters. Calls rejected before execution and startup skill
preloads do not increase the model tool-call count.

Running-child output includes current prompt turns, executed tool calls, distinct
tool names, provider errors, elapsed time, and context usage sampled before model
requests. The live context estimate adds new conversation items to the last
reported total and excludes model output already covered by that total. Unknown
context capacity remains unknown.

The coordinator publishes live child progress every two seconds when counters
change, with an eight-second heartbeat while unchanged. The Tasks pane keeps
updating while a child is open; stale attempts and updates after completion are
ignored. These progress updates are transient and are not appended to history.

Children receive the subagent base instructions followed by their definition's
prompt body. Tool placeholders use the child's available tools, and the workspace
path reflects its prepared directory or worktree. A same-identity wake keeps the
rendered prompt, including after restart.

## Permissions and skills

The caller needs `spawn_subagent` in its toolset and the shared `task` permission
for the requested definition. Each child uses its resolved tools and policy
under the shared project policy. Skills cannot grant tools or override these
checks. Permission policy is not an operating-system sandbox; see the
[permission guide](../permissions/permissions.md).

Definitions list startup preloads in `skills`. With `inherit_skills: true`, the
child receives its actual spawner's startup catalog. Otherwise, discovery uses
the child's effective working directory and default roots; `discover_skills:
false` suppresses that discovery. Catalog inspection does not load skill bodies.

**Accepted parity exception:** Grok explicitly preloads definition-listed skills
even when ordinary skill calls are disabled or denied. Harness keeps shared
skill permissions for both startup preloads and ordinary calls. Missing, denied,
disabled, malformed, or unsafe preloads fail preparation before the child runs.

**Accepted clipboard exception:** Child-viewer selection and copy use Harness's
shared clipboard integration. Unlike Grok, Harness does not write every copied
selection to a persistent backup file or display that backup path in a toast.
Mouse release copies a nonempty selection and keeps it highlighted; Escape
clears it before closing the viewer. `y` copies the current logical line or visual
range, clears the selection, and briefly shows `Copied!`.

A same-identity wake keeps the original catalog and preload cache. Finalized
private state retains the body-free catalog and preload names. A live wake after
restart reloads preloads through the normal permission gates. Replay never
rediscovers or loads skill files. Older state without a retained catalog requires
a fresh child.

## Output, waiting, cancellation, and messages

| Tool | Behavior |
| --- | --- |
| `get_command_or_subagent_output` | Read `task_ids`, optionally waiting up to `timeout_ms`. Returns status, output, timing, and truncation metadata for owned children or background commands. |
| `wait_commands_or_subagents` | Wait for `task_ids` with `mode: "wait_any"` or `"wait_all"` and optional `timeout_ms`. A timeout leaves work running. |
| `kill_command_or_subagent` | Cancel the selected `task_id` through the coordinator and its cleanup path. |
| `send_subagent_message` | Send `text` to an authorized `subagent_id` using `delivery: "steer"`, `"queue"`, or `"interject"`; omission selects steering. Admission and quota failures return structured outcomes. |

Read, wait, and kill tools remain available for background commands when spawning
is disabled. Hidden aliases support old tool names, but new callers use the
public names above. The former `task(load_skills = [...])` interface and
`background_output` history options are superseded.

The coordinator owns scheduling, permission checks, message admission, child
ownership, cancellation, lifecycle transitions, and event appends. UI actions
submit intents to that owner. Waiting does not hold an execution slot needed by
the work being awaited. Completion notifications and messages are committed
before they are delivered. A background completion reaches the parent once,
through either its wake prompt or a buffered reminder. Polling a finished child
consumes its pending completion notification.

An idle parent's wake combines its buffered child reports into one reminder.
During an active turn, reports appear in the next completed tool result. These
reminders use the polling output format and escape embedded reminder tags. When
the parent has an output polling tool, each report is limited to 16,000 UTF-8
bytes with a pointer to the full result; otherwise the report is included in full.

## History and inspection

Children have their own session journals and private artifacts. Finalized
conversation state preserves the completed context needed for resume, including
settled assistant content. Live provider fragments and raw wire payloads remain
transient. Support exports omit provider reasoning and redact sensitive values.

Replay and inspection read committed history without running tools, hooks, skill
discovery, or provider requests. Missing child history is reported as unavailable.
Opening and closing a child view preserves the parent's conversation and draft.

The Tasks pane combines child work and background commands. Open a child with
Enter. Within the read-only child view, `q` or Esc returns to the parent, Ctrl+C
requests child cancellation, and Ctrl+E toggles reasoning visibility. Vim mode
adds transcript navigation and link selection; Enter opens the selected link or
entry. Search and other overlays own their input until dismissed.

The child block viewer follows live output until upward navigation detaches it.
Enter quotes the selected logical line or selection into the parent draft.
With Vim navigation enabled, `r` toggles the selected Markdown entry in place.
That entry retains its mode when its block viewer closes and reopens; the viewer
also accepts `r` directly.
Raw/Markdown toggles reproduce the pinned Grok renderer's source-map behavior:
it numbers the unfrozen tail from zero after a mode change. A later toggle can
therefore select an earlier line, including a blank line near the start. This
cursor jump is retained for parity.
