# Workspace memory, code index and edit attribution

These commands use the working directory selected by `--cwd`. Add `--workspace
DIR` after `memory`, `code-graph` or `attribution` to select another existing
directory. Results are JSON.

## Memory

```bash
harness memory put build-command 'cargo check --workspace'
harness memory get build-command
harness memory search cargo
harness memory list
```

Memory lives in `.agent-harness/memory/entries.json`. Reads leave a missing store
absent. Writes lock and atomically replace the store, retaining other writers'
entries. Credential patterns are redacted before storage. Values are limited to
64 KiB and the store to 4 MiB. A missing key or malformed store returns an error.

## Code index

```bash
harness code-graph build
harness code-graph query CoordinatorHandle
harness code-graph query start_run --kind callers
harness code-graph query start_run --kind references
```

Builds scan supported source files and honor ignore files. The index contains
lexical symbol definitions, calls and references. It does not resolve types or
imports. Use LSP queries when language-server resolution is needed.

Queries support `symbol_def`, `callers`, `callees` and `references`. Call queries
exclude plain references. Queries never rebuild the index; a missing or invalid
index returns a structured unavailable result and a nonzero exit code. Rebuilding
replaces stale symbols atomically. Limits are 4,096 source files, 1 MiB per file,
32,768 symbols, 65,536 edges and 16 MiB of serialized index data.

## Edit attribution

```bash
harness attribution diff src/lib.rs
harness attribution blame src/lib.rs
```

Successful native edits record their final content through the coordinator in
`.agent-harness/edit-attribution.jsonl`. Rejected edits leave attribution intact.
Diff and blame compare the latest agent baseline with the current file, including
external edits and deletions. These reports do not modify files or history.

Attribution retains one baseline per path, with limits of 1,024 paths, 8 MiB per
snapshot and 64 MiB per journal. It verifies the completed edit's digest before
recording. Snapshots containing recognized or registered credentials are refused.
The file edit remains successful and the runtime emits an attribution warning.
Reports redact configured credentials and credential patterns from current file
content. Files outside the run workspace have no attribution entry.

## Cron receipts

```bash
harness cron fire-due --minute 30 --hour 14 'review:30 14 * * *'
```

This command evaluates the supplied schedules and records due entries in
`.agent-harness/cron-journal/cron-fires.jsonl`. It does not start a timer or execute
the schedule's payload. `--journal-dir DIR` selects another receipt directory.
Repeated evaluation of a schedule at the same supplied civil time produces one
receipt. The stored civil tuple has no year, so this command is unsuitable for an
autonomous annual scheduler. Schedule IDs containing credential patterns are
rejected before any write.

## Team mailboxes

```bash
harness team create implementation
harness team add-member team_1 writer general
harness team add-member team_1 reviewer general
harness team send team_1 writer 'Review the changes' --to reviewer
harness team deliver team_1 reviewer
harness team list
harness team cancel team_1
```

Use the team ID returned by `create`. Team records and messages live in
`.agent-harness/team-mailbox.json`. Each command reloads durable state. `deliver`
drains that member's pending messages; omitting `--to` gives each current member a
copy to consume. Message bodies are redacted before storage. Listing an empty
workspace creates no files. These commands manage local mailbox records; creating
a team does not start agent workers.

## Prompt queue

```bash
harness prompt-queue enqueue 'Review the tests' --session ./sessions/run-1
harness prompt-queue interject 'Check the migration first' --session ./sessions/run-1 --turn-running
harness prompt-queue list --session ./sessions/run-1
harness prompt-queue dequeue --session ./sessions/run-1
```

The queue lives at `SESSION/tui/prompt-queue.json`. Enqueue appends; interject
inserts at the front. `--turn-running` records the caller's report of an active
turn. Queue operations do not execute prompts, cancel work or change conversation
events. List and dequeue leave a missing queue absent.

Updates lock and atomically replace the store, so concurrent writers retain each
other's entries. IDs must be unique within the queue; omit `--id` to generate one.
Text is trimmed and credential patterns are redacted. Limits are 256 entries,
64 KiB per prompt and 4 MiB per store. Invalid documents and unsupported versions
remain unchanged after a failed operation.

## Worktrees

```bash
harness worktree list
harness worktree list --all
harness worktree remove SLUG --keep-branch
harness worktree cleanup
```

Worktree commands use Git and return JSON. Listing selects managed worktrees under
`.agent-harness/worktrees`; `--all` includes other checkouts. Remove selects a
managed slug, and cleanup attempts each managed checkout. Dirty worktrees are
retained unless `--force` is explicit. Removal also deletes a `harness/wt-*` branch
unless `--keep-branch` is set. Primary and unmanaged worktrees cannot be removed.
Partial cleanup reports each failure and exits nonzero.

## Plugin packages

```bash
harness plugin discover --workspace .
harness plugin install ./review-package
harness plugin activate review.plugin
harness plugin upgrade review.plugin ./review-package-v2
harness plugin deactivate review.plugin
harness plugin remove review.plugin
harness plugin list
```

Install validates a local `extension.manifest.json` under the workspace and records
the package as disabled. Activate is an explicit permission grant to validate
package entries and write a load receipt. It does not execute shell commands,
native libraries or WebAssembly. The legacy `loads_code` field reports entry
receipt state, not process execution.

Upgrade preserves enablement after validating the replacement's identity and
entries. Remove requires a disabled package and leaves its files in place.
Lifecycle state persists at `.agent-harness/plugins.json`.

Discover scans the workspace root and its immediate subdirectories. It registers
static descriptors at `.agent-harness/extension-registry.json`; discovery neither
installs nor activates packages. Commands return JSON and accept `--workspace`.
