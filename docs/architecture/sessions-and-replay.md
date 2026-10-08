# Sessions and replay

A session is an append-only `events.jsonl`, private metadata, and referenced
artifacts. The event sequence is authoritative. Catalogs and UI projections can
be rebuilt; they cannot authorize provider or tool work.

## Storage layout

The CLI stores generated data outside the project. The data directory is the
first available location in this order:

1. `$HARNESS_DATA_HOME/harness`
2. `$XDG_DATA_HOME/harness`
3. `$HOME/.local/share/harness`

Empty environment values are ignored. These locations are separate from the
XDG configuration directory.

Each project uses a key derived from its canonical absolute path, so symlink
aliases share storage. Remove one leading `/` or `\`, replace every `/`, `\`
and `:` with `-`, then wrap the result in `--`. For example, `/work/app` becomes
`--work-app--`. There are no home-relative shortcuts or hashed buckets.

```text
<data-dir>/sessions/<project-key>/<run-id>/events.jsonl
<data-dir>/projects/<project-key>/
<data-dir>/worktrees/<project-key>/
```

Session metadata and artifacts stay with their run. Memory, the code index and
edit attribution use the project runtime directory. Managed Git worktrees use
the worktree directory. Authored agents, skills, prompts and configuration remain
in the project, as do `.agent-harness/permission-grants.json` workspace grants.

An empty `runtime.session_dir` selects the managed session directory automatically.
A nonempty setting or `--session-dir` overrides only session storage; relative
paths are still resolved against the selected project directory. Existing
`.agent-harness/sessions` histories are not migrated or discovered automatically.
Use `--session-dir <project>/.agent-harness/sessions` to access them explicitly.
See [saved sessions](../operations/sessions.md) for command examples.

## Inspection

```bash
harness sessions list
harness sessions inspect <run-id-or-path>
harness sessions tree <run-id-or-path>
harness replay --session <path> --json
```

Inspection, replay and readiness do not execute providers, tools, hooks, MCP
servers or network calls. Scenario fixtures are omitted from the operator history
list but remain directly inspectable. The four model-visible session tools provide
bounded list, history, search and metadata results under the same read-only rule.
See [session tools](../tools/sessions.md) for their selectors and limits.

Catalog reads compare directory and file fingerprints and reuse unchanged rows.
Malformed histories become unavailable rows without hiding healthy sessions.
Opening, exporting or resuming a selected session validates its journal directly;
a cached catalog entry never replaces that check.

Historical cron, team, plugin, and binary-update events retain their durable
schema variants and replay projections after their CLI producers were removed.
Inspection never reruns their old operations or loads an extension package.

## Resume and recovery

Resume acquires the writer lock, validates the journal, restores agent bindings,
messages, tool results, attachments, model selections, todos and compaction, then
terminates interrupted work. New work starts only when a caller submits a turn.
Historical tools and hooks are never repeated.

Recovery may preserve and remove one incomplete, unterminated final JSON record.
A valid final event missing only its newline is normalized before appending.
Terminated invalid JSON, corruption inside the journal, unknown writer ownership
and conflicting active writers fail closed. Recovery retains the removed tail
for diagnosis. Read-only replay does not repair files.

Permission checks use the current policy and registered agent identity. Stored
credentials and newly refreshed credentials are redacted from historical output
before it is exported or returned to a model.

## Forks and children

Fork and clone create a new session from a validated stable prefix. They copy
referenced artifacts after path, length and digest checks, regenerate run/event
identity and preserve the source. Compaction cutoffs are translated to the new
sequence. Missing or corrupt artifacts fail the operation instead of producing a
partially valid child.

Delegated agents share the coordinator and provider capacity. Each also has a
standalone journal containing its own events, artifacts and recorded model/profile.
The root journal remains authoritative. Resume repairs a missing or partial child
projection from that source, without repeating any work.

The parent owns a child journal until parent shutdown. Completed children release
their conversation buffers and journal descriptors; the root writer lock prevents
another writer from taking over. Continuation reloads the child's history. A
standalone child writer prevents a concurrent parent resume. Forking a parent
assigns new child session IDs, so continuing the fork cannot mutate source children.

## Rewind

`ConversationRewound` excludes a range from the active conversation. Replay and
resume apply the same exclusion; the source journal is never truncated. Native
file checkpoints support a separate, explicit workspace restore with conflict
checks and bounded private baselines. Replaying `WorkspaceReverted` never writes
workspace files. See [session commands](../operations/sessions.md) for snapshot,
branch, recovery and export options.

`/rewind` (alias `/undo`, or double Esc on an empty idle prompt within 800 ms)
opens the Reference-style conversation rewind picker. It lists newest prompts first,
dims the conversation from the selected prompt, and asks for confirmation by
default. A running turn must be cancelled first. Rewind removes the selected
prompt and later conversation from the active projection and provider context,
restores that prompt’s text to the composer, and shows “Reverted conversation”
for three seconds. Workspace files stay as they are.

## Live overlays

Live provider text, reasoning, and tool-input fragments are an ephemeral TUI overlay. Settling a
turn removes that overlay and displays the single durable semantic assistant commit from
`CanonicalSessionProjection`; replay never reconstructs draft fragments as durable messages.

## Exports

Support export redacts values, omits provider reasoning and raw payloads, validates
referenced artifacts, and scans the complete staged result before publication.
A secret finding prevents output. Archives use bounded input and atomic replacement.
These exports are support evidence; they are not a raw backup of credential or
attachment storage.
