# Chat rendering parity log

This document tracks the active Opencode-to-Harness transcript rendering comparison.
It is intentionally evidence-first: each finding names the source lines that define
the current behavior and the Harness line(s) that must match or intentionally differ.

## Current scenarios

1. Happy path: a deterministic transcript with user text, assistant markdown,
   reasoning, and tool rows renders with Opencode spacing and entry semantics.
2. Edge: long markdown/code/tool text wraps without terminal overflow at narrow
   widths.
3. Adjacent regression: task/subagent activity renders as Opencode task rows, with
   subagent details routed through the subagent surface instead of extra transcript
   hint rows.

## Findings

### Opencode entry semantics

- Opencode's immutable scrollback maps committed user entries to plain text with
  a leading `› ` prompt marker in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:51`,
  colors that text with `entry.user.body = scrollbackTheme.primary` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/theme.ts:545`, and
  writes it through `RunEntryContent` as a full-width wrapped text renderable in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.writer.tsx:151`.
- Opencode's active sent-message/prompt surface is a separate footer surface: it
  uses `border={["left"]}`, `borderColor={theme().highlight}`, and a custom
  left border glyph `vertical: "█"` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.view.tsx:642`;
  the inner prompt container uses `paddingLeft={0}`, `paddingRight={0}`,
  `paddingTop={0}`, `gap={0}`, and `backgroundColor={theme().surface}` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.view.tsx:656`;
  the prompt body itself adds `paddingTop={1}`, `paddingBottom={1}`, and
  `paddingRight={2}` around the textarea in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.prompt.tsx:252`.
- Opencode maps reasoning to markdown code content and rewrites leading
  `Thinking:` to `_Thinking:_` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:61`.
- Opencode removes `[REDACTED]` from reasoning entries and suppresses the entry
  when the cleaned body is empty in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:61`.
- Opencode suppresses assistant start/final text and renders in-flight assistant
  text as markdown in `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:180`.
- Opencode keeps inline tool rows adjacent without a blank separator; block rows
  get one separator row in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.writer.tsx:81`.
- Opencode classifies tool text rows as `inline`, while assistant text is `block`;
  `separatorRows` only suppresses spacing for same entry groups or inline→inline
  transitions, so assistant block text followed by an inline tool gets one blank
  separator row in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.writer.tsx:52`.

### Opencode task/subagent behavior

- Opencode task inline rows use status icons `✗`, `•`, and `✓`, with title equal
  to the task description when present and subtitle `${Kind} Agent` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:366`.
- Opencode final task snapshots are structured blocks titled `# ${Kind} Task`
  with the task description as the body row in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:571`.
- Opencode completed `task` tool output extracts `<task_result>...</task_result>`
  or drops `task_id:` metadata lines, then renders the remaining result through
  markdown in `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:759`
  and `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:1436`.
- Opencode subagent inspection lives in the footer panel, not as a transcript hint:
  `RunFooterSubagentBody` renders the selected subagent title/status and commits
  under a scrollbox in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:121`.
- Opencode subagent inspection keeps the selected child activity scrolled to the
  newest rows by setting `stickyScroll={true}` and `stickyStart="bottom"` on the
  footer scrollbox in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:150`.
- Opencode reserves 14 rows for the subagent inspector in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:11`
  and uses status icons `●`, `○`, `◍`, and `◔` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:29`.

### Opencode todo behavior

- Opencode todo-write inline/block rows are titled `Todos`, use icon `#`, and
  render todo item markers as `[✓]` only for `completed`, `[•]` for
  `in_progress`, and `[ ]` for every other status in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:377`.
- Opencode structured scrollback renders cancelled items as struck `[ ]` rows in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.writer.tsx:10`.

### Opencode skill behavior

- Opencode skill rows use icon `→` and title `Skill "${name}"` with no output
  body in `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:396`.

### Opencode LSP behavior

- Opencode LSP rows use icon `→` and title `LSP ${operation}` or
  `LSP ${operation} ${filePath}:${line}:${character}` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:445`.

### Opencode shell and file-search behavior

- Opencode bash rows render through the run-mode `$` block helper: the row title
  is the command itself and the completed output is printed as block body in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:1272` and
  `inspirations/opencode/packages/opencode/src/cli/cmd/run.ts:78`, with no
  Harness-only rail/card chrome.
- Opencode grep rows use icon `✱`, title `Grep "${pattern}"`, and optional
  description `in ${path} · N matches` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:301`.
- Opencode glob rows use icon `✱`, title `Glob "${pattern}"`, and optional
  description `in ${path} · N matches` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:286`.
- Opencode list rows use icon `→` and title `List ${path}` or `List`, without a
  count/path subtitle, in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:314`.

### Opencode batch/write/edit/patch behavior

- Opencode batch rows use icon `#`, block mode, and title `Batch N tool(s)` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:435`.
- Opencode write and edit rows use icon `←`, block mode, and titles
  `Write ${filePath}` / `Edit ${filePath}` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:332` and
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:349`.
- Opencode apply-patch rows use icon `%` and title `Patch`, `Patch 1 file`, or
  `Patch N files` in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:403`.

### Opencode markdown table behavior

- Opencode streams assistant markdown with `MarkdownRenderable` using
  `internalBlockMode: "top-level"` and `tableOptions: { widthMode: "content" }`
  in `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.surface.ts:175`.
- Opencode commits stable markdown by top-level block rather than by raw line in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.surface.ts:287`,
  which keeps table blocks visually distinct from surrounding paragraph text.

### Opencode fenced code behavior

- Opencode renders standalone reasoning/code bodies through `CodeRenderable` with
  `width: "100%"`, `wrapMode: "word"`, `drawUnstyledText: false`, streaming
  enabled, and theme-derived syntax style in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.surface.ts:164`.
- Opencode renders markdown-contained fenced code through `MarkdownRenderable`
  with the same theme syntax style and top-level block streaming in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/scrollback.surface.ts:175`.

### Harness parity behavior implemented

- Harness now applies Opencode user-entry and reasoning-entry body semantics in
  `crates/harness-tui/src/ui_transcript_render.rs:56` and
  `crates/harness-tui/src/ui_transcript_render.rs:463`: reasoning strips
  `[REDACTED]` plus rewrites a leading `Thinking:` marker to `_Thinking:_` before
  markdown-ish rendering, matching
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:51`
  and `inspirations/opencode/packages/opencode/src/cli/cmd/run/entry.body.ts:61`.
- Harness top-level user rows follow the known-good GitHub `origin/dev`
  Opencode-style transcript block rather than the downgraded active-prompt
  block styling: `build_user_render_surface` uses the emphasized transcript
  surface, `show_outer_rail: true`, the profile accent `┃` rail, one surface
  padding row above and below, two-column left body padding, and the same
  two-column trailing gap as `origin/dev` in
  `crates/harness-tui/src/ui_transcript_render.rs`. Selection rows preserve that
  per-surface rail through `crates/harness-tui/src/ui_transcript_types.rs`,
  `crates/harness-tui/src/ui_transcript_layout.rs`, and
  `crates/harness-tui/src/ui_transcript_surface.rs`.
- Harness now suppresses redacted-only reasoning before transcript section
  construction in `crates/harness-tui/src/ui_transcript_events.rs:4`, preventing
  bare `Thinking:` rows when Opencode would return `RUN_ENTRY_NONE`.
- Harness now renders completed task results as markdown detail blocks in
  `crates/harness-tui/src/ui_transcript_tool_sections.rs:857` and extracts
  `<task_result>` or strips `task_id:` metadata in
  `crates/harness-tui/src/ui_transcript_tool_sections.rs:898`, matching
  Opencode `taskResult` plus final `markdownBody` behavior.
- Harness now uses the task description as the inline task row title and
  `${Kind} Agent` as the subtitle in
  `crates/harness-tui/src/ui_tool_delegation.rs` and
  `crates/harness-tui/src/ui_transcript_tool_sections.rs`.
- Harness no longer inserts a separate transcript `view subagents` hint row;
  subagent inspection remains routed through task-row navigation and the
  subagent/footer surface.
- Harness task rows no longer add transcript-visible child toolcount,
  duration, or background continuation hint rows; background checks remain
  available through the native background tools instead of the Opencode-shaped
  transcript task row.
- Harness task rows render the subtitle through `TaskInline` in
  `crates/harness-tui/src/ui_transcript_tool_render.rs`, keeping the Opencode
  inline title/subtitle split in the transcript surface.
- Harness todo rows now use the fixed `Todos` block title with `#` icon and render cancelled
  items as `[ ]` with strikethrough styling in
  `crates/harness-tui/src/ui_tool_question_todo.rs`, matching the Opencode
  todo-write marker/title contract and structured cancelled-row styling.
- Harness now lets `transcript_surface_leading_gap` insert a one-row separator
  between assistant body text and following tool rows in
  `crates/harness-tui/src/ui_transcript_surface.rs`, matching Opencode's
  block→inline separator rule.
- Harness now renders markdown table blocks through a focused content-width,
  borderless column path in `crates/harness-tui/src/ui_markdown_table.rs`, routed
  from `crates/harness-tui/src/ui_markdown.rs:278`, matching Opencode's
  top-level MarkdownRenderable table settings instead of showing raw pipe rows.
- Harness table cells now conceal inline link/code/emphasis markup while keeping
  CJK display width padding in `crates/harness-tui/src/ui_markdown_table.rs:138`,
  so richer table rows remain content-width and selection-safe.
- Harness fenced code blocks already route through
  `crates/harness-tui/src/ui_syntax_highlight.rs` from
  `crates/harness-tui/src/ui_markdown.rs:248`, preserving syntax-highlighted
  code rows without nested frame rails or raw fence markers.
- Harness replay mode now suppresses the operator rail overlay in narrow layouts
  and only uses the rail when there is enough width for a real split in
  `crates/harness-tui/src/layout.rs:478`, preventing sidebar labels from
  overwriting wrapped transcript text at 60-column widths.
- Harness no longer renders compatibility-alias disclosure rows such as
  `Compat alias · read → fs.read` in transcript tool details. Opencode read and
  webfetch rows render only the operator-facing tool row in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:322` and
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:341`; Harness
  now keeps alias resolution internal in
  `crates/harness-tui/src/ui_transcript_tool_sections.rs` instead of appending a
  non-Opencode metadata line.
- Harness no longer renders the read-specific `↳ Loaded ...` detail row after a
  successful read. Opencode `runRead` returns only the inline `Read ${file}` row
  and optional input description in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:322`, so
  Harness keeps read output details out of the transcript row unless Opencode has
  a matching visible field.
- Harness no longer adds transcript-local `Click to expand` / `Click to collapse`
  preview rows or ellipsis truncation for generic and bash tool bodies. Opencode
  run-mode block rendering prints the `ToolInline.body` it receives in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run.ts:78`, and bash sets
  that body from completed output in
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/tool.ts:1272`; runtime
  truncation remains a tool-output concern rather than a transcript collapse
  affordance.
- Harness bash rows now render as an Opencode-style `$ command` block with only
  the optional `# Running in <workdir>` preface in
  `crates/harness-tui/src/ui_transcript_bash.rs`, and
  `crates/harness-tui/src/ui_transcript_tool_render.rs` routes shell tools
  through that block instead of the old Harness rail/card surface.
- Harness grep and glob rows now put `Grep "pattern"` / `Glob "pattern"` in the
  title and `in <path> · N matches` in the subtitle, while list rows render only
  `List <path>` or `List`, in
  `crates/harness-tui/src/ui_transcript_tool_sections.rs` and
  `crates/harness-tui/src/ui_tool_paths.rs`.
- Harness skill rows now render `→ Skill "name"` without the previous
  Harness-only `Load skill ...` wording or `✦` icon in
  `crates/harness-tui/src/ui_tool_titles_harness.rs` and
  `crates/harness-tui/src/ui_transcript_tool_sections.rs`, matching Opencode
  `runSkill`.
- Harness LSP rows now render `→ LSP operation file:line:character` without the
  previous `⌘` icon, metadata-style operation suffix, or visible generic
  `[operation=...]` row in `crates/harness-tui/src/ui_tool_titles_harness.rs`
  and `crates/harness-tui/src/ui_transcript_tool_sections.rs`, matching
  Opencode `runLsp`.
- Harness question rows now render `→ Asked N question(s)` without the previous
  question/answer transcript body in `crates/harness-tui/src/ui_tool_question_todo.rs`
  and `crates/harness-tui/src/ui_transcript_tool_sections.rs`, matching
  Opencode `runQuestion`.
- Harness batch, write, edit, and apply-patch rows now use Opencode titles and
  icons in `crates/harness-tui/src/ui_tool_titles.rs`,
  `crates/harness-tui/src/ui_tool_diffs.rs`, and
  `crates/harness-tui/src/ui_transcript_tool_sections.rs`: `# Batch N tools`,
  `← Write file`, `← Edit file`, and `% Patch N files`, without Harness-only
  `Run batch`, `Preparing edit...`, `Preparing patch...`, or path/count subtitle
  disclosure on the row header.
- Harness subagent replay now reserves the Opencode 14-row footer inspector in
  `crates/harness-tui/src/layout.rs:53` and renders the selected subagent status,
  title, optional label, count, and activity body from
  `crates/harness-tui/src/ui_subagent_footer.rs:31`, with Opencode-style
  `paddingLeft={1}`, `paddingRight={3}`, and no old Harness left `┃` rail or
  `Parent`/`Prev`/`Next` footer navigation row.
- Harness subagent footer status now maps child orchestration states to the
  Opencode status set in `crates/harness-tui/src/ui_subagent_footer.rs`: running
  rows use spinner frames, completed rows use `●`, cancelled rows use `○`, and
  failed/timed-out rows use `◍`, matching
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:29`.
- Harness subagent footer activity now uses
  `crates/harness-tui/src/ui_transcript.rs:612` to render only Opencode-style
  child entry body parts: child user commits with the `› ` prompt marker,
  assistant markdown, reasoning, native tool rows, and post-tool assistant text,
  while omitting parent user prompt chrome and Harness assistant footer summary
  rows. This matches Opencode
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/footer.subagent.tsx:84`
  calling `RunEntryContent` and `entry.body.ts:166` suppressing summary commits.
- Harness subagent footer overflow now renders the latest visible child rows by
  slicing the normal transcript lines from the bottom in
  `crates/harness-tui/src/ui_subagent_footer.rs:211`, matching Opencode's
  sticky-bottom subagent inspector.

## Evidence

- `transcript_user_and_reasoning_match_reference_entry_body` locks the top-level
  user sent-message block to the known-good `origin/dev` thin `┃` rail with
  two-column body padding while preserving Opencode's no-duplicate
  `Thinking: Thinking:` reasoning behavior.
- `redacted_only_reasoning_matches_reference_empty_body` locks Opencode's
  redacted-only reasoning suppression.
- `transcript_task_rows_match_reference_inline_title_and_no_hint` locks the
  description-title plus `${Kind} Agent` subtitle behavior.
- `transcript_task_rows_show_child_status_duration_and_counts` locks completed
  task rows to the Opencode inline title/subtitle shape without child
  toolcount, duration, or background continuation hint rows.
- `task_row_renders_task_result_markdown_without_wrappers` locks completed task
  result extraction: the `<task_result>` body is visible as markdown detail while
  `task_id:`, `request_id:`, and XML wrappers stay hidden.
- `tool_lifecycle_rows_stay_ordered_without_pty` locks the deterministic TUI
  snapshot shape with `✓ audit tool lifecycle parity · Researcher Agent` and no
  transcript-level `view subagents` hint.
- `subagent_footer_body_renders_child_user_commit_without_assistant_summary`
  locks the selected child footer body to Opencode `RunEntryContent` semantics:
  the child user commit renders as `› ...`, child assistant content is visible,
  and the Harness `Assistant · model` summary footer is absent from the selected
  footer body.
- `todo_write_rows_render_open_checklist` locks the fixed `# Todos` title,
  completed/in-progress/pending markers, and cancelled `[ ]` marker ordering.
- `latest_assistant_footer_stays_after_trailing_tool_rows` locks the blank
  separator row between assistant prose and the following inline tool row.
- `markdown_tables_match_reference_top_level_columns` and
  `assistant_markdown_tables_match_reference_top_level_columns` lock the markdown
  table behavior: `Name   Age`, `Alice  30`, no raw `| Name | Age |`, and no
  grid border glyphs.
- `markdown_table_selection_matches_rendered_rows` locks transcript selection
  parity for markdown tables: selectable rows mirror the rendered `Name   Age`,
  `Alice  30`, and `Bob    5` rows and do not expose the raw markdown separator
  row.
- `markdown_tables_render_inline_links_code_alignment_and_cjk_width` and
  `markdown_table_rich_selection_matches_rendered_rows` lock richer table parity:
  alignment separator rows stay hidden, link hrefs are concealed, inline code and
  emphasis render as cell text, and CJK cells preserve display-width padding in
  both rendered and selected rows.
- `fenced_code_highlighting_uses_syntect_styles_for_known_languages` and
  `fenced_code_highlighting_falls_back_to_plain_text_when_unknown` lock
  known-language syntax coloring, plain fallback for unknown languages, same
  background as surrounding prose, and absence of nested frame rails.
- `shell_tool_cards_use_reference_dollar_block_without_harness_chrome` locks the
  bash block to `# Running in /workspace/crates/harness-tui`, `$ cargo test`, and
  command output rows while rejecting old `┃`, `# list files`, and box-card
  chrome.
- `file_search_rows_match_reference_title_description_shape` locks grep, glob,
  and list rows to the Opencode title/subtitle shape: `✱ Grep "fn render"` with
  `in crates/harness-tui/src · 3 matches`, `✱ Glob "**/*.rs"` with
  `in crates/harness-tui · 5 matches`, and `→ List crates/harness-tui/src`
  without a count subtitle.
- `native_tool_transcript_rows_show_reference_timestamps_and_task_metadata`
  locks current Opencode web rows from `run/tool.ts`: `% WebFetch <url>`,
  `◈ <provider> Web Search "query" (N results)` using provider metadata like
  `Parallel Web Search`, and `◇ Exa Code Search "query" (N results)`.
- Fresh body→tool spacing evidence lives under
  `/tmp/opencode/harness-tui-spacing-parity-qa/`, with the direct rendered
  separator capture in `body-tool-spacing.txt`.
- Fresh markdown table parity evidence lives under
  `/tmp/opencode/harness-tui-markdown-table-parity-qa/`, with direct rendered
  table rows in `rendered-table.txt` and width evidence in
  `tui-check-test-output.json`. The replay-driven terminal capture in
  `live-table-tui-capture.txt` exercises the real `harness tui --replay`
  surface and shows aligned `Name   Age`, `Alice  30`, and `Bob    5` rows
  with raw pipe table rows and grid borders absent.
- Fresh todo parity test and TUI-width evidence lives under
  `/tmp/opencode/harness-tui-todo-parity-qa/`, with the direct rendered todo
  rows captured in `rendered-todo.txt`.
- Fresh tmux capture evidence lives under
  `/tmp/opencode/harness-tui-task-row-qa/` for the 180-column deterministic TUI
  surface.
- Fresh fenced-code replay evidence lives under
  `/tmp/opencode/harness-tui-code-block-parity-qa/`: `live-code-tui-capture.txt`
  exercises `harness tui --replay` and shows `fn main() {`, `let answer = 42;`,
  and `println!("{answer}");` with raw fences and nested frame rails absent;
  `live-code-tui-check.json` reports max width `66/80` with no overflow lines.
- Fresh narrow wrapping/cutoff evidence lives under
  `/tmp/opencode/harness-tui-wrapping-parity-qa/`: the pre-fix captures showed
  replay operator labels overwriting wrapped transcript text at `60x40`; the
  fixed capture `live-wrap-fixed-tui-capture.txt` shows wrapped ordinary words,
  split long tokens, CJK text, and `TAILTOKEN` preserved with the operator rail
  absent, and `live-wrap-fixed-tui-check.json` reports max width `58/60` with no
  overflow lines.
- Fresh `harness.jsonc` dogfood evidence also lives under
  `/tmp/opencode/harness-tui-wrapping-parity-qa/`: `harness-jsonc-dogfood-capture.txt`
  shows the live startup TUI using the configured Build lane with GLM 5.2,
  `harness-jsonc-dogfood-command-palette.txt` exercises `Ctrl+p`, and
  `harness-jsonc-dogfood-command-palette-bad-input.txt` exercises an unmatched
  `zzz` query with `No results found`; the matching `*-check.json` files report
  max width `100/100` with no overflow lines.
- Fresh final parity replay evidence lives under
  `/tmp/opencode/harness-tui-final-parity-qa/`: `replay-session/events.jsonl`
  contains a redacted-only reasoning delta, a rich CJK/inline markdown table, and
  a completed task output with `<task_result>` wrapper metadata;
  `live-final-tui-capture.txt` exercises the real `harness tui --replay` surface
  and shows `모델  Docs  spawn()  완료`, `A     Bold  x        대기`, `Result`,
  `Task markdown is visible`, and `Wrapper metadata is hidden` while omitting
  `Thinking:`, `task_id:`, `request_id:`, link hrefs, and task-result XML
  wrappers. `live-final-tui-check.json` reports max width `97/100` with no
  overflow lines.
- Fresh continuation dogfood evidence also lives under
  `/tmp/opencode/harness-tui-final-parity-qa/`: `continue-dogfood-capture.txt`
  reruns the real `harness --config harness.jsonc tui --replay ...` surface at
  `80x24` and shows the rendered CJK markdown table plus completed task result
  markdown while omitting `Thinking:`, task metadata lines, link hrefs, and XML
  wrappers. `continue-dogfood-check.json` reports max width `78/80`, no
  overflow lines, `borderMisaligned: false`, and the expected CJK wide columns.
- Fresh live `harness.jsonc` startup evidence is captured in
  `continue-config-dogfood-capture.txt`: the TUI launches with the configured
  Umans provider surface and shows `Build GLM 5.2 Umans AI Coding Plan · high`;
  `continue-config-dogfood-check.json` reports max width `78/80` and no
  overflow lines. The plain-text border heuristic reports `borderMisaligned`
  on the startup logo/rail mix, but the capture has no overflow and no transcript
  rendering overlap.
- Fresh live model-switcher dogfood evidence is captured in
  `continue-model-dogfood-capture.txt` and
  `continue-model-kimi-dogfood-capture.txt`: `/model` opens the selector with
  the active `GLM 5.2` row under `Umans AI Coding Plan`, and filtering `kimi`
  shows `Kimi K2.7 Code` under the same provider. The matching `*-check.json`
  reports max width `78/80` with no overflow lines.
- Fresh output-collapse replay evidence lives under
  `/tmp/opencode/harness-tui-output-collapse-qa/`: `replay-capture.txt`
  exercises the real `harness tui --replay` surface and shows `line 10`,
  `line 11`, and `line 12` for a long bash output while omitting `Click to
  expand`, `Click to collapse`, and ellipsis cutoff rows.
- Fresh skill-row replay evidence lives under
  `/tmp/opencode/harness-tui-skill-row-qa/`: `replay-capture.txt` exercises the
  real `harness tui --replay` surface and shows `→ Skill "rust-best-practices"`
  while omitting the old `Load skill` wording, `✦` icon, and `skill loaded`
  output body.
- Fresh LSP-row replay evidence lives under
  `/tmp/opencode/harness-tui-lsp-row-qa/`: `replay-capture.txt` exercises the
  real `harness tui --replay` surface and shows
  `→ LSP goto_definition src/main.rs:12:4` while omitting the old `⌘` icon,
  `[operation=goto_definition]` generic metadata suffix, and output body.
- Fresh question-row replay evidence lives under
  `/tmp/opencode/harness-tui-question-row-qa/`: `replay-capture.txt` exercises
  the real `harness tui --replay` surface and shows `→ Asked 2 questions` while
  omitting the question prompt text, answer labels, and `User has answered your
  questions.` output body.
- Fresh batch/write/edit/patch replay evidence lives under
  `/tmp/opencode/harness-tui-batch-write-patch-qa/`: `replay-capture.txt`
  exercises the real `harness tui --replay` surface and shows
  `# Batch 2 tools`, `← Write src/generated.rs`, `← Edit src/generated.rs`, and
  `% Patch 2 files` while omitting the old `Run batch`, `≋`,
  `Preparing edit...`, `Preparing patch...`, `Patch · 2 files`, and row-header
  disclosure glyphs for write/edit rows.
- Fresh subagent-footer replay evidence lives under
  `/tmp/opencode/harness-tui-subagent-footer-qa/`: the current capture
  `final-manual-replay-capture-current.txt` exercises the real
  `target/debug/harness --config harness.jsonc tui --replay .../agent_alpha`
  surface at `80x24` and shows the selected Opencode-style padded footer body
  under `● agent_alpha  Researcher` containing `› Inspect footer parity` and
  `Footer inspector activity is visible.` with no left `┃` rail and no
  `Assistant · model-1` summary inside the selected footer body. The matching
  `final-manual-replay-tui-check-current.json` reports max width `44/80`, no
  overflow lines, and `borderMisaligned: false`.
- `child-replay-capture-current.txt` in
  `/tmp/opencode/harness-tui-subagent-sticky-qa/` exercises the current
  child-session replay surface through
  `harness --config harness.jsonc tui --replay .../agent_alpha` at `80x24` and
  shows the selected footer body under `● agent_alpha  Researcher` containing
  child `RunEntryContent` rows. The normal transcript above the footer may still
  show `Assistant · model-1`, but the selected footer body omits that summary
  row.
- `subagent_footer_body_keeps_ordered_transcript_tool_rows` locks the selected
  child footer body to the same ordered transcript renderer used by the main
  chat surface: assistant markdown appears before `Read docs/rust.md`, and
  post-tool assistant body appears after that tool row.
- `subagent_footer_status_uses_running_and_cancelled_icons` locks the Opencode
  footer status set for active and cancelled child orchestration rows: the first
  spinner frame `⠋` for running and `○` for cancelled.
- `subagent_footer_body_sticks_to_latest_activity` locks Opencode's
  sticky-bottom subagent inspector behavior for overflowing child transcript
  activity: newest child rows stay visible and oldest rows scroll out.
- `subagent_footer_body_preserves_child_text_matching_parent_label` locks that
  selected child footer content is not rewritten: a child output string that
  happens to contain the parent label remains intact, matching Opencode's lack
  of an equivalent content rewrite.
- Fresh sticky-bottom subagent replay evidence lives under
  `/tmp/opencode/harness-tui-subagent-sticky-qa/`:
  `child-replay-capture-current.txt` exercises the real
  `harness --config harness.jsonc tui --replay .../agent_alpha` surface and
  shows footer rows `child activity line 14` through `child activity line 24`
  while omitting `child activity line 01` from the footer;
  `child-replay-tui-check-current.json` reports max width `76/80` with no
  overflow lines and `borderMisaligned: false`.
- Fresh subagent-footer tool-row replay evidence lives under
  `/tmp/opencode/harness-tui-subagent-toolrow-qa/`:
  `toolrow-capture-current.txt` exercises the real
  `harness --config harness.jsonc tui --replay .../agent_alpha` surface and
  shows the selected footer body containing the child user commit
  `› Inspect footer tool rows`, child assistant text `Before tool row.`, the
  shared native tool row `→ Read docs/rust.md`, and post-tool assistant text
  `After tool row.` while omitting `Assistant · model-1` from the selected
  footer body. `toolrow-tui-check-current.json` reports max width `76/80` with
  no overflow lines and `borderMisaligned: false`.
- Redacted `harness.jsonc` dogfood checklist evidence is tracked at
  `/tmp/opencode/harness-tui-subagent-footer-qa/redacted-dogfood-checklist.md`:
  it records the exhaustive chat-rendering parity matrix for every
  `docs/native-tool-catalog.md` tool id, the exact Opencode-vs-Harness
  same-transcript row diff, config validation, and log/artifact inspection with
  credential strings redacted and no API key value copied into docs. Supporting
  artifacts in the same directory are `same-transcript-opencode-harness-diff.txt`
  and `native-tool-chat-rendering-matrix.md`.

## Open blockers after Oracle verification

- No known transcript/tool-row blocker remains in the parity slices documented
  above. Final completion still depends on skeptical verification against the
  original broad Opencode-to-Harness chat-rendering parity goal.
