# Harness TUI Design Contract (Reference-Measured)

> **Status:** Binding presentation contract for reference-parity work.
> **Source of truth:** Frozen Grok Build captures plus clean-room behavioral audits of the pinned local source — **not** current Harness chrome, theme tokens, or implementer preference.
> **Identity rule:** Harness branding may substitute logo glyphs and product text only. Geometry, rhythm, borders, focus, and choreography stay reference-shaped.

## 0. Evidence bases

| Corpus | Path | Viewport | Notes |
|---|---|---|---|
| Freeze startup (canonical) | `artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run{1,2,3}-startup/` | 120×32 | Three identical ref-vs-ref runs (`reference-freeze.receipt.json`) |
| Freeze draft | `.../reference/freeze/run{1,2,3}-draft/` | 120×32 | Welcome cleared after typing `Browser QA draft` |
| Diagnostic startup | `/tmp/opencode/artifacts/harness-xterm-qa/evidence/grok-startup/` | 120×32 | Same shell anatomy; model badge shows `test-model` |
| Diagnostic draft | `/tmp/opencode/artifacts/harness-xterm-qa/evidence/grok-draft/` | 120×32 | Same draft transition; breadcrumb may show token usage |
| Freeze receipt | `artifacts/qa-evidence/20260717-tui-reference-parity/receipts/reference-freeze.receipt.json` | — | Binary SHA-256, font stack, Chromium/xterm versions |
| Complete chat-shell parity corpus | `artifacts/qa-evidence/20260801-grok-chat-shell-parity-final/` | 60×20 through 140×40 | Lifecycle, tools/diffs, overlays, questions, CJK, responsive, and synchronized motion evidence |
| Pinned reference source | `inspirations/grok-build/crates/codegen/xai-grok-pager*` and `xai-ratatui-inline` | — | Observable state, hit-testing, grouping, diff, terminal-output, and pacing rules extracted without transplanting implementation |

**Measured files per freeze run:** `terminal.txt`, `terminal-ansi.txt`, `terminal.png`, `metadata.json`.

The complete chat-shell corpus supersedes the startup-only interim evidence for compact viewports, overlays, permission/question surfaces, transcript/tool blocks, and source-derived GrokNight color roles.

---

## 1. Invalidated prior claim (DIV-004)

### DIV-004 — “Compose-first home = reference parity” is **INVALID**

Harness AGENTS and signoff text currently describe a **compose-first home** as: centered logo, onboarding hints, bare prompt, model line, bottom status — without a bordered welcome panel, without breadcrumb/warning chrome matching the reference, and **without clearing welcome content on first keystroke**.

Measured reference startup is **not** that surface:

| Concern | Measured reference (freeze) | Prior Harness “compose-first” |
|---|---|---|
| Top chrome | Breadcrumb + optional warning | Path/status bottom bar only |
| Primary body | Bordered welcome panel (logo, title, changelog, action rows) | Centered block logo + hint lines |
| Composer | Rounded **bordered** 3-row box with `❯` and right model badge on bottom edge | Unboxed `❯` + placeholder + model line |
| Typing | Welcome **clears**; body empties; composer + shortcut footer remain | Logo/hints **remain** while draft edits |
| Footer | Startup: right-aligned auth/status; draft: left-aligned shortcut strip | Different vocabulary and placement |

**Rule going forward:** “Compose-first” may still mean *keyboard focus starts in the composer* and *home is not a replay browser*, but it is **not** acceptance for visual parity. Parity requires the measured shell vertical order, welcome anatomy, bordered composer, footer grammar, and startup→draft clear transition documented here.

Diagnostic proof of the gap (same pipeline, 120×32): Harness startup/draft PNGs still show the centered HARNESS logo after typing; Grok draft clears the welcome panel.

---

## 2. Shell vertical order (measured @ 120×32)

Canonical order top → bottom (1-based terminal rows from freeze `run1-startup/terminal.txt`; export has 31 non-empty-trailing lines for a 32-row viewport — treat row 32 as blank/export trim):

| Region | Rows (approx.) | Content |
|---|---|---|
| Top margin | 1 | Blank |
| **Breadcrumb** | 2 | Branch glyph + branch/path context (` …`); dim intensity |
| Spacer | 3–4 | Blank |
| **Warning** (conditional) | 5–6 | Right-biased multi-line notice (`Clipboard may be unreachable.` / `See /terminal-setup…`) |
| Spacer | 7 | Blank / soft pad |
| **Welcome panel** | 8–23 | Rounded bordered box (16 rows) |
| **Body** | 24–26 | Empty gap between welcome and composer (3 blank rows at this viewport) |
| **Composer** | 27–29 | Rounded bordered 3-row strip |
| Spacer | 30 | Blank |
| **Footer** | 31 | Status / shortcuts |

### Draft (typed) vertical order

| Region | Behavior |
|---|---|
| Breadcrumb | **Retained** (may gain right-side token usage in some runs) |
| Warning | **Cleared** |
| Welcome panel | **Cleared entirely** |
| Body | Empty (transcript area once live; empty at pure draft) |
| Composer | **Retained**; draft text after `❯` |
| Footer | Switches to shortcut strip (`Enter:send │ Shift+Tab:mode │ Ctrl+x:shortcuts`) |

---

## 3. Breakpoints and compact behavior

| Viewport | Status | Freeze (3× identical) |
|---|---|---|
| **120×32** | Primary design lock | `run{1,2,3}-startup` / `run{1,2,3}-draft` |
| **120×40** | Measured startup | `run{1,2,3}-startup-120x40` — same welcome+composer anatomy; extra vertical gap between welcome and composer |
| **100×30** | Measured startup | `run{1,2,3}-startup-100x30` — full bordered welcome retained (narrower box); bordered composer retained |
| **80×24** | Measured startup + draft | `run{1,2,3}-startup-80x24` / `run{1,2,3}-draft-80x24` |
| **79×24, 60×20, 140×40, 120×50** | Measured responsive shell | Final comparison boards `17` through `23` |

### Compact collapse @ 80×24 (measured)

**Startup** drops the **bordered welcome panel** entirely. Retains:

| Region | Behavior |
|---|---|
| Breadcrumb | Present (` branch path`) |
| Clipboard warning | Present when applicable |
| Actions | Unboxed action rows (New worktree / Resume / Changelog / Quit + shortcuts) |
| Changelog body | Unboxed section label + bullet list (truncated with `…`) |
| Composer | **Still bordered** 3-row box; model badge on bottom border |
| Footer | Right-biased auth/status |

**Draft @ 80×24:** breadcrumb retained (may show token usage); welcome/actions/changelog cleared; optional tip `Tight on space? Try /compact-mode`; bordered composer retains draft; footer → `Enter:send  │  Shift+Tab:mode  │  Ctrl+x:shortcuts`.

**Rule:** Collapse is **not** “unbox the composer”. Composer borders survive compact. Only the welcome chrome collapses at 80×24.

---

## 4. Padding / spacing rhythm (measured)

All columns 0-based character indices in the text grid (freeze startup):

| Element | Measurement |
|---|---|
| Breadcrumb left pad | 2 spaces before `` |
| Welcome outer left | 3 spaces before `╭` |
| Welcome box width | 114 cells (`╭` col 3 … `╮` col 116); top/bottom `─` count = **112** |
| Welcome inner content pad | 2 spaces after left `│` before logo/text cluster |
| Logo column | Content starts ~col 6 (after `│` + 2 spaces) |
| Title / “Changelog” / action labels | Content column **23** (aligned under title block, right of logo) |
| Action shortcut column | `ctrl+w` / `ctrl+s` / `ctrl+q` start at col **108**, right-padded before right border |
| Gap welcome → composer | **3** blank rows |
| Composer outer left | **2** spaces before `╭` (1 cell left of welcome inset) |
| Composer box width | 116 cells (`╭` col 2 … `╮` col 117); top `─` count = **114** |
| Composer prompt | `│` col 2, space, `❯` col 4, space, draft text col 6+ |
| Footer draft left pad | 2 spaces before `Enter:send` |
| Footer startup | Right-biased auth/provider summary; connected API-key capture starts `Logged in with API key` ~col 87, with `│` at 111 and `Beta` at 114 |

**Rhythm summary:** outer horizontal inset is small (2–3 cells). Welcome is slightly more inset than the composer. Vertical rhythm is sparse: multi-row blank bands separate breadcrumb / warning / welcome / composer / footer rather than packed card stacks.

---

## 5. Composer anatomy (measured)

### Structure (always 3 text rows when single-line draft)

```text
  ╭──────────────────────────────── … ──────────────────────────────╮
  │ ❯ [draft text …]                                                │
  ╰──────────────────────────────── … ──────────── [model badge] ─╯
```

| Part | Spec |
|---|---|
| Border | Rounded box-drawing: `╭╮╰╯` + `─` + `│` |
| Prompt glyph | `❯` (U+276F) immediately inside left border after one space |
| Draft text | Same row as `❯`, one space after glyph |
| Height (single line) | Exactly **3** rows (top border, content, bottom border) |
| Dynamic height | Multi-line drafts wrap inside the border and cap at 6 content rows |
| Model badge | Embedded in **bottom border row**, right-aligned before the closing `╯`, with `─` padding; examples: `test-model`, `unknown`, or blank spacer `  ─` when empty |
| Focus | Composer is the edit surface at startup; cursor ends on the draft line after paint |

### Cursor (from `terminal-ansi.txt`)

- Alternate screen + mouse + bracketed paste enabled in capture pipeline.
- Cursor hidden during bulk paint (`?25l`), then shown (`?25h`).
- Final cup after startup paint: row **28**, col **7** (cell after `❯` + space) — empty-draft caret position.
- Draft run ends with cup on the draft line after typed text (e.g. positions around row 28).

---

## 6. Welcome panel anatomy (measured)

Outer box: rows 8–23, cols 3–116, rounded borders.

### Interior vertical map (inside borders)

| Inner band | Content |
|---|---|
| Top pad | Empty row inside top border |
| Logo + title | Braille/block logo (7 content rows tall in capture) left; title `Grok Build Beta` **bold** + version `0.2.0-dev` normal to the right of logo |
| Section label | `Changelog` bold at title column |
| Changelog bullets | Three lines, each `• ` + sentence; indented under section |
| Spacer | Empty row |
| Harness action rows (4) | Real Harness actions only; label left (col 23), shortcut right (col 108) |
| Bottom pad | Empty row before bottom border |

### Action rows

| Label | Shortcut |
|---|---|
| `New worktree` | `ctrl+w` |
| `Resume session` | `ctrl+s` |
| `Changelog` | — |
| `Quit` | `ctrl+q` |

Labels and shortcuts use bold labels + normal/dim shortcuts (ANSI intensity). Shortcuts are right-aligned as a column, not trailing immediately after the label. `Changelog` opens the real Release Notes overlay and shares the same four-row WelcomeLayout used by keyboard, pointer, and session-history layering.

### Logo

- Rendered as multi-line braille art (not a single emoji).
- **Harness identity substitution:** replace braille/logo glyphs and the title string with Harness logo + `Harness` (and version), **keeping the same bounding columns and row count**. Do not reflow the panel around a different logo aspect without a new capture.

---

## 7. Overlay dimensions / z-order

| Topic | Status |
|---|---|
| Overlay sizes | Measured palette, session picker, permission/question, and help surfaces with deterministic bounds |
| Z-order | Modal overlays render above the shell and restore the composer/draft surface after dismissal |
| Preemption / dismiss | Keyboard dismissal restores prior focus; permission/question prompts retain transcript and bottom-shell geometry |

Evidence owners are final comparison boards `05`, `12`, and `14`–`16`, plus `question-pairs/`.

---

## 8. Border / separator grammar

| Token | Role |
|---|---|
| `╭` `╮` `╰` `╯` | Rounded rectangle corners (welcome + composer) |
| `─` | Horizontal edges; also model-badge padding on composer bottom edge |
| `│` | Vertical edges; also footer segment separator (` │ `) between shortcut clusters and status fields |
| **Not observed** at startup/draft | Sharp corners `┌┐└┘`, heavy blocks, left rail `┃`, card mid-rules |

**Rule:** Primary chrome boxes use **rounded** box drawing only. Do not reintroduce Harness card rails as a substitute for this grammar.

---

## 9. Glyph roles

| Glyph | Role (measured or contract-known) | Evidence |
|---|---|---|
| `❯` | Composer prompt / input affordance | Freeze + diagnostic composer |
| `•` | Changelog bullet | Welcome panel |
| `` (Powerline branch) | Breadcrumb VCS marker | Startup/draft row 2 |
| `│` | Box vertical + footer separator | Shell |
| `─` | Box horizontal + badge rail | Composer bottom |
| `◆` | Tool header marker | Final tool/diff capture states |
| `●` / `○` | Permission/question selected and unselected choices | Final permission/question captures |
| Braille block art | Welcome logo mass | Welcome panel |

---

## 10. Color roles

### From `terminal-ansi.txt` (freeze + diagnostic)

**No RGB (`38;2` / `48;2`) and no 256-color (`38;5` / `48;5`) sequences appear in the captured ANSI dumps.** Only intensity/reset:

| SGR | Observed use |
|---|---|
| `1` bold | Title (`Grok Build Beta `), section `Changelog`, action labels, draft footer key names (`Enter`, etc.) |
| `2` dim | Breadcrumb text; some footer separators |
| `22` / `0` / empty reset | Version string, shortcut chords, body text, borders |

### RGB / theme palette

| Role | Value | Status |
|---|---|---|
| Background | `#141414` (`20,20,20`) | Measured at pixel `(0,0)` in the pinned startup PNG |
| Selected user-message surface | `#555753` (`85,87,83`) | Measured at pixel `(409,45)` in the normalized frozen failure-state PNG |
| Elevated composer surface | `#1c1c1c` (`28,28,28`) | Measured at pixel `(339,57)` in the normalized frozen draft PNG |
| Scrollbar thumb | `#242424` (`36,36,36`) | Measured at pixel `(184,54)` in the normalized frozen draft PNG |
| Foreground primary / secondary | `#e1e1e1` / `#6c6c6c` | GrokNight source and final truecolor captures |
| Question accent / selected surface | `#c8c8c8` / `#363636` | GrokNight source and question captures |
| Focused composer border | `#505058` | GrokNight `prompt_border_active`; native 256-color capture quantizes to xterm 239 |
| Welcome panel border | `#333333` | GrokNight `gray_dim` blended 45% over `bg_base`, matching `welcome/hero_box.rs` |
| Active-thinking user surface | `#262626` | Native synchronized motion captures |
| Error / diff roles | Tokenized in `Theme::GROK_TERMINAL_COLORS` | Tool/diff and failure-state captures |

The observation receipt is `crates/harness-tui/tests/fixtures/harness-chat-rgb-observations.json`; the final source inventory records the additional GrokNight roles traced directly from the pinned source and synchronized captures.

### Default `harness-chat` role mapping

`Theme::harness_chat()` constructs this palette directly rather than inheriting
from `harness-dark`. The mapping is locked exhaustively by
`harness_chat_maps_every_visible_palette_role_to_groknight_truecolor` and
`harness_chat_maps_terminal_and_diff_roles_to_groknight_truecolor`.

| Harness role | GrokNight role | Truecolor |
|---|---|---|
| `surface.canvas`, `surface.shell`, `surface.panel`, `surface.overlay` | `bg_base` | `#141414` |
| `surface.panel_elevated` | `bg_dark` | `#1c1c1c` |
| `surface.card` | `bg_highlight` | `#242424` |
| `surface.hover` | `bg_hover` | `#2c2c2c` |
| `surface.selected_card` | pinned selected user surface | `#555753` |
| `border.subtle` | `prompt_border` | `#323237` |
| `border.strong` | `selection_border` | `#3c3c41` |
| `border.focus` | `prompt_border_active` | `#505058` |
| `text.primary` | `text_primary` | `#e1e1e1` |
| `text.secondary` | `gray` | `#6c6c6c` |
| `text.tertiary` | `gray_dim` | `#585858` |
| `text.accent` | `accent_thinking` | `#bb9af7` |
| `text.inverse` | `bg_base` | `#141414` |
| `question.surface` | `bg_highlight` | `#242424` |
| `question.selected` | `bg_visual` | `#363636` |
| `question.primary` | `text_primary` | `#e1e1e1` |
| `question.accent` | `accent_user` | `#c8c8c8` |
| `question.secondary` | `gray` | `#6c6c6c` |
| `status.success` | `accent_success` | `#9ece6a` |
| `status.warning` | `warning` | `#e0af68` |
| `status.error` | `accent_error` | `#f7768e` |
| `status.info` | `running` | `#7dcfff` |
| `status.disabled` | `gray_dim` | `#585858` |
| `markdown.heading_h1` | `md_heading_h1` | `#1abc9c` |
| `markdown.heading_h2` | `md_heading_h2` | `#7aa2f7` |
| `markdown.heading_h3` | `md_heading_h3` | `#9d7cd8` |
| `markdown.heading_h4` | `md_heading_h4` | `#787878` |
| `markdown.heading_h5` | `md_heading_h5` | `#6c6c6c` |
| `markdown.heading_h6` | `md_heading_h6` | `#5a5a5a` |
| `markdown.link` | `accent_system` | `#7aa2f7` |
| `markdown.link_text` | `link_fg` | `#7aa6da` |
| `markdown.code` | `md_code` | `#3a95ab` |
| `markdown.task_checked` | `accent_success` | `#9ece6a` |
| `markdown.task_unchecked` | `md_text` | `#c8c8c8` |
| `markdown.muted` | `md_muted` | `#6c6c6c` |
| `markdown.code_background` | `bg_dark` | `#1c1c1c` |
| `markdown.text`, `markdown.emph`, `markdown.strong` | `md_text` | `#c8c8c8` |
| `markdown.block_quote`, `markdown.list_item`, `markdown.list_enum`, `markdown.rule` | `md_muted` | `#6c6c6c` |
| `agent.build` | `accent_system` | `#7aa2f7` |
| `agent.plan` | `accent_verify` | `#bb9af7` |
| `agent.docs` | `warning` | `#e0af68` |
| `agent.ask` | `running` | `#7dcfff` |
| `scrollbar.track` | `scrollbar_bg` | `#111111` |
| `scrollbar.thumb` | `scrollbar_fg` | `#242424` |
| `scrollbar.thumb_active` | `prompt_border_active` | `#505058` |
| `reference_terminal.welcome_border` | `blend(bg_base, gray_dim, 0.45)` | `#333333` |
| `reference_terminal.diff_added`, `diff_added_gutter` | `diff_insert_bg` | `#063806` |
| `reference_terminal.diff_removed`, `diff_removed_gutter` | `diff_delete_bg` | `#420e14` |
| `reference_terminal.diff_added_highlight` | `diff_insert_fg` | `#9ece6a` |
| `reference_terminal.diff_removed_highlight` | `diff_delete_fg` | `#f7768e` |
| `reference_terminal.diff_hunk_header` | `accent_system` | `#7aa2f7` |

ANSI-256 quantizes these RGB values deterministically. ANSI-16 uses named
semantic fallbacks, and no-color mode uses the terminal-native reset palette;
those modes preserve role distinctions without claiming RGB identity.

Terminal capability negotiation also selects glyph and motion fallbacks. A
Unicode-capable terminal keeps the preferred composer and transcript prompt `❯` plus `◆`, `●`, and
`✗` status glyphs; the compact/legacy capability profile uses the semantic
ASCII alternatives `>`, `*`, `o`, and `x`. Transcript user rows use the stable
`❯` marker and elevated-band contract in section 14. Setting `HARNESS_TUI_REDUCED_MOTION` to `1`, `true`, `yes`,
or `on` selects the scheduler's immediate-settle path: status distinctions
remain visible, but continuous animation deadlines are not armed. Capability
evidence labels record color, glyph, and motion modes and must not describe a
reduced capture as pixel-identical truecolor output.

Capture paths for future color lock:

```text
artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run1-startup/terminal.png
artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run1-startup/terminal-ansi.txt
artifacts/qa-evidence/20260801-grok-chat-shell-parity-final/reference-pinned/startup-welcome-120x32/terminal.png
.config/artifacts/qa-evidence/20260801-grok-chat-shell-parity-final/reference-normalized/run1-shell-fail-pinned-v1/terminal.png
```

---

## 11. Focus / cursor rules (measured + interim)

| Rule | Spec |
|---|---|
| Startup focus owner | Composer input (caret on draft line after `❯`) |
| Cursor visibility | Shown when idle after paint; may hide during synchronized updates |
| Cursor position empty draft | Content row of composer, first editable cell after `❯ ` |
| Welcome actions | Visible labels and keyboard shortcuts; compact and primary layouts are captured |
| Overlay focus | Palette/help/session and question selection transitions are captured and restore prior focus on dismiss |
| Mouse | Capture enables mouse modes; pointer hit regions follow the rendered overlay and transcript geometry |

---

## 12. Startup → draft transition

**Measured input:** type `Browser QA draft` (freeze draft metadata `interaction`).

| Region | Startup | After first draft text |
|---|---|---|
| Welcome panel | Visible | **Removed** (all rows become empty body) |
| Warning | Visible if applicable | Cleared |
| Breadcrumb | Visible | Visible |
| Composer border + `❯` | Visible | Visible; text inserted after `❯` |
| Model badge | Present or blank spacer | Present (`unknown` / `test-model` depending on env) |
| Footer | Right auth/provider status (`Logged in with API key │ Beta` when connected; `Provider not connected │ Beta` otherwise) | Left shortcuts: `Enter:send  │  Shift+Tab:mode  │  Ctrl+x:shortcuts` |

**Harness must match this choreography.** Keeping the welcome logo while editing is a parity failure (current Harness draft capture).

---

## 13. Harness identity substitutions

Only these fields may differ from the reference while preserving geometry:

| Region | Reference example | Harness substitution | Geometry constraint |
|---|---|---|---|
| Welcome logo art | Braille Grok mark | Harness logo art | Same row span and max column width as reference logo cell |
| Title text | `Grok Build Beta` | `Harness` (+ channel label if needed) | Same title column; do not push changelog/actions |
| Version | `0.2.0-dev` | Harness version string | Same relative placement after title |
| Product strings in actions/docs | Grok-specific copy | Harness-equivalent labels **only when** the action maps to a real Harness capability | Keep row count and shortcut column; drop/replace unmapped actions with documented divergence IDs later |
| Footer auth wording | `Logged in with API key` | Harness-accurate auth/provider summary | Keep right-bias and `│` segment grammar |

**Must not change for identity:** border style, composer anatomy, vertical order, welcome clear-on-type, shortcut footer grammar, padding rhythm.

---

## 14. Transcript / live shell

The pinned reference set now covers the live shell, lifecycle, transcript tool and
diff rows, scroll state, permission/question activity, overlays, and responsive
breakpoints. These captures supersede the earlier startup-only interim contract.
For live sessions the implementation contract requires:

- Full-width transcript/body above the same bordered bottom composer (no persistent right operator sidebar as primary chrome).
- Operator facts on secondary surfaces (status dialog, details, palette, slash) — Harness seam language from `crates/harness-tui/AGENTS.md`.

### SHELL-IDLE

| Region | Contract |
|--------|----------|
| Body | Full-width transcript area; empty live shell is **card-free** (no elevated Session/Harness empty-state card, no value_prop / example_prompts body copy) |
| Composer | Same bordered strip as startup: rounded `╭─╮/╰─╯`, `❯` glyph, model badge on bottom border; multi-line drafts wrap inside the border (capped at 6 content rows) |
| Queue | When `queued_prompt_count > 0`, badge may append `· queued N` |
| Footer | Left anchored shortcut grammar, beginning at the shell's outer horizontal padding; no right-justified footer group |
| Idle status | No startup/session-progress row after the live shell has settled |
| Topology | No persistent right operator sidebar as primary chrome |

Structural owners: `crates/harness-tui/tests/reference_parity_tx_shell_test.rs` (`shell_idle_*`).

### TX-USER / TX-ASSISTANT

| Region | Contract |
|--------|----------|
| User rows | Full-width elevated band inside the transcript gutters: `surface.card` when idle, `surface.selected_card` when selected, and the active-thinking semantic surface while live; one blank surface row above and below the message vertically centers a single-line prompt; the content row starts with three cells of inset, the stable `❯ ` marker, then body text; wrapped rows align under the body; timestamps are visible by default, reserve the rightmost 10 cells before wrapping, end two cells before the band's right edge, show `h:mm AM/PM` at rest, expand leftward to `HH:mm:ss | Mon DD` when that 10-cell hit region is hovered, suppress safely when the row cannot fit, and disappear when the timestamps setting is disabled; no `You` label, legacy left rail `┃`, synthetic header, border, corner, or mid-rule at any measured width |
| Assistant rows | Rail-free body; footer/meta may show model/status on shell surface |
| Shared | No sharp corners / card mid-rules as primary transcript chrome |

Structural owners: `crates/harness-tui/tests/reference_parity_tx_shell_test.rs` (`tx_user_*`, `tx_assistant_*`) plus existing exact transcript tests.

### TX-TOOL / TX-DIFF

| Region | Contract |
|--------|----------|
| Group summary | Flat `◈` summary row (`Ran N commands` plus failure suffix); no card border or per-command body expansion |
| Context verb group | Consecutive non-destructive context activity folds from its first member into one source-ordered `◈` header such as `Read 1 file, Searched 2 patterns, Listed 1 dir`; any running member changes every bucket to present tense, and `· N failed` is the only error suffix |
| Command row | Flat `◆ Run …` row. Failed rows use the error accent/left accent state, without an extra `command failed` body line in collapsed mode |
| Edit/diff row | Flat `◆ edit`/path summary in collapsed mode; disclosure is represented by the fold indicator, not an always-expanded inline diff card |
| Running accent | The active entry has a one-cell animated accent rail; finished siblings retain their settled accent state |
| Fold state | Collapsed is the default transcript presentation; expansion is an explicit interaction and must preserve scroll/selection anchors |
| Context rail | Running and failed context groups use a one-cell heavy `┃` rail; running samples the shared elapsed-time wave, failed is static error, and settled/cancelled groups use a dim `❙`. The rail and `◈` share state color while the bold label remains muted |

Context verb-group nouns and boundaries follow the pinned reference vocabulary:
file/files, skill/skills, pattern/patterns, dir/dirs, website/websites,
and subagent/subagents. Reads of `SKILL.md` use the skill bucket. Read,
search, list, skill, web-fetch, web-search, and subagent rows may share one
source-ordered group; commands, edits, ordinary MCP dispatch, unknown tools,
and pending-user-input rows break it. A single context member folds immediately
to avoid a second-call layout jump. Collapsed groups hide members; expansion
keeps the header in place and reveals every member below it.

Measured owners: pinned `run1-tx-tool-pinned-v1` and
`run1-tx-diff-pinned-v1` captures under the parity reference freeze, plus
`scrollback/blocks/tool/*`, `scrollback/types.rs`, and
`scrollback/wrappers/entry_renderer.rs` in the frozen reference source.

### Live lifecycle matrix

The chat shell acceptance run must capture each row at `120x40` through the same
PTY, xterm.js, font, Chromium binary, DPR, locale, and terminal environment.
Identity text may differ only inside the declared semantic identity fields.

| State | Required visible contract |
|-------|---------------------------|
| idle | Header + empty body + composer + left-anchored shortcuts; no progress row |
| draft | Startup body cleared on first grapheme; composer cursor and draft remain inside the same border |
| streaming | User row, active turn-status spinner/elapsed/token segment, cancel hint, composer, shortcuts |
| tool running/success/failure | Flat grouped tool rows with active accent and collapsed output policy |
| diff/edit | Flat collapsed edit rows and fold indicators; expanded diff only after disclosure |
| permission | Permission activity and prompt state preserve transcript, draft, focus, and bottom-shell geometry |
| question | Question activity and prompt state preserve transcript, draft, focus, and bottom-shell geometry |
| cancelled | Terminal cancellation state replaces active status without moving the composer |
| failed | Error state uses error semantics without introducing a card or permanent rail |
| recovered | Subsequent active state clears the terminal error chrome and resumes normal animation |
| completed | Assistant body and completion duration settle; active status/cancel hint disappear |
| scrolled | Follow is disabled, viewport anchor and selection remain stable, and return-to-live affordance is visible when applicable |
| palette/help/session picker | Modal or prompt-anchored overlay is above the shell, has deterministic bounds, and restores prior focus/draft on dismiss |

Reference capture names: `run4-shell-idle-pinned-v1`,
`run1-shell-stream-pinned-v1`, `run1-shell-perm-pinned-v1`,
`run1-shell-question-pinned-v1`, `run1-shell-cancel-pinned-v1`,
`run1-shell-fail-pinned-v1`, `run1-shell-recover-pinned-v1`,
`run1-shell-complete-pinned-v1`, `run1-shell-scroll-pinned-v1`,
`run1-palette-pinned-v1`, `run1-ovl-help-pinned-v1`, and
`run1-ovl-session-pinned-v1`.

### Modal list rows

Compatible modal selectors use one gutter-aware row primitive. At normal widths,
the row band begins two cells inside the list and leaves two cells at the right;
scrolling lists reserve the final content-side cell for a position-aware
scrollbar. Compact geometry reduces the inset to one cell before allowing the
content band to collapse. Borders, titles, close chrome, and scrollbar cells are
never painted by the row band.

Pointer hit-testing uses the full logical row width, matching Grok's separation
between full-width item rectangles and inset paint rectangles. Both gutters and
the scrollbar-side cell therefore keep row hover/click behavior while retaining
the modal or scrollbar material beneath the pointer.

Keyboard selection uses `question.selected` with bold primary text. Pointer
hover uses `surface.hover`; when a row is both selected and hovered, the hover
background wins while the selected bold text remains. Normal rows use the modal
canvas, and unavailable rows use tertiary text unless selected. Pointer movement
may move the picker selection, matching the pinned Grok source; hover remains a
separate visual state so its softer material is still observable.

### Dynamic projection and frame contract

Moving-state parity is governed by the live reference scrollback behavior, not
by isolated settled screenshots. Harness keeps its event-sourced projection and
product identity, but the visible transition semantics are binding:

- **Stable entry identity.** A tool call owns one row identity from queued or
  running through terminal completion. Refining metadata, appending output,
  changing tense, or entering the finish flash updates that entry in place; it
  must not replace an inline row with a differently shaped block. Command rows
  are born as `Run <command>` and retain that header while output and disclosure
  rows appear below it.
- **Incremental dirty layout.** A live delta invalidates the changed entry and
  dependent cumulative heights only. Width changes may remeasure visible
  content, but restore a logical-line anchor rather than a wrapped display row.
  Structural insertion or removal uses a one-shot stable-entry anchor.
- **Follow is reading intent.** Tail growth follows only while follow mode is
  armed. Manual scroll, selection, expansion, or fold growth that would move
  the reader's content disarms follow until the explicit return-to-live action.
- **Prompt transactions.** Permission and question surfaces stash the composer
  draft and prior pane focus once, keep transcript geometry stable while open,
  and restore both on resolve or dismiss. Unfocused prompts dim; focused prompts
  do not animate their bounds.
- **Blocking-card focus.** Permission and question cards force prompt-pane focus
  only when the queue first opens. Bare Tab and Shift+Tab wrap through answer
  rows; Ctrl/Alt/Super-modified Tab is inert. Esc parks an unanswered card in
  scrollback (or clears the active question selection first), while Ctrl+C is
  the explicit cancellation chord and Shift+X also cancels questions. Horizontal
  arrows, `h`/`l`, and brackets switch questions; Enter accepts the focused row
  regardless of modifiers, while Space toggles it. In custom text, bare Enter
  commits, Shift/Alt+Enter inserts a newline, and Ctrl/Super+Enter is inert.
- **Shared live motion.** Active tool and thinking rails use one lifecycle:
  queued or waiting static state, a 32-row continuous spatial wave sampled from
  elapsed time, frozen static frame when user input requires attention or motion
  is reduced, a 400 ms elapsed-time terminal finish flash, then settled semantic
  color. The rail, tool glyph, group marker, and status label sample the same
  brightness. Motion never changes row count or wrapping, and skipped frames do
  not extend a transition.
- **Dense activity groups.** Consecutive tool and transparent completed-thought
  entries use zero internal vertical gaps. One group projection owns member
  visibility, synthetic summary rows, expansion, height, navigation, hit testing,
  and copy. Dense runs show at most 10 member entries before an aggregated hidden
  count; separate conversational turns retain one full separator row.
- **Conditional bottom dock.** Disclosure and status rows exist only while they
  carry distinct live state. An empty unfocused composer collapses to one row.
  At 20 rows or fewer optional vertical padding is removed; at 16 rows or fewer
  tips, disclosure, redundant status spacing, and bottom margins are suppressed.
- **Normalized scroll streams.** Raw direction-only terminal mouse reports form
  streams separated by 80 ms. Terminal and multiplexer profiles, automatic or
  forced wheel/trackpad mode, fractional line carry, duplicate-safe interval
  acceleration, inversion, speed, line count, viewport-relative caps, and a
  16 ms flush cadence resolve logical lines before presentation pacing.
  `HARNESS_TUI_SCROLL_MODE`, `HARNESS_TUI_SCROLL_LINES`,
  `HARNESS_TUI_SCROLL_SPEED`, and `HARNESS_TUI_INVERT_SCROLL` override those
  preferences when terminal auto-detection is not the desired behavior.

Frame timing uses the measured reference cadences: active animation at `33 ms`,
write coalescing at `16 ms`, slow/background chrome at `83 ms`, scroll sampling
at `16 ms`, an `80 ms` stream-gap finalizer, and a `400 ms` terminal finish
flash. Harness may batch more aggressively internally, but it may not emit more
than one unacknowledged terminal frame, skip the final stream frame, or write
bytes after the UI is settled. Reduced motion performs one deterministic state
transition and parks continuous timers.

Acceptance requires ordered before/mid/after PTY frame traces. A settled capture
alone cannot prove this contract.

---

## 15. Module disposition

Machine-readable classifications live in:

```text
docs/tui-reference-module-disposition.v1.json
```

Enums: `replace` | `rework` | `retain-seam-only` | `retain-with-reference-proof`.

Default for old visual painters: `replace` or `retain-seam-only`. No module is `retain-with-reference-proof` until L2–L5 evidence exists.

---

## 16. Implementation notes (non-presentation)

- Layout math belongs in `layout.rs` / theme geometry once rebuilt to this contract — not ad-hoc in widgets.
- App seams (`SessionProjection`, permissions state, `UiIntent`, overlay stack pointer) remain Harness-owned; see `src/app/AGENTS.md`.
- Do not copy reference source, tests, themes, or identifiers (`docs/grok-build-tui-implementation-prompt.md` §7).

---

## 17. Measurement and proof matrix

The responsive shell is measured at `120x50`, `120x40`, `100x30`, `80x24`,
`79x24`, `60x20`, and `140x40`. A release-quality parity run captures all seven
viewports and every state in the live lifecycle matrix above. Pixel comparison is
zero-tolerance outside semantic identity regions; cell comparison also checks
glyph, foreground/background color, modifier, cursor, focus, and z-order.

Animation proof is an ordered frame trace, not one settled screenshot: active
spinner/accent frames run at the measured 30 fps cadence, elapsed text remains
monotonic, reduced/disabled motion is deterministic, and settled idle requests no
animation redraws. Tool expansion, permission/question selection, overlay focus,
multiline composer growth, and scroll/follow each require both the before and
after frame plus a state-transition assertion.

Tool motion uses the design-contract `ToolPulse` token for active running rails
and `ToolFinishFlash` (33 ms × 12 frames, approximately 400 ms) for the bounded terminal transition.
Queued, waiting, replayed, off-screen, and settled tool rows remain static.

If a state cannot be produced against both exact binaries, its row remains
`incomplete`; it cannot be promoted by a structurally similar helper frame or by
the former tool/diff or overlay divergence exemptions.
