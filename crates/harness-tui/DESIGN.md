# Harness TUI Design Contract (Reference-Measured)

> **Status:** Binding presentation contract for reference-parity work (Wave T03).  
> **Source of truth:** Frozen black-box Grok Build captures — **not** current Harness chrome, theme tokens, or implementer preference.  
> **Identity rule:** Harness branding may substitute logo glyphs and product text only. Geometry, rhythm, borders, focus, and choreography stay reference-shaped.

## 0. Evidence bases

| Corpus | Path | Viewport | Notes |
|---|---|---|---|
| Freeze startup (canonical) | `artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run{1,2,3}-startup/` | 120×32 | Three identical ref-vs-ref runs (`reference-freeze.receipt.json`) |
| Freeze draft | `.../reference/freeze/run{1,2,3}-draft/` | 120×32 | Welcome cleared after typing `Browser QA draft` |
| Diagnostic startup | `/tmp/opencode/artifacts/harness-xterm-qa/evidence/grok-startup/` | 120×32 | Same shell anatomy; model badge shows `test-model` |
| Diagnostic draft | `/tmp/opencode/artifacts/harness-xterm-qa/evidence/grok-draft/` | 120×32 | Same draft transition; breadcrumb may show token usage |
| Freeze receipt | `artifacts/qa-evidence/20260717-tui-reference-parity/receipts/reference-freeze.receipt.json` | — | Binary SHA-256, font stack, Chromium/xterm versions |

**Measured files per freeze run:** `terminal.txt`, `terminal-ansi.txt`, `terminal.png`, `metadata.json`.

**Not yet captured (mark TBD below):** compact/narrow viewports, overlays/pickers, permission/question modals, transcript/tool blocks, truecolor RGB roles (ANSI dump has intensity only).

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
| 79×24, 60×20, width ≥ 121, 120×50 | **TBD** | no freeze yet |

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
| Footer startup | Right-biased; `Logged in with API key` starts ~col 87; `│` at 111; `Beta` at 114 |

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
| Dynamic height | Multi-line draft **TBD** (not in freeze) |
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
| Action rows (4) | Label left (col 23), optional shortcut right (col 108) |
| Bottom pad | Empty row before bottom border |

### Action rows (exact labels @ freeze)

| Label | Shortcut |
|---|---|
| `New worktree` | `ctrl+w` |
| `Resume session` | `ctrl+s` |
| `Changelog` | *(none shown)* |
| `Quit` | `ctrl+q` |

Labels and shortcuts use bold labels + normal/dim shortcuts (ANSI intensity). Shortcuts are right-aligned as a column, not trailing immediately after the label.

### Logo

- Rendered as multi-line braille art (not a single emoji).
- **Harness identity substitution:** replace braille/logo glyphs and the title string with Harness logo + `Harness` (and version), **keeping the same bounding columns and row count**. Do not reflow the panel around a different logo aspect without a new capture.

---

## 7. Overlay dimensions / z-order

| Topic | Status |
|---|---|
| Overlay sizes | **TBD** — no freeze capture of palette, session picker, permission, question, help, model switcher |
| Z-order | **TBD** — expected later: modal overlays above shell; composer remains under modal dim/stack per future captures |
| Preemption / dismiss | **TBD** |

Until captured: do not invent overlay geometry from current Harness `ui_overlays*`.

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
| `◆` | Tool header marker (contract/reskin notes; **not** in startup/draft freeze) | **TBD** — capture tool states |
| `●` / `○` | Permission choice selected/unselected (contract notes; **not** in freeze) | **TBD** — capture permission surface |
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
| Background | — | **TBD** — sample `terminal.png` under pinned xterm.js pipeline (receipt fonts/DPR) |
| Foreground default | — | **TBD** |
| Accent / link / warning / success / error | — | **TBD** — require truecolor cell capture or PNG sampling with provenance |
| Border color vs text | — | **TBD** |

**Do not invent palette values.** When capturing colors, record exact resolved RGB per role into this section and cite the capture path.

Capture paths for future color lock:

```text
artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run1-startup/terminal.png
artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/run1-startup/terminal-ansi.txt
```

---

## 11. Focus / cursor rules (measured + interim)

| Rule | Spec |
|---|---|
| Startup focus owner | Composer input (caret on draft line after `❯`) |
| Cursor visibility | Shown when idle after paint; may hide during synchronized updates |
| Cursor position empty draft | Content row of composer, first editable cell after `❯ ` |
| Welcome actions | Visible labels; keyboard shortcuts shown — selection chrome **TBD** (no selected-row capture) |
| Overlay focus | **TBD** |
| Mouse | Capture enables mouse modes; hit targets **TBD** |

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
| Footer | Right status (`Logged in with API key │ Beta`) | Left shortcuts: `Enter:send  │  Shift+Tab:mode  │  Ctrl+x:shortcuts` |

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
| Footer auth wording | `Logged in with API key` | Harness-accurate auth summary | Keep right-bias and `│` segment grammar |

**Must not change for identity:** border style, composer anatomy, vertical order, welcome clear-on-type, shortcut footer grammar, padding rhythm.

---

## 14. Transcript / live shell (not in freeze; interim)

Freeze captures are startup + empty-body draft only. For live sessions the implementation contract still requires:

- Full-width transcript/body above the same bordered bottom composer (no persistent right operator sidebar as primary chrome).
- Operator facts on secondary surfaces (status dialog, details, palette, slash) — Harness seam language from `crates/harness-tui/AGENTS.md`.

### SHELL-IDLE (interim, structural)

| Region | Contract |
|--------|----------|
| Body | Full-width transcript area; empty live shell is **card-free** (no elevated Session/Harness empty-state card, no value_prop / example_prompts body copy) |
| Composer | Same bordered strip as startup: rounded `╭─╮/╰─╯`, `❯` glyph, model badge on bottom border; multi-line drafts wrap inside the border (capped at 6 content rows) |
| Queue | When `queued_prompt_count > 0`, badge may append `· queued N` |
| Disclosure | Bottom control-dock disclosure row retained (e.g. `live ctx … Ctrl+p commands`) |
| Topology | No persistent right operator sidebar as primary chrome |

Structural owners: `crates/harness-tui/tests/reference_parity_tx_shell_test.rs` (`shell_idle_*`).

### TX-USER / TX-ASSISTANT (interim, structural)

| Region | Contract |
|--------|----------|
| User rows | Flat `›` marker; no legacy left rail `┃`; no outer card chrome |
| Assistant rows | Rail-free body; footer/meta may show model/status on shell surface |
| Shared | No sharp corners / card mid-rules as primary transcript chrome |

Structural owners: `crates/harness-tui/tests/reference_parity_tx_shell_test.rs` (`tx_user_*`, `tx_assistant_*`) plus existing exact transcript tests.

Detailed tool `◆` headers, diffs, and streaming chrome remain **TBD pending reference captures**.

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

## 17. Open measurement backlog

1. Compact viewports (80×24, 100×30, …) — welcome/composer collapse rules.  
2. Truecolor RGB roles from cell grid or controlled PNG sampling.  
3. Overlay sizes, placement, dimming, z-order.  
4. Permission / question choice glyphs (`●`/`○`) and dock geometry.  
5. Transcript/tool/markdown/diff block anatomy.  
6. Multi-line composer growth.  
7. Live streaming, cancel, and scroll/follow chrome.

Until each item is captured, mark implementation rows **blocked** rather than inventing geometry.
