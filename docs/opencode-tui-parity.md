# Harness / OpenCode TUI parity specification

**Status:** design-language reference for the first public release. Produced as
PRD §8 P1-1.

**Scope:** For every TUI surface that OpenCode exposes, this doc records what
OpenCode does, which Harness seam implements it, and the current parity status.
It also records the scope decisions from
[`docs/opencode-visual-tool-parity-prd.md`](opencode-visual-tool-parity-prd.md)
§3 that justify intentional Harness deviations.

**How to read the statuses**

- **match** — surface already aligns with the OpenCode target at the
  composition/glyph/color-role level.
- **gap** — surface is recognizable but differs in layout, color role, or
  information hierarchy.
- **divergence** — Harness deliberately behaves differently because of an
  architecture invariant or a settled scope decision.

---

## Table of surfaces

> **Visual parity work reverted on 2026-06-16.** The harness-tui source and
> snapshots were rolled back to commit `3ffaf9a5` after the applied visual changes
> degraded the TUI compared with both its pre-PRD state and the OpenCode
> reference. Statuses below describe the restored pre-PRD tree; redo work must
> consult `inspirations/opencode/` directly while preserving Harness's visual
> identity.

| Surface | OpenCode reference | Harness seam | Status |
|---|---|---|---|---|
| Start Screen | `routes/home.tsx`, `component/logo.tsx`, `routes/session/footer.tsx` | `ui_lifecycle.rs::render_startup_lifecycle_flow`, `ui_composer.rs::render_document_composer_content`, `ui_chrome.rs::render_footer` | gap (reverted) |
| Session Transcript | `routes/session/index.tsx` | `ui_transcript_render.rs::build_transcript_render_surfaces`, `ui_markdown.rs::append_rich_text_block` | gap (reverted) |
| Footer | `routes/session/footer.tsx` | `ui_chrome.rs::render_footer`, `view_model.rs` footer hints | gap (reverted) |
| Sidebar | `routes/session/sidebar.tsx` | `ui_secondary.rs::render_operator_sidebar`, `ui_secondary/sidebar_data.rs`, `ui_secondary/sidebar_sections.rs` | gap (reverted) |
| Command Palette | `component/command-palette.tsx`, `ui/dialog-select.tsx` | `ui_overlays.rs::render_command_palette_overlay`, `ui_chrome::command_palette_*` tokens | gap (reverted) |
| Tool-Call Rows | `routes/session/index.tsx` (`ToolPart`, `InlineTool`, per-tool components) | `ui_tool_titles.rs`, `ui_transcript_tool_render.rs::append_tool_call_section_lines` | gap (reverted) |
| Inline Diff | `routes/session/index.tsx` (`Edit`, `ApplyPatch`, `<diff>`), `feature-plugins/system/diff-viewer.tsx` | `ui_tool_diffs.rs`, `ui_transcript_tool_render.rs::append_tool_call_diff_block`, `ui_diff.rs` | gap (reverted) |
| Thinking / Todo Blocks | `routes/session/index.tsx` (`ReasoningPart`, `TodoWrite`) | `ui_transcript_render.rs::append_reasoning_block`, `ui_transcript_tool_render.rs::append_tool_call_todo_list` | partial / gap (reverted) |


---

## Scope decisions that affect parity

These are taken verbatim from the PRD §3. They are not re-litigated here; they
explain why certain OpenCode regions are intentionally absent or altered in
Harness.

1. **No standalone `write` tool.** File creation is performed by the hashline
   edit path (anchorless append). This is recorded in
   `crates/harness-tools/src/hashline_edit.rs` and in
   `docs/native-tool-catalog.md`.
2. **Category routes and extra native tools stay.** `task(category=...)`
   profiles and tools such as `ast-grep`, `session_*`, `background_*`,
   `batch`, `codesearch`, `github.*`, and `lsp.rename` remain first-class.
   They are native Rust ports, not parity blockers, so they are not trimmed.
3. **Per-model-family prompts stay.** The runtime prompt-family resolver is
   treated as genuine OpenCode parity, not orchestration overreach.
4. **Cloud / share / account / plugin surfaces are out.** OpenCode ships
   `/connect`, share links, plugin runtimes, and snapshot-based undo UI that
   Harness does not adopt. Their screen space is either left empty or reused
   for Harness equivalents.
5. **Events are the source of truth; replay is side-effect free.** The TUI
   never appends events, resolves permissions locally, or executes tools. Any
   OpenCode flow that mutates local state directly is replaced by a TUI intent
   sent to the coordinator.
6. **Runtime config (`harness.json{,c}`) and TUI config (`tui.json{,c}`) are
   separate contracts.** New theme/visual settings belong to `tui.json{,c}`.
7. **Custom markdown slash commands (`$ARGUMENTS`) are out of v1.**

---

## Per-surface parity

### 1. Start Screen

**OpenCode behavior**

`routes/home.tsx` centers a two-tone block wordmark (`<Logo />`), then a single
prompt box. The prompt is implemented in `component/prompt/index.tsx` and is
slim: it accepts placeholder rotation, optional `right` slot content, and stores
its own history/stash. Model and agent metadata sit inside the prompt chrome
(lines below the textarea). A plugin slot `home_bottom` supplies the hint
cluster (`● N mcp servers`, `tab agents`, `ctrl+p commands`, `● Tip …`). The
bottom of the screen hosts the shared `Footer` from
`routes/session/footer.tsx`, which shows the current directory on the left and
LSP/MCP/status dots on the right.

Adjacent files surveyed: `routes/home/session-destination.tsx` (session creation
flow), `component/logo.tsx` (wordmark asset), `context/tui-config.tsx`,
`context/prompt.tsx`.

**Harness seam**

- Logo and centered layout: `crates/harness-tui/src/ui_lifecycle.rs`
  `render_startup_lifecycle_flow` and `startup_logo_lines`.
- Prompt input: `crates/harness-tui/src/ui_composer.rs`
  `render_document_composer_content`, invoked through the control dock path in
  `ui_chrome.rs::render_unified_bottom_dock`.
- Footer bar: `crates/harness-tui/src/ui_chrome.rs::render_footer`; startup
  path uses `startup_directory_branch_label` and a fixed hint set.

**Parity status: gap**

Current differences:

- Harness renders a thin, spaced `Harness` wordmark built from box-drawing
  glyphs instead of OpenCode's two-tone block logo.
- Model/agent metadata is duplicated above the input box and inside the composer
  metadata line. OpenCode shows it once, inside the prompt box.
- The bottom status bar exists in Harness only as a replay/read-only footer;
  the startup screen does not yet render an OpenCode-style status row with
  `~/cwd  ⊙ N MCP  /status … version`.
- There is no hint cluster or rotating `Tip` line below the prompt.

**Adaptations / blockers**

- Footer content is governed by the runtime/TUI config split: status dots
  should read from `tui.json{,c}` visibility flags where they differ from the
  runtime `harness.json{,c}`.
- Cloud/account/plugin regions from the OpenCode footer are excluded by the
  §4 non-goal list.

**Checklist**

- [ ] Two-tone block wordmark.
- [ ] Single in-box metadata line (agent · model).
- [ ] Bottom status bar with cwd, LSP/MCP dots, `/status`.
- [ ] Hint cluster (`● N mcp`, `tab agents`, `ctrl+p commands`).
- [ ] Rotating tip line.

---

### 2. Session Transcript

**OpenCode behavior**

`routes/session/index.tsx` renders a scrollable message list. Each user turn is
`UserMessage`: a left-border block colored by the agent, plain text, optional
file badges, and a timestamp/queued badge. Assistant turns are `AssistantMessage`
which iterate `PART_MAPPING` (`text`, `tool`, `reasoning`). Text is rendered with
a streaming markdown component (`internalBlockMode="top-level"`). Tools are
rendered inline with `InlineTool`, one per tool call, with a 2-cell icon, verb
title, subtitle, and `↳` continuation lines. A final line per assistant turn
shows the agent, model, duration, and an `▣` marker. There is no heavy
full-width user message box.

Adjacent files surveyed: `routes/session/footer.tsx`,
`routes/session/sidebar.tsx`, `component/todo-item.tsx`, `util/transcript.ts`,
`util/collapse-tool-output.ts`.

**Harness seam**

- Transcript surface construction:
  `crates/harness-tui/src/ui_transcript_render.rs::build_transcript_render_surfaces`.
- Markdown rendering:
  `crates/harness-tui/src/ui_markdown.rs::append_rich_text_block` and
  `parse_inline_markdown_spans`.
- User-message surface:
  `ui_transcript_render.rs::build_user_render_surface`.
- Assistant footer (the `▣` / timing line):
  `ui_transcript_render.rs::build_assistant_footer_render_surface`.

**Parity status: gap**

Current differences:

- Each user message is wrapped in a heavy, full-width emphasized surface with
  a `›` prefix. OpenCode uses a subtle left-border panel with plain text.
- Assistant turns currently inject a `• agent · model · Ns` bullet between
  steps. OpenCode keeps agent/model in the prompt footer and only adds a final
  `▣` marker line after the turn.
- The markdown parser at `ui_markdown.rs::parse_inline_markdown_spans` has a
  known word-fusion regression where adjacent tokens are emitted without spaces
  and retain stray colored backgrounds.
- OpenCode renders file attachments inside the user message as inline badges;
  Harness does not yet render equivalent badges in the user surface.

**Adaptations / blockers**

- Per-turn timing cannot move to a TUI-local footer alone; it must be sourced
  from event replay-derived data (`ActivityStatus`/`duration_ms`).
- Replay mode is read-only and must not emit live submission intents, so any
  message-action affordances (copy/edit) must be disabled or turned into
  replay-safe overlays.

**Checklist**

- [ ] Unbox user messages; use left-border + plain text flow.
- [ ] Remove inline `• agent · model · Ns` bullets; move model info to footer.
- [ ] Fix markdown word-fusion / stray highlight bug.
- [ ] Add inline file-attachment badges for user messages.
- [ ] Ensure final `▣` marker matches OpenCode's agent · model · duration line.

---

### 3. Footer

**OpenCode behavior**

`routes/session/footer.tsx` shows a single horizontal row. Left side: the current
working directory. Right side: a status cluster that, when connected, shows
permission warnings (`△ N Permission`), a colored `•` next to the LSP count, a
success/error `⊙` next to the MCP count, and a `/status` affordance. The footer
never displays generic keybinding help such as `Enter send q quit`.

Adjacent files surveyed: `context/directory.tsx`, `component/use-connected.tsx`,
`context/sync.tsx`.

**Harness seam**

- Primary footer renderer: `crates/harness-tui/src/ui_chrome.rs::render_footer`.
- Status candidates: `ui_chrome.rs::live_footer_status_candidates` and
  `live_footer_status_cluster_candidates`.
- Footer hint model: `crates/harness-tui/src/view_model.rs` (FooterHint
  structure) and `ui_chrome.rs::compact_footer_hints`.

**Parity status: gap**

Current differences:

- Harness renders keybinding hints (`Enter send`, `q quit`, etc.) on the right
  side of the footer instead of OpenCode's status cluster.
- The status cluster (`△ N Permission`, `• N LSP`, `⊙ N MCP`, `/status`) is
  computed but currently competes for space with the hint row; OpenCode reserves
  the footer for status only.
- Replay mode suppresses the live footer entirely instead of adapting it to a
  read-only status row.

**Adaptations / blockers**

- The prior PRD owns footer *behavior* (T-UI-01); this PRD owns the *visual*
  target. The visual grouping of status left/right must be merged with the
  prior PRD's functional requirements.
- Generic footer hints must be moved into the prompt/composer chrome or
  demoted to a collapsed help overlay, matching OpenCode.

**Checklist**

- [ ] Directory label on the left.
- [ ] Right cluster: permission warning, LSP dot/count, MCP dot/count,
      `/status`.
- [ ] Remove generic keybinding hints from the footer.
- [ ] Replay mode shows a read-only status row, not a blank footer.

---

### 4. Sidebar

**OpenCode behavior**

`routes/session/sidebar.tsx` renders a 42-column right-hand pane with a panel
surface. Title area shows the generated session title in bold, then the session
ID, workspace label (with icon and status), and share URL when present. The
body is filled by plugin slots (`sidebar_content`). The footer shows the
OpenCode brand line (`● OpenCode <version>`). The sidebar is persistent on wide
terminals and overlays the transcript on narrow ones.

Adjacent files surveyed: `component/workspace-label.tsx`,
`context/project.tsx`, `context/theme.tsx`.

**Harness seam**

- Sidebar surface: `crates/harness-tui/src/ui_secondary.rs::render_operator_sidebar`.
- Section layout: `crates/harness-tui/src/ui_secondary/sidebar_sections.rs::build_operator_rail_body_layout`.
- Data model: `crates/harness-tui/src/ui_secondary/sidebar_data.rs::build_operator_rail_model`.
- Title generation: `sidebar_data.rs::operator_sidebar_session_title`.

**Parity status: gap**

Current differences:

- Harness has the right structural pieces (generated title, LSP list, MCP
  per-server status, modified-files diffstat, brand footer) but they are
  grouped differently from OpenCode's Context/MCP/Changes semantics.
- The `Modified Files` section already shows `+N -M` counts from
  `ui_diff::structured_diff_stats`, matching the OpenCode diffstat style.
- OpenCode's workspace label and share URL are omitted because Harness does not
  ship cloud/share workspaces.
- The Harness brand footer currently reads `• Harness <version>` and is placed
  below a directory footer; OpenCode shows only the brand footer.

**Adaptations / blockers**

- Workspace/share URL fields are excluded by the §4 non-goal list.
- LSP and MCP data come from runtime config and live tool-call state, not from
  a separate sync service, so the visual grouping must stay congruent with
  `harness_core::config`.

**Checklist**

- [ ] Generated session title (not `run_<id>`).
- [ ] LSP section lists active servers.
- [ ] MCP section shows per-server status dots.
- [ ] Modified Files shows `+N -M` diffstat.
- [ ] Brand footer `● Harness <version>`.
- [ ] Omit workspace/share URL regions (justified by §4).

---

### 5. Command Palette

**OpenCode behavior**

`component/command-palette.tsx` builds a `DialogSelect` list from
`keymap.getCommandEntries`. Header is titled "Commands". Suggested commands
are hoisted into a "Suggested" section. Each row shows the command title and the
formatted keybinding (`footer`). Selected row uses a salmon full-width bar.
Section headers are purple/bold. There are no inline descriptions; the right
column is reserved for the keybinding.

Adjacent files surveyed: `ui/dialog-select.tsx`, `keymap.ts`,
`util/format.ts` (`formatKeyBindings`).

**Harness seam**

- Overlay dispatcher: `crates/harness-tui/src/ui_overlays.rs::render_command_palette_overlay`.
- Row rendering: `ui_overlays.rs::command_palette_row` and
  `command_palette_section_row`.
- Color tokens: `crates/harness-tui/src/ui_chrome.rs::command_palette_section`,
  `command_palette_selection_bg`, `command_palette_selection_fg`,
  `command_palette_cursor`.
- Command registry: `crates/harness-tui/src/keybindings.rs` (actions and palette
  labels).

**Parity status: gap**

Current differences:

- Selection background is wired to `theme.text.accent` (orange `#f5a742`) and
  section headers are hardcoded to a purple `command_palette_accent()`. OpenCode
  uses peach/salmon for the selection bar and purple only for section headers.
- The right column currently shows `Action::palette_command_label` / command ids
  in some modes instead of the formatted keybinding.
- Inline descriptions are rendered next to the title; OpenCode drops
  descriptions and shows only title + keybinding.
- The command set is smaller; OpenCode-style rows such as "Switch model",
  "Open editor", "Skills", "View status", "Switch theme", "Help", "Open docs",
  "New/Continue/Replay session" are missing for commands that Harness actually
  supports.

**Adaptations / blockers**

- Commands must map only to real Harness actions. Inventing commands for
  absent features is prohibited by §4.
- The palette is reused by slash commands, model switcher, and session
  history; color changes must not break those surfaces.

**Checklist**

- [ ] Purple section headers.
- [ ] Salmon full-width selection bar.
- [ ] Right column shows keybinding, not command id.
- [ ] Drop inline descriptions.
- [ ] Add rows for supported commands only.

---

### 6. Tool-Call Rows

**OpenCode behavior**

`routes/session/index.tsx` dispatches each tool part through `PART_MAPPING`. The
presentation primitives are `InlineTool` and `BlockTool`. Titles are verb
forms: `Read <path>`, `Wrote <path>`, `Patched <path>`, `Grep "<pattern>"`,
`Glob "<pattern>"`, `WebFetch <url>`, `Ran <cmd>`, `Skill "<name>"`, etc.
Generic tools fall back to `InlineTool` with an icon and the raw tool id.
Subagent tasks show an icon plus
`<Agent> Task (background)? — <description>` and `↳` continuation lines. The
`Read` component emits `↳ Loaded <path>` lines. Completed tool groups are
auto-collapsed into "Gathered context".

Adjacent files surveyed: `component/todo-item.tsx`, `util/locale.ts`,
`tool/` set (`tool/read.ts`, `tool/grep.ts`, etc.).

**Harness seam**

- Tool title generation: `crates/harness-tui/src/ui_tool_titles.rs`
  (`generic_tool_name`, `mcp_tool_title`, `batch_tool_title`,
  `background_output_tool_title`).
- Tool section rendering: `crates/harness-tui/src/ui_transcript_tool_render.rs`
  `append_tool_call_section_lines`.
- Subagent/task title formatting: `crates/harness-tui/src/ui_tool_delegation.rs`
  and `ui_transcript_render.rs::build_context_tool_group_render_surface`.
- Tool status tokens: `crates/harness-tui/src/ui_chrome.rs::tool_status_tokens`.

**Parity status: gap**

Current differences:

- `ui_tool_titles.rs::generic_tool_name` returns the raw `tool_id`
  (`read`, `edit.apply_patch`, `grep`, `bash`). OpenCode maps these to
  verb forms.
- The context-tool group summary in
  `ui_transcript_render.rs::build_context_tool_group_render_surface` uses
  "Gathered context" / "Gathering context" which is close to OpenCode, but the
  detailed rows still show raw titles.
- MCP tools are titlecased from the server/tool name but do not follow
  OpenCode's verb-first pattern.

**Adaptations / blockers**

- Harness has more native tools than OpenCode (`ast-grep`, `batch`,
  `codesearch`, `github.*`, `lsp.rename`, `session_*`). They must get
  verb-form titles that are consistent with the OpenCode language but still
  identify the Harness-specific tool.
- The PRD §3 decision to retain these extra tools means the title map cannot
  be a 1:1 copy of OpenCode's smaller set.

**Checklist**

- [ ] Replace raw-id fallback with a verb map.
- [ ] Core tools: `Read`, `Wrote`, `Patched`, `Grep`, `Glob`, `WebFetch`,
      `WebSearch`, `Ran`, `Skill`, `Task`.
- [ ] Extra native tools get consistent verb forms (e.g. `Searched (ast-grep)`,
      `Read session …`).
- [ ] Unknown/MCP tools fall back to titlecased name with optional server.
- [ ] Preserve `↳` continuation lines (already present).

---

### 7. Inline Diff

**OpenCode behavior**

`routes/session/index.tsx` renders `Edit` and `ApplyPatch` tool results with the
`<diff>` component. It supports `view="split"` on wide terminals and
`view="unified"` when narrow. It shows line numbers, syntax highlighting, and
red/green backgrounds. The title is "← Edit <path>" or "← Patched <path>"
("Created", "Deleted", "Moved" variants for `apply_patch`). A collapsed raw
payload view is shown only as a disclosure fallback. `util/collapse-tool-output.ts`
truncates long outputs.

OpenCode also ships a full-screen interactive diff viewer in
`feature-plugins/system/diff-viewer.tsx` (with `diff-viewer-file-tree.tsx`,
`diff-viewer-file-tree-utils.ts`, and `diff-viewer-ui.tsx`). It registers a
`diff` route, shows a file tree sidebar, split/unified source views,
per-file "mark reviewed" state, and hunk navigation. Harness does not expose a
dedicated full-screen diff route in v1; parity for this PRD is scoped to the
inline transcript diff presentation.

Adjacent files surveyed: `util/collapse-tool-output.ts`,
`context/theme/opencode.json` (diff color tokens), `tool/edit.ts`,
`tool/apply_patch.ts`, `feature-plugins/system/diff-viewer.tsx`,
`routes/session/permission.tsx`.

**Harness seam**

- Diff preview detection: `crates/harness-tui/src/ui_tool_diffs.rs`
  (`tool_call_has_diff_preview`, `collect_apply_patch_file_render_entries`).
- Inline diff construction: `ui_tool_diffs.rs::tool_call_inline_diff_block`.
- Diff rendering: `crates/harness-tui/src/ui_diff.rs`
  (`render_structured_diff_lines_with_options`, `render_structured_diff_model`
  in `ui_diff_render.rs`).
- Transcript wiring:
  `crates/harness-tui/src/ui_transcript_tool_render.rs::append_tool_call_diff_block`.

**Parity status: gap**

Current differences:

- The diff rendering machinery exists and is even tested against the OpenCode
  color reference in `ui_diff.rs`, but the `apply_patch` tool path currently
  renders the raw `*** Begin Patch ***` payload plus an artifact link in live
  flows.
- `tool_call_inline_diff_block` only fires for the `edit` and `fs.write`
  tool ids; it does not yet handle the `apply_patch` input shape.
- OpenCode collapses long diff output; Harness does not consistently apply the
  same truncation before deciding to switch to disclosure.

**Adaptations / blockers**

- Diff artifacts are stored relative to the session directory. The TUI reads
  them during replay, so the feature stays side-effect free.
- `apply_patch` can affect multiple files. Harness must render one diff block
  per file, with a header, matching OpenCode's `For each={files()}` loop.

**Checklist**

- [ ] Wire `apply_patch` to `tool_call_inline_diff_block` / diff rendering.
- [ ] Render one line-numbered diff block per file.
- [ ] Use side-by-side view above 120 cols, unified below.
- [ ] Keep raw payload only as a collapsed disclosure.
- [ ] Align diff colors with `opencode.json` reference (already tested in
      `ui_diff.rs`).

---

### 8. Thinking / Todo Blocks

**OpenCode behavior**

`routes/session/index.tsx` renders reasoning parts with
`ReasoningPart`/`ReasoningHeader`. In minimal mode a single collapsible line
shows `Thinking: <summary>` or a spinner; expanded mode shows the body as muted
markdown. Todos are rendered by `TodoWrite` as a `# Todos` `BlockTool` where
each item is a checkbox line using `TodoItem`. Both blocks sit inline in the
assistant turn, not in a separate sidebar.

Adjacent files surveyed: `component/todo-item.tsx`, `context/thinking.tsx`,
`util/locale.ts`.

**Harness seam**

- Reasoning block: `crates/harness-tui/src/ui_transcript_render.rs::append_reasoning_block`.
- Inline todo list: `crates/harness-tui/src/ui_transcript_tool_render.rs::append_tool_call_todo_list`.
- Sidebar todo section: `crates/harness-tui/src/ui_secondary/sidebar_data.rs::operator_sidebar_todo_items`
  and `ui_secondary/sidebar_sections.rs::build_operator_rail_section_lines`.
- Activity-level reasoning data: `crates/harness-tui/src/app.rs`
  (`TranscriptLabeledTextSection`).

**Parity status: partial / gap**

Current differences:

- Reasoning already renders as `Thinking:` / `Thought` with a spinner and
  duration, which is close to OpenCode's inline form. The toggleable expansion
  and summary extraction are not yet implemented.
- Todos are rendered both inline (when the `todo.write` tool result is shown)
  and in the sidebar. OpenCode keeps them inline in the transcript as a `#
  Todos` block. Harness should align the primary placement to inline and
  optionally keep the sidebar as a secondary summary.
- Checkbox glyphs differ: Harness uses `[✓]`, `[•]`, `[ ]`; OpenCode uses its
  own `TodoItem` component style.

**Adaptations / blockers**

- Reasoning content may be redacted in persisted events; the renderer must
  handle empty/missing text gracefully.
- Sidebar todo visibility is useful for operators and is not removed, but the
  transcript form must match OpenCode's hierarchy.

**Checklist**

- [ ] Inline `# Todos` block in the assistant turn.
- [ ] Collapsible `Thinking:` header with summary when available.
- [ ] Expanded reasoning body as muted markdown.
- [ ] Keep sidebar todo section as secondary summary.

---

## Cross-cutting theme notes

OpenCode's default dark palette lives in
`context/theme/opencode.json`. The relevant roles for the surfaces above are:

- `primary`/`darkStep9` `#fab283` — prompt border, focused accents.
- `accent`/`darkAccent` `#9d7cd8` — section headers, command palette headers.
- secondary `#56b6c2` — live/secondary accents.
- selection bar `#ffc09f` — salmon full-width highlight.
- warning `#f5a742` — orange; OpenCode uses this only for warnings, not for
  the primary selection bar or logo.

Harness already encodes most of these values in
`crates/harness-tui/src/theme.rs::Theme::harness_dark`, but the token wiring in
`ui_chrome.rs::command_palette_*` currently forces orange into selection and
prompt-border roles. The theme role remap is PRD workstream WS-A and affects
almost every surface above.

---

## Evidence and verification

Verification for this doc itself:

```bash
scripts/test-lanes.sh fast
```

The doc is a static contract artifact. Any code change that closes a gap above
must also update this doc's checklist and status column and must pass the
relevant crate tests (for example `cargo test -p harness-tui` and the targeted
snapshot lanes named in each PRD workstream).
