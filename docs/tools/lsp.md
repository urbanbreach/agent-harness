# Language tools

The `lsp` tool starts an installed language server after the coordinator approves
the call. It reuses that server within a run and closes it on shutdown. No server
starts during registry construction or replay.

```json
{"operation":"goToDefinition","filePath":"src/lib.rs","line":12,"character":5}
```

Queries require `filePath`; `file_path` is an alias. Lines and UTF-16 columns are
1-based; a column inside a surrogate pair is rejected. Both `lsp` and `read`
permission rules apply to the requested path and its canonical form. External
paths also require `external_directory` approval. Language servers are native
processes, and these permission checks are not an operating-system sandbox.

| Operation | Additional input |
| --- | --- |
| `goToDefinition`, `goToImplementation`, `findReferences`, `hover` | `line`, `character` |
| `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls` | `line`, `character` |
| `documentSymbol`, `fileDiagnostics`, `workspaceDiagnostics` | None |
| `workspaceSymbol` | Nonempty `query`, at most 4,096 bytes |

Results contain the server ID, its result, and any reported server status.
Protocol positions in returned results remain 0-based. Unsupported server
capabilities produce an error. File diagnostics use pull diagnostics when
available, otherwise published diagnostics for the synchronized document.
Workspace diagnostics use the server's workspace-pull capability when available.
Otherwise Harness scans up to 200 files matching that server's extensions and
requests file diagnostics, or waits for each file's published diagnostics.
The fallback reports `filesScanned`, `diagnosticCount`, `reports`, `skippedFiles`
and `complete`. Read or LSP rules that deny another file, or require a separate
approval, exclude it and set `complete` to false. The initially requested document
keeps its existing approval. Unsupported, invalid or missing diagnostic responses
produce an error rather than a clean result.

## Server configuration

Default commands are `rust-analyzer`, `typescript-language-server --stdio`,
`pyright-langserver --stdio`, and `gopls`, selected by file extension. The commands
must be available on `PATH`. Configured servers take precedence over defaults.
`serverId` selects a particular matching server.

```json
{
  "lsp": {
    "servers": {
      "custom": {
        "command": ["my-language-server", "--stdio"],
        "extensions": [".custom"],
        "env": {},
        "initialization": {}
      },
      "rust": {"disabled": true}
    }
  }
}
```

Set `lsp.disabled` to disable all servers. Configured environment values join the
run's secret registry. Server stderr is discarded; protocol payloads are not logged.
Server requests to apply workspace edits are refused. File edits use the shared
[formatter runner](formatting.md).

## Rename and edit diagnostics

```json
{"filePath":"src/lib.rs","line":12,"character":5,"newName":"answer","apply":false}
```

`lsp.rename` previews a semantic rename by default. `apply: true` requests a fresh
plan from the server and applies it. `prepareRename` is used when advertised.
Both forms require read and LSP permission; applying also requires edit permission.
Every additional file passes coordinator policy before its contents are loaded.
Read approval cannot grant edit access. Denials, cancelled approvals, invalid
ranges and stale file contents stop the operation before publication.

The tool accepts `changes` and versioned `documentChanges`, including file create,
rename and delete operations. It checks UTF-16 boundaries, overlapping ranges,
versions, local file URIs and workspace containment. Directory operations are
unsupported. Resource operations are evaluated in order in memory; final file
contents are published before deletions. This is not a filesystem transaction:
if a later commit fails, earlier committed files remain changed and retain their
normal undo receipts. Moved files retain their permissions. Previews return a
private `.diff` artifact readable by the existing TUI.

Plans are limited to 256 operations and files, 10,000 text edits per document
change, 8 MiB per file, 32 MiB of source and resulting contents, and a 1 MiB
combined preview. Unknown document versions fail rather than guessing.

Successful native writes, edits, patches, structural replacements and renames
request diagnostics for changed files using the same server connections. These
checks share a 30-second deadline, examine at most 16 files, and retain at most
1 MiB of diagnostic output. Read or LSP rules requiring separate approval skip
the check and report it as unavailable. A failed diagnostic check does not undo
or misreport a completed edit. Disabling LSP disables these checks too.

## Installation choices

```json
{"operation":"installDecision","serverId":"rust","decision":"declined"}
```

This operation records `allowed` or `declined` without a file path or a running
server. The coordinator checks LSP permission for the server ID and writes an
immutable private receipt, linked from the journal and returned in `artifacts`.
Later choices leave earlier receipts intact. `recorded_only: true` distinguishes
this record from an installation result. The operation never installs software
and does not grant permission to execute another tool. It also works when LSP
queries are disabled.

## Resource limits and verification

Each registry allows 16 live servers and runs one LSP call at a time. Each server
retains hashes and versions for up to 64 documents, rather than keeping their full
contents. Changed documents use the server's full or incremental synchronization
mode. The first path in sort order is closed when the document limit is reached.

Source files are limited to 512 KiB. Protocol headers are limited to 8 KiB and
messages to 4 MiB. Stored diagnostics and combined call-hierarchy output are also
bounded to 4 MiB. Server operations have a 30-second deadline. Cancellation and
shutdown terminate the process group and wait for the server to exit.
The workspace fallback stops after 100,000 directory entries, skips symlinks,
version-control directories, build directories and default Harness storage, and
joins its filesystem scans before returning. It retains one source buffer at a
time; the existing 64-document server limit still applies.

The client follows [LSP 3.17 framing and lifecycle rules](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).
For rust-analyzer, it waits for the initial
[workspace readiness notification](https://rust-analyzer.github.io/book/contributing/lsp-extensions.html#server-status)
before issuing queries. Reported workspace errors fail the operation; warnings
remain visible in the result's status.

Native checks cover document synchronization, UTF-16 validation, server reuse,
diagnostics, rejected server edits, cancellation, and process cleanup. A separate
check runs the installed rust-analyzer against a temporary Rust project and
verifies symbols, definitions and semantic rename. Rename checks also cover
preview artifacts, separate read/edit approvals, stale sources, cancellation,
UTF-16 ranges, ordered resource operations, file permissions and undo. Run them with:

```bash
HARNESS_BINARY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tools \
  --test binary_smoke --ignore-default-filter
```
