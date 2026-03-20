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
- Harness: `target/pty-visual-artifacts/pty_harness_tui_tool_lifecycle_220x30.png`, `pty_session_transcript_rich_shell.png`, `pty_replay_diff_tab.png`, `pty_continue_live_diff_secondary.png`

## What already matches reasonably well

Harness already mirrors several high-level Opencode decisions:

- simple tools render inline (`crates/harness-tui/src/ui_transcript.rs:444-482`, `556-570`)
- `shell.run` promotes to a block once output exists (`crates/harness-tui/src/ui_transcript.rs:483-509`)
- edit/apply flows stay in the transcript with inline diffs rather than the main `harness-tui` review surface (`crates/harness-tui/src/ui_transcript.rs:510-555` and `crates/harness-tui/src/ui.rs:334-348`)
- tool details default on and generic-tool output defaults off, like Opencode (`crates/harness-tui/src/app.rs:961-963`; Opencode `index.tsx:153-161`)

The rest of this document covers the remaining gaps.

## Confirmed TUI-backed parity gaps

### 1. Harness still lacks the finer per-item disclosure Opencode TUI uses for some tool types

Opencode TUI has targeted per-item disclosure primitives:

- `ResultsButton` toggles result bodies per tool (`inspirations/opencode/packages/web/src/components/share/part.tsx:659-679`)
- bash blocks add overflow-based expand/collapse (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1769-1819`)

Harness has only global booleans:

- `show_transcript_thinking`, `show_tool_details`, `show_generic_tool_output` (`crates/harness-tui/src/app.rs:903-905`)
- palette commands for those toggles, all without shortcuts (`crates/harness-tui/src/keybindings.rs:147-186`)

There is no per-tool or per-block expand/collapse state in `crates/harness-tui/src/ui_transcript.rs:419-617`. Harness only supports app-wide visibility switches.

### 2. Harness pending/running tool rows are less expressive than Opencode TUI

In the Opencode TUI route, active tools differ from completed ones through spinners, pending text, and limited interaction:

- `InlineTool` shows pending text or a spinner while active (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1626-1715`)
- `BlockTool` uses a spinner title while active (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1717-1762`)

Harness tool rows are static text renders. `append_opencode_inline_tool_section_lines` and `append_opencode_block_tool_section_lines` only style spans; they do not introduce spinner-like active affordances or a richer active/done distinction (`crates/harness-tui/src/ui_transcript.rs:1128-1188`).

### 3. Harness tool rows are visually simpler and less semantically rich than Opencode

Opencode uses per-tool icons in both the share UI and the TUI route:

- share icons: `part.tsx:90-125`
- TUI-specific icons/titles: `index.tsx:1863-2217`

Harness maps most tools to only a handful of ASCII markers (`→`, `✱`, `$`, `│`, `⚙`) inside `build_opencode_tool_call_section` (`crates/harness-tui/src/ui_transcript.rs:444-585`).

This is functionally acceptable, but it is not on par with Opencode’s tool-specific iconography and richer title grammar.

### 4. Harness generic-tool rendering is much less expressive than Opencode’s fallback tool rendering

Opencode fallback tools:

- preserve the full tool id (`part.tsx:724-751`)
- flatten nested input objects and arrays into `a.b[0].c` paths (`part.tsx:754-782`)
- wrap output behind a result toggle (`part.tsx:742-747`)

Harness generic tools:

- collapse the name to the last segment via `generic_tool_name` (`crates/harness-tui/src/ui_transcript.rs:620-627`)
- only surface primitive args through `tool_input_suffix` / `tool_summary_string` paths (`crates/harness-tui/src/ui_transcript.rs:571-585`, `656-659+`)
- expose output only through the global `show_generic_tool_output` flag (`crates/harness-tui/src/ui_transcript.rs:577-604`)

Result: unknown tools in harness are materially less informative than Opencode.

### 5. Harness has no transcript timestamp feature, while Opencode does

Opencode has a dedicated timestamp toggle and command surface:

- state in TUI route: `index.tsx:153-170`
- command entry: `index.tsx:583-593`

Inside `crates/harness-tui/src`, transcript timestamp handling does not exist. The only timestamp logic found is for session-history formatting, not transcript rows (`crates/harness-tui/src/app.rs:624-684`).

So harness cannot match Opencode’s timestamp-on-demand transcript behavior at all today.

### 6. Harness does not expose Opencode’s diff-wrap / diff-style controls

Opencode exposes diff behavior as a user-facing contract:

- `diff_wrap_mode` state in the TUI route (`index.tsx:159`, `2056`, `2103`)
- docs for diff rendering under `/docs/tui` (`inspirations/opencode/packages/web/src/content/docs/tui.mdx`)

Harness does render stacked diffs for narrow widths (`crates/harness-tui/src/tests.rs:219+`, `crates/harness-tui/src/ui_secondary.rs:1598-1730`), but there is no user-facing transcript diff-wrap or diff-style control in `crates/harness-tui/src/app.rs`, `src/keybindings.rs`, or `src/ui_transcript.rs`.

### 7. Harness does not surface the assistant/turn duration metadata Opencode TUI shows

Opencode TUI and adjacent transcript surfaces show timing metadata when it matters:

- task duration summaries in the TUI route (`index.tsx:1987-2005`)
- transcript timestamp/timing controls exist in the TUI route (`index.tsx:153-170`, `583-593`)

Harness `ToolCallEntry` has no start/end timestamps or duration field; it stores only sequence boundaries and summaries (`crates/harness-tui/src/app.rs:72-86`).

Because the data is absent, harness cannot currently match Opencode’s timing/timestamp behavior in the transcript.

### 8. Harness edit blocks are diff-first, but still less interactive and configurable than Opencode

Opencode edit/apply-patch blocks:

- render dedicated diff widgets (`index.tsx:2045-2070`, `2081-2144`)
- switch between split/unified based on width and config (`index.tsx:2033-2038`, `2087-2091`)
- carry diagnostics after edit/write (`index.tsx:2069`, `2141`; share `part.tsx:583-614`)

Harness edit blocks:

- render diff content when `diff_rel_path` can be loaded (`crates/harness-tui/src/ui_transcript.rs:528-539`)
- always treat edits as block surfaces (`crates/harness-tui/src/ui_transcript.rs:554`)
- do not provide per-item diff-mode interaction or disclosure

The visual result is also looser than Opencode. The current `tool_lifecycle` snapshot leaves large vertical gaps around the diff (`crates/harness-tui/tests/snapshots/tool_lifecycle.snap:8-24`), while the Opencode screenshots are denser and more structured.

### 9. Harness screenshot evidence shows lower transcript density than Opencode

Harness snapshots and PTY images visibly under-fill the screen:

- `crates/harness-tui/tests/snapshots/streamed_response.snap:2-30`
- `crates/harness-tui/tests/snapshots/tool_lifecycle.snap:2-30`
- `target/pty-visual-artifacts/pty_session_transcript_rich_shell.png`

Observed problems:

- many empty rows between transcript blocks
- sparse message stacking
- sidebars and status rows consume space without increasing transcript richness

By contrast, `inspirations/opencode-ui-images/session.png` and `session-diff.png` show a denser log-like transcript with tighter group spacing and less dead air.

### 10. Harness still ships unpolished `unknown` metadata in many transcript/shell states

Examples:

- `crates/harness-tui/tests/snapshots/streamed_response.snap:28`
- `crates/harness-tui/tests/snapshots/tool_lifecycle.snap:28`
- `crates/harness-tui/src/snapshots/harness_tui__live_empty_state_snapshot_renders_input_first_shell.snap:15`

The screenshots repeatedly show labels like `unknown · mock/model-1` or `Preset unknown · unknown/-`.

Opencode screenshots and source both lean into concrete mode/model metadata rather than placeholder-y identity rows (`session.png`, `part.tsx:145-155`, `index.tsx:1376-1400`). Even where harness is technically correct, this is not on par with Opencode polish.

### 11. Harness task/subagent transcript UX is materially behind Opencode

Opencode TUI’s `Task` tool carries live child-session context:

- task content can include the active child-tool title while running (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1975-2008`)
- completed tasks include toolcall count and duration (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:2004-2008`)
- the row is clickable for session navigation when a child session exists (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:2011-2026`)

Harness `agent.spawn` is just a static inline title:

- `build_opencode_tool_call_section` maps it to `Task {description}` with no live child-session state, count, duration, or navigation affordance (`crates/harness-tui/src/ui_transcript.rs:564-570`)

This is a direct TUI parity gap, not just polish.

### 12. Harness misses several concrete TUI metadata niceties on simple tools

Opencode TUI includes more metadata on lightweight rows than harness does today:

- `Glob` shows match counts (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1863-1872`)
- `Grep` shows match counts (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1908-1917`)
- `Read` can emit loaded-file follow-on rows (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:1877-1903`)

Harness currently renders these tools as simpler one-line titles without equivalent follow-on metadata in `crates/harness-tui/src/ui_transcript.rs:445-482`.

### 13. Harness thinking rows are still plainer than Opencode’s transcript treatment

This gap should be read narrowly. Opencode TUI also uses a global thinking toggle, so this is not a claim of completely different structure. The gap is in richness and presentation:

- Opencode exposes explicit `thinking_visibility` state in the TUI route (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/routes/session/index.tsx:153-155`)
- harness also supports show/hide thinking globally (`crates/harness-tui/src/app.rs:961-963`, `3205`, `3348-3350`)

But harness thinking remains a plain labeled text section (`crates/harness-tui/src/ui_transcript.rs:374-378`, `1060+`) and reads less like a deliberate first-class transcript element than Opencode’s overall treatment.

## Cross-repo contradictions and verification debt

### 14. The wider repo still preserves dedicated diff review surfaces

Even though `crates/harness-tui/src/ui.rs:345-347` now only renders `Events` and `Help`, the repository still contains explicit diff-surface PTY coverage in `harness-testkit`:

- `crates/harness-testkit/tests/pty_e2e.rs:1438-1501`
- `crates/harness-testkit/tests/pty_e2e.rs:1503-1696`
- `crates/harness-testkit/tests/support/visual_contracts.rs:107-123`

The shipped artifacts `target/pty-visual-artifacts/pty_replay_diff_tab.png` and `pty_continue_live_diff_secondary.png` visibly show separate diff review surfaces. That directly contradicts the inline-only diff goal and remains parity debt at the repository level.

### 15. PTY proof is weaker than the implementation in several important places

The focused `tool_lifecycle` PTY lane waits for `← Patched ...` and captures at that point (`crates/harness-tui/tests/pty_e2e.rs:1460-1472`).

That proves inline edit diffs, but does **not** visually prove:

- shell block promotion
- overflow/long-output handling
- generic-tool output blocks
- failure-state styling
- multiple stacked tool rows in a dense run

So even where the code is close, the evidence is not yet on par with the level of proof the user requested.

## Screenshot-backed visual findings

### 16. Harness still reads more like a sparse shell plus sidebar than a dense tool log

From `pty_harness_tui_tool_lifecycle_220x30.png` and `pty_session_transcript_rich_shell.png`:

- transcript rows are isolated with large empty regions
- block hierarchy is subtle to the point of under-emphasis
- the sidebar carries meaningful state, but the transcript itself feels visually under-powered

From `session.png` and `session-diff.png`:

- Opencode uses tighter vertical rhythm
- tool output blocks feel more intentional and distinct
- the transcript reads as the main product surface, not an empty canvas around a few rows

### 17. Harness diff screenshots are still visually associated with separate inspector workflows

`pty_replay_diff_tab.png` and `pty_continue_live_diff_secondary.png` are unmistakably separate diff review screens. Even as legacy evidence, they undermine parity because they continue to define part of the repo’s visual story.

## Cross-surface Opencode polish references (use as secondary evidence only)

These are real Opencode affordances, but they come from web/share UI sources rather than the local TUI route. They should inform polish goals, not be treated as direct TUI contract violations.

- `part.tsx` has richer per-tool iconography, disclosure buttons, error blocks, and diagnostic blocks than harness currently shows.
- `basic-tool.tsx` and `tool-status-title.tsx` show a more sophisticated interaction language for expandable tools, active shimmer, and title morphing.
- share surfaces also include content-specific previews and footers that go beyond the current harness transcript model.

## Bottom line

Harness is no longer far away on the **basic structural decision** level: simple tools inline, shell output promoted, edit diffs inline, no diff tab in the current `harness-tui` shell.

It is still **not on par** with Opencode in four major areas:

1. **Disclosure and interaction** — Opencode TUI has item-level overflow/disclosure behavior for important tool types; harness mostly relies on coarse global toggles.
2. **Visual fidelity and density** — harness transcript screenshots are still sparser, flatter, and less information-dense.
3. **Data richness** — harness lacks timestamps, task/subagent richness, richer generic-arg flattening, and several simple-tool metadata niceties.
4. **Repository-level consistency** — `harness-testkit` still preserves dedicated diff-review surfaces that contradict the inline-only goal.

## Suggested follow-up order

1. Remove or replace the remaining `harness-testkit` diff-surface contracts and screenshots.
2. Add per-tool disclosure state instead of relying only on global booleans.
3. Add transcript timestamp support and meaningful per-tool durations if the event model can provide them.
4. Tighten transcript spacing and block emphasis until PTY screenshots resemble Opencode density.
5. Add PTY scenarios that visibly prove shell blocks, generic-tool blocks, and failure states.
