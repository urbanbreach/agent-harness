# Sessions and replay

A session is an append-only `events.jsonl`, private metadata, and referenced
artifacts. The event sequence is authoritative. Catalogs and UI projections can
be rebuilt; they cannot authorize provider or tool work.

## Storage layout

The CLI stores generated data outside the project in the Harness home. A
nonempty `HARNESS_HOME` is used as-is; otherwise the root is `$HOME/.harness`.
The `<data-dir>` paths below refer to this root, which also holds personal
runtime and keyboard config, credentials, prompts, agents and skills.

Each project uses `v2-` followed by the full, lowercase BLAKE3 hex digest of its
canonical absolute UTF-8 path. The key is always 67 bytes, so long project paths
fit within filesystem component limits. Symlink aliases share storage; paths
that differ in slashes, backslashes, colons or dashes have distinct keys.

```text
<data-dir>/sessions/<project-key>/<run-id>/events.jsonl
<data-dir>/projects/<project-key>/
<data-dir>/worktrees/<project-key>/
```

Session metadata and artifacts stay with their run. Memory, the code index and
edit attribution use the project runtime directory. Managed Git worktrees use
the worktree directory. Project-authored agents, skills, prompts and configuration
remain in the project, as do `.harness/permission-grants.json` workspace grants.

An empty `runtime.session_dir` selects the managed session directory automatically.
A nonempty setting or `--session-dir` overrides only session storage; relative
paths are still resolved against the selected project directory. Existing
project-local session histories are not migrated or discovered automatically.
Use `--session-dir <old-session-dir>` to access them explicitly.
See [saved sessions](../operations/sessions.md) for command examples.

## Legacy storage

Older builds used `--path--` keys, replacing path separators and colons with
dashes. Different projects could share one directory. Harness leaves those
directories intact and does not load or migrate them automatically: memory and
plans do not contain enough ownership information to separate mixed data safely.

Open old sessions explicitly with `harness --session-dir <old-session-dir>
sessions list` or set `runtime.session_dir` to that directory. Inspect each
session's recorded workspace before resuming it.

To move data to the new layout, stop Harness processes using it and back it up.
`harness memory list --workspace <project>` reports the new runtime location as
`store_path` without creating it; its project key also applies to sessions and
worktrees. Move only data verified to belong to that project, and never overwrite
an existing destination. Whole `sessions/<old-key>` or `projects/<old-key>`
directories can be renamed only when all their contents belong to the same
project. If ownership is uncertain, keep the old data separate. Move Git
worktrees individually with `git worktree move` so Git updates its registrations;
`harness worktree list --all` includes checkouts still at legacy locations.

## Inspection

```bash
harness sessions list
harness sessions inspect <run-id-or-path>
harness sessions tree --root <run-id-or-path>
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

## Compaction records

A committed pass appends one `SessionCompaction` event. It carries the summary,
the method, the first kept event sequence (`first_kept_event_seq`, the event's own
sequence when nothing was kept), token counts, file lists, an optional `warning`,
hook `details` and `preserve_data`, `from_hook`, `operation_id` and, for a native
pass, a `native` block with the lane, provider, model, wire API and payload
reference. Archive frames are metadata only (path, digest, size, grid); the
images live in private attachment artifacts. Nothing in the event is edited later.

`CompactionPatched` records amend an earlier compaction by `compaction_seq`.
A `warning` patch attaches a warning found after the commit, such as a dead-end
recovery or an oversized kept tail; the newest one wins. A `reader_view` patch
holds the reasoning-free summary and archive for one reader key. Resume hands the
newest view per key to the request caches, and a model that does not read
archived reasoning uses it, while the original summary and archive stay intact for
readers that do.

A rescue that has to write a second boundary sets `supersedes_seq` on the new
compaction. The superseded compaction no longer shows as its own card, and
supersession is per agent, so a child's compaction never replaces the root's.

`ContextNotes` records are revisions of an agent's notebook: `agent_id`, a
`version` (only `1` exists) and the redacted `text`, at most 16,384 UTF-8 bytes.
An empty text clears the notebook. The newest valid revision on the active fold
is the notebook; an oversized or unknown-version record never shadows an earlier
valid one, and a rewind past a revision restores the one before it. A save is
refused if the branch changed (rewind, agent stop or disabling the setting)
between starting and appending it.

Provider-native compaction payloads are the one provider-originated state that
reaches disk. Commit writes the payload as `artifacts/native-<blake3>.bin`, a
private (0600) file, then appends an `ArtifactWritten` event with metadata
`kind: native_compaction` and the `SessionCompaction` event that references it
by path, digest and size. The file is limited to 32 MiB. A load succeeds only when
the path is the canonical name for the digest, the file opens as private storage
(symlinks are refused) with the recorded size and BLAKE3 digest, the content parses, the
recorded wire API matches, and the payload belongs to the live provider and
model. Anything else counts as unreadable.

Rebuilding a context after resume or fork picks the newest readable boundary. A
compaction that replaced history through an OpenAI native lane is readable only
when its payload loads for the live provider, model and API; if it does not, the
previous readable compaction applies and the originals that boundary kept return
to the context. Every other compaction is readable from its summary text alone.
`provider_replay_through_seq` records the last journal event a native snapshot
covers. When a payload is attached, only entries after that sequence replay; the
rest of the history is the payload. Entries are kept by sequence number against the
boundary, so a turn that continues past a compaction that kept nothing survives,
and a missing kept boundary never fails replay. Resume of a child restores the
same boundary through the same loader.

Support exports and trace archives drop `native_compaction` artifact records and
the `native.payload` reference. The session catalog does not count the payload
files as artifacts. Choosing a boundary reads local files only; replay never
contacts a provider.

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
opens the conversation rewind picker. It lists newest prompts first,
dims the conversation from the selected prompt, and asks for confirmation by
default. A running turn must be cancelled first. Rewind removes the selected
prompt and later conversation from the active projection and provider context,
restores that prompt's text to the composer, and shows "Reverted conversation"
for three seconds. Workspace files stay as they are.

## Live overlays

Live provider text, reasoning, and tool-input fragments are an ephemeral TUI overlay. Settling a
turn removes that overlay and displays the single durable semantic assistant commit from
`CanonicalSessionProjection`; replay never reconstructs draft fragments as durable messages.
Finalized agent artifacts retain semantic conversation and execution metadata.
Exact provider inputs, reasoning and raw tool results remain in process memory.
Live continuation after restart uses the semantic snapshot; inspection and replay
do not dispatch providers or rerun tools.

## Exports

Support export validates the journal, redacts values, omits provider reasoning,
raw tool result JSON and native compaction payloads, and scans keys and string values
before publication.
It does not copy or validate attachment blobs as a backup would. A secret finding
prevents output. Archives use bounded input and atomic replacement.
These exports are support evidence; they are not a raw backup of credential or
attachment storage.

`harness export` writes the active conversation's user and assistant text as
Markdown. `harness sessions export` writes a JSON support bundle. `harness trace`
packages sanitized journal, metadata and support files locally; it never uploads
them. `harness wrap` packages workspace files and can include those diagnostics
when explicitly configured with in-workspace session storage. See
[export and archive options](../operations/sessions.md#export).
