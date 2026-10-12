# Monitor

`monitor` starts a background watch and returns immediately. Use it when a build,
test, server, or log will print the condition you need, or when you need to know
that a file appeared or changed. Keep working while it runs.

The default profile exposes it directly. Inside eval, call
`tool.monitor({...})`; it disappears from the direct tool list only when
`eval.route_tools` lists `monitor`. It has the Shell capability.

## Arguments and results

All arguments belong to one flat object.

| Argument | Meaning |
| --- | --- |
| `action` | `create` (default) or `rearm`. |
| `description` | Required for create, 1-200 characters after trimming whitespace. A short human label for the watch. |
| `command` | Shell command. For create, supply exactly one of `command` or `path`. |
| `path` | File to watch. The parent directory must exist; symlinks are rejected. |
| `event` | Path only: `create` (default) or `modify`. |
| `filter` | Command only: an unanchored, case-sensitive Rust regex. No lookaround or backreferences. Omit it to match every non-empty line. |
| `timeout_ms` | 1-3,600,000 milliseconds; default 300,000 (five minutes). |
| `task_id` | Rearm target. Omit it or use a blank string to resume every paused monitor owned by the caller. |

Unknown arguments are rejected. Command watches reject `event`; path watches
reject `filter`. Send only `action` and an optional `task_id` for rearm.
That call uses `task` permission for `monitor rearm` and does not launch a new
command or file watch.

Create text starts `Monitor started with task_id <id>.`. The structured result
has `type: "Monitor"`, `task_id`, `description`, `command`, `path`, `event`,
`filter`, `status: "running"`, `output_file`, and a `notifications` boolean.
Unused source fields are null. Rearm returns one line of text and
`{"type":"MonitorRearm","resumed":["<id>"],"dropped":0}`. `dropped` counts
matching lines discarded while the resumed monitors were paused.

Use the returned ID with `get_command_or_subagent_output`,
`wait_commands_or_subagents`, or `kill_command_or_subagent`. A watch is a
coordinator background task, not a detached eval cell. It records
`TaskScheduled(Started)` with `queue_key: "monitor"` and the initiating tool
call as `parent_tool_call_id`. A successful exit records `TaskCompleted`;
failure or timeout records `TaskCancelled` with `failure: true`. Kill and
shutdown record cancellation with `failure: false`.

## Sources and permissions

Command watches use the same validation, `bash` permission, command scanner,
allowlists, and optional OS sandbox as [bash](shell.md). They run in the workspace
root in their own process group, with stdin closed, `TERM=dumb`, and `NO_COLOR=1`.
A pseudo-terminal merges stdout and stderr into a line-buffered stream. Output
is retained like background commands: an 8 MiB log and a 40,000-character preview.
Timeout kills the process group. A watch cannot answer an interactive prompt.

Path watches require `read` permission on the path. Harness checks every 250 ms.
`create` fires when a previously absent file appears. An already existing file
is not an immediate hit, but deleting and recreating it can trigger the watch.
`modify` requires an existing regular file and fires when its size, inode, mtime,
or ctime differs from the initial snapshot. A hit prints `create <path>` or
`modify <path>`, then completes. These watches do not survive a session restart.

## Events and limits

Only newline-terminated command lines are considered. Harness strips ANSI
escapes before matching and truncates lines longer than 1,000 characters.
Matching lines batch for two seconds. Each monitor delivers at most once every
five seconds. A delivery holds at most 50 lines and about 4 KB across monitors;
extra lines are counted as omitted, with a pointer to
`get_command_or_subagent_output`. An identical repeat of a monitor's previous
line-only batch is skipped.

When notifications are enabled, exits are always reported, even if no line
matched: `monitor completed (exit code 0)`, `failed (exit code N)`, or `timed out`.
Kill and session stop do not send an exit notice.

Events reach the model as durable `RuntimeReminder` records of kind
`monitor_event`, wrapped in a system reminder. They do not authorize another
command.

In the TUI, a monitor call is one transcript line, `Monitor <description> ·
<command>` (or `Rearm monitors`), with no output body; a failed call is marked
like other failed tool rows and keeps its error in the full view. Events stay out
of the transcript, and running monitors are not listed in the tasks pane or the
"still running" line. The row under the input box shows `◉ watching
<description> (12s)` or, for several watches, `◉ watching 3: a, b (+1 more)
(1m3s)`. Elapsed time starts at the oldest watch. The text is capped at 48
display cells; ` muted` means all watches are paused, and ` 2 muted` means two of
them are paused.
Clicking the watching status opens the status dashboard. In ASCII mode, the
watching marker is `o` instead of `◉`.

## Wake budget and rearm

Five consecutive monitor deliveries without a tool call or user message between
them pause the contributing command monitors. A delivered exit or a quiet gap
over ten seconds resets the count. The delivery that pauses a watch says so.

A paused command keeps running and retaining output, but drops matching lines
instead of delivering them. Its exit is still reported. Resume it explicitly:

```json
{"action":"rearm","task_id":"<returned task_id>"}
```

Use `{"action":"rearm"}` for all of the caller's paused watches. Any user
message also resumes paused monitors. Pause and resume publish a live,
non-durable `MonitorState` event for the footer; rearm cannot recover discarded
event lines, so retrieve retained output if those lines matter.

## Notifications and headless runs

`runtime.behavior.command_notifications` governs both ordinary background
command completion notices and monitor events. With `enabled: false`, no
monitor events are delivered; use the wait and output tools instead.

Headless `harness run` and `harness prompt` stop the session when the requested
turn ends, which also kills its monitors. Events reach the agent only while that
turn is still working, so a headless agent that needs a monitor's result waits
for it with `wait_commands_or_subagents` before it finishes.

With `enabled: true` and `wake_idle: false`, events wait for the next turn rather
than waking an idle agent. There are no separate monitor tuning settings.

## Examples

A Rust build prints a success line or error lines. Filter for those rather than
sending the full compiler stream:

```json
{"description":"Rust build result","command":"cargo build","filter":"Finished|error:","timeout_ms":600000}
```

A development server can stay running after it announces readiness. Match the
actual ready line your server prints, and kill the watch when you no longer need
that server:

```json
{"description":"Dev server ready","command":"npm run dev","filter":"ready|Local:"}
```

Watch a long test run for failures. The exit notice still reports the final
outcome, even if every test passes and the filter matches nothing:

```json
{"description":"Test failures","command":"cargo nextest run --profile ci","filter":"FAIL|error:","timeout_ms":3600000}
```

Wait for a generated file without launching a shell:

```json
{"description":"Build artifact appears","path":"target/release/app","event":"create"}
```

For a one-shot condition that has no file event, use an approved project script
with a loop that prints a sentinel such as `READY\n` and exits when the condition
holds. Watch that script with `filter: "^READY$"`. Keep the loop in the watched
command, not in an eval cell, and obey bash's command-validation rules.

## Avoid these patterns

- Sleeping, repeatedly checking output, or starting a subagent just to wait.
- Holding or detaching an eval cell for a watch instead of calling `monitor`.
- Omitting `filter` on a noisy stream when only one condition matters.
- Printing a sentinel without a newline, or expecting a filter to match terminal
  progress updates that never finish a line.
- Treating a `create` watch as an existence check for a file already present.
- Leaving an unused server or log watch running. Cancel it with
  `kill_command_or_subagent`.
- Assuming rearm restarts the command, or that a paused watch has stopped it.
