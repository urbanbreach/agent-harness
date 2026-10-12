# Session tools

Session tools inspect saved journals. They do not resume work, execute historical
tools, contact providers, load MCP servers, repair files, or write to the inspected
session. The current calling run still records the inspection tool call and its
redacted result.

The default root is the calling run's configured session directory. Set
`sessionRoot` to inspect another root. Explicit roots pass through `read` and
workspace-path checks; an outside root also needs `external_directory` approval.
Each operation checks its tool ID as a permission name. Session selectors and
path aliases share those checks.

## Operations

| Tool | Arguments |
| --- | --- |
| `session_list` | Optional `status`, `profile`, `resumable`, `filter`, `sort`, and `limit`. |
| `session_read` | Required `session`; optional event/message offsets and limits, `from_end`, and `include_todos`. |
| `session_search` | Required `query`; optional `session`, `case_sensitive`, `limit`, and `context_limit`. |
| `session_info` | Required `session`. Returns catalog metadata, event counts, lineage, journaled artifacts, and resume readiness. |

`session` accepts a run ID or a path to an immediate child of the selected root.
`run_id` is an alias. Read and info also accept `path`. Parent traversal and
symlinked private storage are rejected. Snake-case and camel-case names are
accepted for root, pagination, direction, and search options.

List status is `running`, `finished`, or `failed`. Sort defaults to
`updated_desc`; `updated_asc`, `run_id_asc`, `run_id_desc`, and `name` are also
accepted. `run_id` is an alias for `run_id_asc`. List text filters match the
redacted catalog JSON without case sensitivity; `profile` matches
`profile_preset`. Read offsets start at zero. `from_end` returns the newest
entries first. History windows and searches follow recorded rewinds.

Search is a literal substring search over redacted user and assistant text,
session titles, run summaries/errors, tool output summaries, and task
completion/cancellation text. It returns one row per matching event, with an
excerpt around the first match, not one row per occurrence. It excludes
reasoning, provider fragments, and raw tool arguments. Search is case-insensitive
unless requested otherwise. The configured literal-secret redactor applies
before matching, so result counts cannot reveal those secret values.

## Limits and errors

List and search return 50 rows by default; read returns 25 events and 25 messages.
All row limits clamp to 1 through 200. Search queries must contain nonblank text
and fit within 4,096 bytes. Search context defaults to 80 characters on each
side of the match and clamps to 1 through 500. Read summaries shorten individual
text fields after 8 KiB. Info returns at most 200 artifact paths. Responses
report counts and truncation; larger responses use the coordinator's normal
artifact handling.

A scan reads at most 2,000 session directories. Inspection rejects journals larger
than 64 MiB and checks cancellation between records. It loads one session at a
time. There is no persistent search index.

An explicit unreadable or malformed session fails the call. Multi-session list
and search responses collect such failures in `errors`, rather than counting them
as healthy sessions. Inspection never repairs the source journal.

Todo inclusion reads successful journaled `todowrite` results. Legacy standalone
todo files are not read. Child sessions have independent journals and appear in
the same catalog as their parent.
