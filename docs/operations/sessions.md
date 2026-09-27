# Saved sessions

`dashboard list`, `dashboard recent --limit N` and `dashboard status` provide
compact views of the same visible session catalog. Add `--json` for structured
output. Status also reports whether runtime configuration was loaded. These
commands do not create directories, rebuild indexes or contact providers.

## Snapshot rewind

```bash
harness sessions rewind RUN_ID --cutoff 20 --dry-run --json
harness sessions rewind RUN_ID --cutoff 20 --workspace . --snapshot snapshot.json --json
```

Dry-run projects the conversation through the cutoff and reports retained and
discarded event counts. Applying also restores an explicit JSON array of
`{"path":"relative/file","content":"saved text"}` entries. The report sets
`conversation_projection_only` because this command leaves the journal and its
active conversation unchanged; it does not perform the live conversation rewind.

Restoration runs through the coordinator and requires exclusive access to the
saved session. All paths and contents are checked before the first file changes.
Paths must stay within the chosen workspace, cannot use symlinks, and cannot
replace session storage or permission grants. Existing permissions are preserved;
new files are private. Detected failures roll back completed file replacements
without overwriting concurrent editor changes.

Limits are 1,024 files, 8 MiB per file and 64 MiB of restored plus original file
data. The snapshot document and inspected journal are each limited to 64 MiB.
The command reports unchanged files separately and never rewrites source history.

Session commands use `--session-dir DIR`, then `runtime.session_dir` from
configuration, then `.agent-harness/sessions` under the working directory. A session selector is its
directory name or an explicit directory path.

```bash
harness sessions list --json --status finished --resumable true
harness sessions search compiler --json
harness sessions inspect --run RUN_ID --json
harness sessions replay RUN_ID --json
```

Listing supports `--profile`, `--search`, `--limit`, `--offset` and `--cursor`.
Sort orders are `updated_desc`, `updated_asc`, `run_id_asc` and `run_id_desc`.
Each JSON row includes a cursor for the next page. A cursor becomes invalid when
its session changes or no longer matches the filters.

Missing storage produces an empty list without creating it. Corrupt histories
remain visible as unavailable entries with a reason. Inspection, search and replay
do not execute tools or contact providers. Catalog scans accept at most 10,000
session directories; each journal read is limited to 64 MiB. Configuration and
credential files are read locally to redact current credentials from old output.

`harness sessions rebuild-index --json` writes a private, bounded catalog cache.
Later lists reuse entries whose journal and metadata file fingerprints still
match. Changed entries are read again. Missing, corrupt and unsupported caches
fall back to journals; ordinary readers never create or repair a cache. Direct
inspection, replay, recovery and continuation still validate the selected history.

## Branch, recover and import

```bash
harness sessions clone --source RUN_ID --json
harness sessions fork --source RUN_ID --cutoff SEQUENCE --json
harness sessions tree --root RUN_ID --json
harness sessions crash-scan --json
harness sessions reopen --session RUN_ID --json
harness sessions continue RUN_ID
harness sessions discover --from FOREIGN_ROOT --json
harness sessions import --from FOREIGN_SESSION --json
```

A clone copies the latest completed, stable prefix. A fork copies an explicit
stable cutoff. Both create a new session beside the source and validate referenced
artifacts before publication. They leave the source unchanged. Tree output includes
parent IDs and depths; `--filter TEXT` selects matching rows.

Crash scanning reads journals and lock state. Reopen performs explicit recovery
when needed: it preserves complete records, retains torn bytes for diagnosis, and
marks interrupted work as having an unknown outcome. It does not retry that work.
Continue hands a resumable session to the terminal interface.

Discovery reports recognized foreign markers. Import currently accepts supported
`events.jsonl` histories and creates a separate replay-only session. Discovery of
another marker does not mean that format can be imported.

## Export

```bash
harness export RUN_ID --output conversation.md
harness sessions export RUN_ID --output support.json
```

The first command exports visible user and assistant text as Markdown. The second
exports a JSON support bundle containing the catalog, metadata, settled events,
counts, configuration and local readiness information. Provider deltas, reasoning
parts and raw tool result payloads are omitted from the support bundle. Local
readiness does not prove that provider authentication or execution will succeed.

Exports apply current configured, stored and environment credential redaction to
historical content. They scan keys and string values before writing. A remaining
secret stops the export and leaves an existing destination intact. File outputs
must be outside the session root and are replaced atomically with private
permissions. Omitting `--output` writes to stdout after the same checks.

## Archives

```bash
harness trace RUN_ID --output trace.tar.gz --json
harness wrap --output workspace.tar.gz
harness wrap --with-sessions --output workspace.tar.gz
```

Trace writes a local diagnostic archive with a sanitized `events.jsonl`, metadata
and a support summary. It omits reasoning, raw tool results and unclassified
artifacts, including provider payloads and attachment blobs. It is not a complete
session backup. The default output is `RUN_ID.tar.gz` in the working directory.

Wrap packages ordinary workspace files, honoring project ignore files. It skips
version-control directories, its own output and session storage. `--with-sessions`
adds the same sanitized diagnostic exports under their workspace-relative paths;
this requires session storage to be inside the workspace. Its default output is
`workspace.wrap.tar.gz`.

Source files and member names are scanned for known credentials. A finding,
symlink, unsupported file type or unsafe name stops packaging. Wrap preserves
source contents and ordinary permission bits. Files are limited to 16 MiB each;
both archive commands allow at most 10,000 members and 256 MiB before compression,
including session data. Each inspected journal is limited to 64 MiB.

Compression streams to a private temporary file beside the output. Publication
replaces the destination atomically after all checks pass. Outputs must stay
outside session storage; failures leave an existing archive intact. Source
histories are never changed.
