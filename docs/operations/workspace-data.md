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

Memory lives in `<data-dir>/projects/<project-key>/memory/entries.json`, outside
the workspace. The [storage layout](../architecture/sessions-and-replay.md#storage-layout)
defines data-directory resolution and canonical project keys; `--workspace`
selects the project whose runtime data is used. Reads leave a missing store
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

The index lives at `<data-dir>/projects/<project-key>/code-graph-index.json`.
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
`<data-dir>/projects/<project-key>/edit-attribution.jsonl`. Rejected edits leave
attribution intact.
Diff and blame compare the latest agent baseline with the current file, including
external edits and deletions. These reports do not modify files or history.

Attribution retains one baseline per path, with limits of 1,024 paths, 8 MiB per
snapshot and 64 MiB per journal. It verifies the completed edit's digest before
recording. Snapshots containing recognized or registered credentials are refused.
The file edit remains successful and the runtime emits an attribution warning.
Reports redact configured credentials and credential patterns from current file
content. Files outside the run workspace have no attribution entry.

## In-session input queue

The live TUI still queues follow-up input when Enter is pressed during a running
turn. Steering input uses Alt+i or Ctrl+Alt+Enter, and interrupted-turn recovery
can restore queued input. These runtime/composer operations are separate from the
removed file-backed `prompt-queue` CLI; no command reads or drains its old store.

## Worktrees

```bash
harness worktree list
harness worktree list --all
harness worktree remove SLUG --keep-branch
harness worktree cleanup
```

Worktree commands use Git and return JSON. Listing selects managed worktrees under
`<data-dir>/worktrees/<project-key>`; `--all` includes other checkouts. The key
comes from the canonical absolute project path, as described in the
[storage layout](../architecture/sessions-and-replay.md#storage-layout). Remove selects a
managed slug, and cleanup attempts each managed checkout. Dirty worktrees are
retained unless `--force` is explicit. Removal also deletes a `harness/wt-*` branch
unless `--keep-branch` is set. Primary and unmanaged worktrees cannot be removed.
Partial cleanup reports each failure and exits nonzero.
