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

Session commands use `--session-dir DIR`, then a nonempty `runtime.session_dir`
from configuration, then `<data-dir>/sessions/<project-key>`. An empty
`runtime.session_dir` means the automatic managed default. Explicit relative
directories retain their existing meaning: they resolve against the project
directory selected by `--cwd`, not the config file or data directory. Absolute
directories are used as supplied. A session selector is its directory name or
an explicit directory path.

`<data-dir>` is a nonempty `HARNESS_HOME` used as-is, otherwise `$HOME/.harness`.
The project key encodes the canonical absolute project path,
so symlink aliases share a session bucket. See the exact
[storage layout](../architecture/sessions-and-replay.md#storage-layout).

Old project-local sessions are left in place. There is no automatic migration or
fallback scan; select the old directory explicitly:

```bash
harness sessions list --session-dir <old-session-dir> --json
harness sessions continue RUN_ID --session-dir <old-session-dir>
```

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

## Behavior census

```bash
harness sessions census
harness sessions census --json --since 2026-10-01 --model codex --limit 100
```

Census scans every session directory, including scenario fixtures and child
journals, using the same directory discovery as `sessions list`. It reads each
journal once and does not use or rebuild the catalog index. No provider, tool,
hook, or network work runs. Missing storage returns an empty report without
creating a directory. The catalog limits of 10,000 directories and 64 MiB per
journal also apply. Invalid, oversized, or identity-mismatched journals are
listed only in `unavailable_session_ids`; their contents and error text are not
printed, and their metrics are excluded.

`--since` accepts an RFC3339 timestamp or `YYYY-MM-DD` (midnight UTC). It selects
sessions with at least one recorded event at or after the cutoff, not individual
events within a session. Sessions without usable timestamps do not match.
`--model` matches model ids from provider requests, case-insensitively. Both
filters select whole sessions. `--limit` keeps the newest matching sessions by
recorded event time, breaking ties by run id. With no limit, all matching sessions
are included.

The default output has session, model, and total tables. JSON uses schema version
`harness-sessions-census-v1` with `session_count`, `sessions` (run id, model ids,
metrics), `models` (model id to metrics), `totals`, and
`unavailable_session_ids`. Model ids are exactly those recorded in provider
requests; the empty model key holds unattributed events. Output contains counts,
ids, model names, and tool names only. Prompts, arguments, reminder text, and tool
results are never printed.

Metrics are journal observations, not judgments about correctness:

- `turns` counts distinct provider/reminder/terminal turn ids. A turn using several
  models counts once in its session and once for each participating model. Event
  counts belong to the active request model; terminal metrics belong to the last
  model. `provider_fallbacks` counts provider or model switches following an error
  in the same turn, attributed to the replacement model.
- `tool_calls_by_tool` counts requested calls, including failed calls and nested
  eval host calls. `eval_calls` counts calls named `eval`; `direct_tool_calls`
  counts all other requested calls. `eval_share` is `eval_calls / tool_calls`,
  or zero with no calls. It is not the share of nested operations performed inside
  an eval cell.
- Repetition compares the stored tool id and argument digest in request order
  within each turn. Non-tool events do not interrupt a run; turn boundaries do.
  `longest_identical_tool_run` is a maximum, and `identical_tool_runs_ge_3` counts
  each run once, including runs longer than three.
- `open_todo_turns` folds `TodoProjection` through each agent-turn terminal event
  and counts pending or in-progress items. Interrupted turns without a terminal
  event do not count.
- `unverified_edit_turns` counts terminal turns with a successful `edit`, `write`,
  `apply_patch`, or `ast_grep_replace` after the latest successful `bash`, `eval`,
  or `lsp` check request. A check requested before an edit is not verification,
  even if it finishes later. Tool names are a heuristic; census does not inspect
  command text or prove that a check tested the changed file.
- `runtime_reminders_by_kind` counts durable runtime reminders. `compactions`
  counts committed session compactions and provider-native compactions.
  `provider_errors` counts provider requests finishing with `error`.
  `turn_failures_by_kind` classifies failed turn terminals reporting an iteration
  limit, loop guard, or stream guard. `subagent_spawns` counts agent spawn events
  with a parent.

Totals sum the selected journals. Parent and child journals can contain copies
of the same child activity; census does not deduplicate across journals.

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
The managed session default is outside the workspace. To use `--with-sessions`,
select an explicit in-workspace directory with `--session-dir`.

Source files and member names are scanned for known credentials. A finding,
symlink, unsupported file type or unsafe name stops packaging. Wrap preserves
source contents and ordinary permission bits. Files are limited to 16 MiB each;
both archive commands allow at most 10,000 members and 256 MiB before compression,
including session data. Each inspected journal is limited to 64 MiB.

Compression streams to a private temporary file beside the output. Publication
replaces the destination atomically after all checks pass. Outputs must stay
outside session storage; failures leave an existing archive intact. Source
histories are never changed.
