# Opencode parity audit for harness tool UX

## Scope and source set

This audit compares `agent-harness` tool/transcript UX against the local Opencode sources in `inspirations/opencode` and the local screenshot set in `inspirations/opencode-ui-images/`, with current harness PTY artifacts under `target/pty-visual-artifacts/`.

Primary Opencode sources reviewed:

- `inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx`
- `inspirations/opencode/packages/web/src/components/share/part.tsx`
- `inspirations/opencode/packages/ui/src/components/basic-tool.tsx`
- `inspirations/opencode/packages/ui/src/components/tool-status-title.tsx`
- `inspirations/opencode/packages/web/src/content/docs/tui.mdx`
- `inspirations/opencode/packages/web/src/content/docs/keybinds.mdx`

Primary harness sources reviewed:

- `crates/harness-tui/src/ui_transcript.rs`
- `crates/harness-tui/src/app.rs`
- `crates/harness-tui/src/keybindings.rs`
- `crates/harness-tui/src/ui.rs`
- `crates/harness-tui/tests/pty_e2e.rs`
- `crates/harness-testkit/tests/pty_e2e.rs`
- `crates/harness-testkit/tests/support/visual_contracts.rs`

Primary screenshots reviewed:

- Opencode: `inspirations/opencode-ui-images/session.png`, `session-diff.png`, `commands-window.png`, `commands-window2.png`
- Harness canonical PTY evidence: `target/pty-visual-artifacts/pty_native_tool_parity_dense.png`, `pty_native_tool_parity_task_row.png`, `pty_native_tool_parity_fetch_row.png`, `pty_opencode_sidebar_session_parity.png`
- Retired legacy artifacts, kept only as historical local output and not as current signoff proof: `target/pty-visual-artifacts/pty_replay_diff_tab.png`, `pty_continue_live_diff_secondary.png`

## What already matches reasonably well

Harness already mirrors several high-level Opencode decisions:

- simple tools render inline (`crates/harness-tui/src/ui_transcript.rs:444-482`, `556-570`)
- `shell.run` promotes to a block once output exists (`crates/harness-tui/src/ui_transcript.rs:483-509`)
- edit/apply flows stay in the transcript with inline diffs rather than the main `harness-tui` review surface (`crates/harness-tui/src/ui_transcript.rs:510-555` and `crates/harness-tui/src/ui.rs:334-348`)
- tool details default on and generic-tool output defaults off, like Opencode (`crates/harness-tui/src/app.rs:961-963`; Opencode `index.tsx:153-161`)

The rest of this document covers the remaining gaps.

## Confirmed TUI-backed parity gaps

### 1. Harness now has per-item transcript disclosure, but not the full Opencode disclosure range

Opencode TUI has targeted per-item disclosure primitives:

- `ResultsButton` toggles result bodies per tool (`inspirations/opencode/packages/web/src/components/share/part.tsx:659-679`)
- bash blocks add overflow-based expand/collapse (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1769-1819`)

Harness now has both global visibility toggles and per-tool transcript disclosure state:

- `show_transcript_thinking`, `show_tool_details`, and `show_generic_tool_output` still exist as app-wide visibility controls (`crates/harness-tui/src/app.rs:1507-1512`, `3488-3503`)
- `expanded_tool_outputs` plus `tool_output_expanded()` add per-tool disclosure state for expandable transcript rows (`crates/harness-tui/src/app.rs:1512`, `3500-3523`)
- `tool_disclosure_state()` and `disclosure_glyph()` render collapsed/expanded affordances inline for tool rows that expose richer output (`crates/harness-tui/src/ui_transcript.rs:1066-1090`, `1776-1844`, `1970-1976`)

The remaining gap is narrower than before: harness now supports per-tool expand/collapse for transcript outputs, but it still does not mirror Opencode's full set of result-button, overflow-specific, and content-specific disclosure affordances for every tool family.

### 2. Harness pending/running tool rows now distinguish state, but still trail Opencode's active-row polish

In the Opencode TUI route, active tools differ from completed ones through spinners, pending text, and limited interaction:

- `InlineTool` shows pending text or a spinner while active (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1626-1715`)
- `BlockTool` uses a spinner title while active (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1717-1762`)

Harness now renders active/completed differences more explicitly than before:

- titles append `queued`, `running`, `pending permission`, timestamps, or durations from task/tool metadata (`crates/harness-tui/src/ui_transcript.rs:1298-1347`)
- block tool rows switch to an emphasized active surface while running (`crates/harness-tui/src/ui_transcript.rs:1809-1844`)
- header color/style changes by status (`crates/harness-tui/src/ui_transcript.rs:1924-1966`)
- exact transcript tests cover running vs completed child-session/task rows (`crates/harness-tui/src/ui_transcript.rs:4055-4168`)

The remaining difference is mostly polish: Opencode still has richer spinner/title morph behavior and more interaction nuance while a tool is active.

### 3. Harness tool rows are visually simpler and less semantically rich than Opencode

Opencode uses per-tool icons in both the share UI and the TUI route:

- share icons: `part.tsx:90-125`
- TUI-specific icons/titles: `index.tsx:1863-2217`

Harness maps most tools to only a handful of ASCII markers (`→`, `✱`, `$`, `│`, `⚙`) inside `build_opencode_tool_call_section` (`crates/harness-tui/src/ui_transcript.rs:444-585`).

This is functionally acceptable, but it is not on par with Opencode’s tool-specific iconography and richer title grammar.

### 4. Harness generic-tool rendering is materially richer now, but still simpler than Opencode’s fallback rendering

Opencode fallback tools:

- preserve the full tool id (`part.tsx:724-751`)
- flatten nested input objects and arrays into `a.b[0].c` paths (`part.tsx:754-782`)
- wrap output behind a result toggle (`part.tsx:742-747`)

Harness generic tools now:

- preserve the full tool id instead of collapsing to only the last segment (`crates/harness-tui/src/ui_transcript.rs:788-799`)
- flatten nested objects and arrays into compact `a.b[0].c=value` metadata via `compact_tool_input_metadata()` (`crates/harness-tui/src/ui_transcript.rs:1139-1207`)
- expose expandable output blocks through transcript disclosure state rather than only an app-wide generic-output toggle (`crates/harness-tui/src/ui_transcript.rs:703-721`, `1066-1090`, `1264-1296`)

Result: unknown tools in harness are now substantially more informative. The remaining gap is that Opencode still has richer content-specific icons, result-button language, and fallback presentation polish.

### 5. Harness transcript timestamps and timing metadata are now landed, with narrower polish headroom

Opencode has a dedicated timestamp toggle and command surface:

- state in TUI route: `index.tsx:153-170`
- command entry: `index.tsx:583-593`

Inside `crates/harness-tui/src`, transcript rows now carry timestamp and duration metadata from the event/projection path, and the dense PTY evidence proves those rows render inline. The remaining gap is narrower than before: harness still lacks some of Opencode’s per-item disclosure affordances, but it no longer lacks transcript timing outright.

### 6. Harness does not expose Opencode’s diff-wrap / diff-style controls

Opencode exposes diff behavior as a user-facing contract:

- `diff_wrap_mode` state in the TUI route (`index.tsx:159`, `2056`, `2103`)
- docs for diff rendering under `/docs/tui` (`inspirations/opencode/packages/web/src/content/docs/tui.mdx`)

Harness does render stacked diffs for narrow widths (`crates/harness-tui/src/tests.rs:219+`, `crates/harness-tui/src/ui_secondary.rs:1598-1730`), but there is no user-facing transcript diff-wrap or diff-style control in `crates/harness-tui/src/app.rs`, `src/keybindings.rs`, or `src/ui_transcript.rs`.

### 7. Harness now surfaces assistant/task timing metadata, but still has some disclosure headroom

Opencode TUI and adjacent transcript surfaces show timing metadata when it matters:

- task duration summaries in the TUI route (`index.tsx:1987-2005`)
- transcript timestamp/timing controls exist in the TUI route (`index.tsx:153-170`, `583-593`)

Harness now projects and renders timing metadata for tool/task rows. The remaining gap is in richness and interaction polish, not in the raw presence of timing data.

### 8. Harness edit blocks are diff-first, with remaining interaction headroom rather than a missing inline contract

Opencode edit/apply-patch blocks:

- render dedicated diff widgets (`index.tsx:2045-2070`, `2081-2144`)
- switch between split/unified based on width and config (`index.tsx:2033-2038`, `2087-2091`)
- carry diagnostics after edit/write (`index.tsx:2069`, `2141`; share `part.tsx:583-614`)

Harness edit blocks:

- render diff content when `diff_rel_path` can be loaded (`crates/harness-tui/src/ui_transcript.rs:528-539`)
- always treat edits as block surfaces (`crates/harness-tui/src/ui_transcript.rs:554`)
- do not provide per-item diff-mode interaction or disclosure

The remaining gap is configurability and disclosure depth, not whether inline edit proof exists. The canonical PTY parity lane now centers on denser transcript-first evidence rather than treating older sparse diff snapshots as the current baseline story.

### 9. Canonical harness screenshot evidence now reads as dense transcript-first output

The current parity screenshots moved this area from a primary gap to a polish headroom item:

- manifest-backed PTY evidence now centers on `pty_native_tool_parity_dense.png`, `pty_native_tool_parity_task_row.png`, `pty_native_tool_parity_fetch_row.png`, and `pty_opencode_sidebar_session_parity.png`
- the current PTY evidence shows tighter message stacking, denser tool rows, and a transcript-first shell rather than the older sparse-shell baseline

The remaining delta versus Opencode is about block richness and tool-specific affordances, not about obvious dead air or under-filled canonical parity screenshots.

### 10. Placeholder `unknown` metadata is mostly retired from the shipped parity surfaces

Examples:

- `crates/harness-tui/tests/snapshots/streamed_response.snap:28`
- `crates/harness-tui/tests/snapshots/tool_lifecycle.snap:28`
- `crates/harness-tui/src/snapshots/harness_tui__live_empty_state_snapshot_renders_input_first_shell.snap:15`

The shipped parity snapshots now sanitize blank or placeholder identity rows into the existing `default` / `local` / `-` fallbacks. Remaining placeholder concerns should be treated as regression checks against new snapshots rather than as the current baseline story.

### 11. Harness task/subagent transcript UX now carries the core child-session metadata, with remaining polish headroom

Opencode TUI’s `Task` tool carries live child-session context:

- task content can include the active child-tool title while running (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1975-2008`)
- completed tasks include toolcall count and duration (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:2004-2008`)
- the row is clickable for session navigation when a child session exists (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:2011-2026`)

Harness task rows now surface child-session state, tool-call count, and duration inline, and the PTY parity lanes cover that behavior. Remaining differences are polish-level disclosure and richer affordance details rather than a missing child-session contract.

### 12. Harness now covers several simple-tool metadata niceties, with narrower polish headroom

Opencode TUI includes more metadata on lightweight rows than harness does today:

- `Glob` shows match counts (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1863-1872`)
- `Grep` shows match counts (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1908-1917`)
- `Read` can emit loaded-file follow-on rows (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1877-1903`)

Harness now covers these cases directly in the transcript layer:

- `Glob` and `Grep` append match counts via `tool_match_count_suffix()` (`crates/harness-tui/src/ui_transcript.rs:516-537`, `1244-1262`)
- `Read` can emit a loaded-file follow-on row when the payload stays lightweight (`crates/harness-tui/src/ui_transcript.rs:724-734`)

The remaining gap is narrower: Opencode still has richer per-tool iconography and some content-specific affordances beyond the current harness titles and follow-on rows.

### 13. Harness thinking rows are still plainer than Opencode’s transcript treatment

This gap should be read narrowly. Opencode TUI also uses a global thinking toggle, so this is not a claim of completely different structure. The gap is in richness and presentation:

- Opencode exposes explicit `thinking_visibility` state in the TUI route (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:153-155`)
- harness also supports show/hide thinking globally (`crates/harness-tui/src/app.rs:961-963`, `3205`, `3348-3350`)

But harness thinking remains a plain labeled text section (`crates/harness-tui/src/ui_transcript.rs:374-378`, `1060+`) and reads less like a deliberate first-class transcript element than Opencode’s overall treatment.

## Cross-repo contradictions and verification debt

### 14. Dedicated diff-review artifacts are now retired from canonical proof

Even though older local `target/pty-visual-artifacts/` runs can still contain `pty_replay_diff_tab.png` and `pty_continue_live_diff_secondary.png`, the checked-in PTY evidence contract now centers on the manifest-backed inline transcript families in `crates/harness-testkit/tests/support/visual_contracts.rs`.

That keeps the repo's shipped signoff story aligned with the current shell contract: inline transcript diffs are canonical, while the old separate diff-review frames are historical output, not active parity proof.

### 15. PTY proof now covers the highest-value inline transcript surfaces

The focused `tool_lifecycle` PTY lane still captures inline edit proof, but it is no longer the only artifact carrying parity weight. The manifest-backed dense parity lane in `crates/harness-testkit/tests/pty_e2e.rs` now provides the main screenshot proof for stacked tool rows, inline attachments, shell failure styling, and transcript-first density.

The remaining evidence gap is narrower: harness still lacks item-level disclosure and some richer metadata, but the canonical screenshots now prove the inline-first shell instead of a separate diff workflow.

## Screenshot-backed visual findings

### 16. Canonical harness parity screenshots now read as dense transcript-first tool logs

From `pty_native_tool_parity_dense.png`, `pty_native_tool_parity_task_row.png`, and `pty_native_tool_parity_fetch_row.png`:

- the transcript is the dominant surface
- stacked tool rows, attachments, shell output, and failure styling stay inline
- the screenshot reads as a dense log of work, not as a sparse shell wrapped around a sidebar

Compared with `session.png` and `session-diff.png`, harness still has room to improve block richness and per-item disclosure, but the current canonical proof now matches the transcript-first density goal.

### 17. Retired diff screenshots should not be treated as parity signoff

`pty_replay_diff_tab.png` and `pty_continue_live_diff_secondary.png` can still appear in old local artifact directories, but they are explicitly retired as signoff evidence. The repo-level visual story should now be read from the manifest-backed inline transcript families instead.

## Cross-surface Opencode polish references (use as secondary evidence only)

These are real Opencode affordances, but they come from web/share UI sources rather than the local TUI route. They should inform polish goals, not be treated as direct TUI contract violations.

- `part.tsx` has richer per-tool iconography, disclosure buttons, error blocks, and diagnostic blocks than harness currently shows.
- `basic-tool.tsx` and `tool-status-title.tsx` show a more sophisticated interaction language for expandable tools, active shimmer, and title morphing.
- share surfaces also include content-specific previews and footers that go beyond the current harness transcript model.

## Bottom line

Harness is no longer far away on the **basic structural decision** level: simple tools inline, shell output promoted, edit diffs inline, no diff tab in the current `harness-tui` shell, and canonical PTY evidence now reinforces that inline-first story.

It is still **not fully on par** with Opencode in three narrower areas:

1. **Disclosure and interaction depth**: harness now has per-tool transcript disclosure, but Opencode still has a broader set of result-button, overflow, and content-specific disclosure affordances.
2. **Metadata richness headroom**: harness now has timestamps, durations, child-session richness, flattened generic metadata, and simple-tool niceties, but some tool-specific affordances and title grammar are still behind Opencode.
3. **Visual polish headroom**: the transcript-first density is now in place, but block richness and content-specific affordances are still simpler than Opencode.

## Suggested follow-up order

1. Deepen item-level disclosure controls beyond the current expandable transcript outputs.
2. Keep enriching generic-tool and content-specific affordances where Opencode still carries more semantic polish.
3. Add more item-level disclosure controls where the richer inline data already exists.
4. Keep growing the manifest-backed PTY evidence set when new inline transcript affordances land.
