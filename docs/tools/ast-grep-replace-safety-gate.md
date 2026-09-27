# Structural search and replacement

`ast_grep_search` searches syntax with the installed `ast-grep` CLI.
`ast_grep_replace` uses its JSON replacements to preview or apply edits.
Both accept `pattern`, `language`, `path` or `paths`, `include`, `exclude`,
`limit`, and `context`. Replacement also requires `rewrite`; its `mode` defaults
to `dry_run`. Use `apply` to change files.

Language is inferred only when the selected readable files have one supported
language. Supported names are Rust, JavaScript, TypeScript, TSX, JSX, Python,
Markdown, JSON, TOML, and YAML, with their usual extension aliases. The installed
ast-grep build must provide the requested grammar.

Discovery respects ignore files and skips symlinks, hidden directories, build
outputs, and session storage. Additional files need an existing read allow rule;
files requiring separate approval are counted as skipped. Explicitly selected
files retain their initial read approval. All paths stay within the workspace.

The CLI reads private temporary copies of approved files, using one worker thread.
It never receives an update flag or the live workspace paths. Harness verifies
returned paths, byte ranges, UTF-8 boundaries, matched text, and non-overlapping
replacements against those copies. Temporary files are removed after the call. Previews retain a redacted `.diff` artifact for the existing TUI renderer.

Before apply, the coordinator checks the complete target set against edit policy.
New targets that need approval use the normal permission prompt, grant scopes,
timeout, and cancellation. A denied path prevents the entire plan from starting.
After approval, all files are checked again for concurrent changes. Each write
then uses the shared formatter, fingerprint, atomic file replacement, diff receipt,
and undo path. Writes are sequential; a later I/O failure reports completed files.

## Limits

| Resource | Limit |
| --- | --- |
| Pattern and replacement | 8 KiB each |
| Include or exclude globs | 64 each, 8 KiB per glob |
| Directory entries | 100,000 per search root |
| Source files | 200, 8 MiB per file, 32 MiB combined |
| Matches returned | Default 100; clamped to 1–200 |
| Context | Clamped to 0–5 lines |
| CLI execution | 30 seconds minus discovery time; 512 KiB per output stream |
| Combined preview diff | 1 MiB |

Apply rejects truncated matches and skipped files. Narrow the selection before
retrying. Large results use the coordinator's existing redacted artifact handling.

The native check covers search, ignore rules, unreadable files, previews,
replacement limits, denied and cancelled approvals, stale files, reusable grants,
edit receipts, and undo:

```bash
HARNESS_BINARY_SIGNOFF=1 cargo nextest run -p harness-tools --test binary_smoke \
  --ignore-default-filter -E 'test(ast_rewrites)'
```
