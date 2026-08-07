# Harness and Grok Build UI Difference Catalog

This document describes the largest user-interface differences between Harness and Grok Build. It is a catalog of separate, bounded pieces of work. It is not a roadmap, execution order, progress tracker, or instruction to keep polishing the same surface indefinitely.

Each item explains:

- what the user would notice;
- what should become close or identical to Grok Build;
- where an implementation agent should look;
- what an observable finished result looks like;
- what must remain Harness-specific.

An agent can be given one item without being asked to solve the entire catalog.

## Product boundary

The intended result is **Harness with a Grok Build-shaped interaction and rendering system**, not a renamed Grok Build clone.

### Match Grok Build closely

- Chat-shell geometry, spacing, borders, transcript rhythm, scrolling, and focus behavior.
- Default dark-theme color roles.
- Tool, thinking, streaming, completion, failure, and recovery choreography.
- Animation timing, redraw discipline, input responsiveness, and rendering fluidity.
- Diff presentation, transcript grouping, folding, and progressive disclosure.
- Responsive behavior at the terminal sizes covered by the reference evidence.

### Keep Harness-specific

- Harness logo art, the Harness name, version, provider/model names, authentication wording, and product terminology.
- Harness event-sourced coordinator, permission-before-execution rule, replay purity, redaction, tool identities, and session model.
- Harness actions and shortcuts when no exact Grok action exists.
- Replay remains read-only.

### Do not add

- Grok logos, product copy, URLs, or trademarked identity assets.
- Buttons, menu rows, status labels, empty panels, or placeholder cards for capabilities Harness does not implement.
- Fake success states for workspace hub, remote MCP OAuth, marketplace installation, browser OIDC, remote sharing, or any other unavailable backend.
- Decorative animation that does not communicate a state change.
- A copied Grok architecture, source file, test fixture, or identifier.

If Grok Build shows an action for which Harness has no real equivalent, omit the action and preserve the surrounding layout as cleanly as possible. Do not fill the space with “coming soon,” “unavailable,” or a dead control.

## Evidence and freshness

The checked-in executable comparison is pinned to Grok Build `0.1.220-alpha.4` at revision `c1b5909ec707c069f1d21a93917af044e71da0d7`. The currently synced readable reference source records revision `6372e41d828b8a6ee82c29e01a69e27ec895cca9` in `inspirations/grok-build/SOURCE_REV`.

Use the pinned captures as the authority for exact visible appearance. Use the readable reference source to understand behavior and implementation mechanics. If the newer readable source disagrees with the pinned executable, record the mismatch and follow the pinned visible behavior until the reference contract is intentionally refreshed.

Relevant evidence owners:

- `crates/harness-tui/DESIGN.md`
- `docs/reference/tui-reference-parity-manifest.v1.json`
- `docs/reference/grok-build-parity-loop-contract.md` for historical context only; its autonomous parity loop is retired
- `inspirations/grok-build/crates/codegen/xai-grok-pager/`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/`

The current manifest still marks the visual parity rows as incomplete. Existing structural tests and snapshots are useful safeguards, but they are not proof that Harness already looks or moves like Grok Build.

## Existing Harness foundations to reuse

These areas already contain useful structure and should normally be refined rather than replaced blindly:

- Theme families and semantic roles: `crates/harness-tui/src/theme.rs` and `crates/harness-tui/src/theme_system/`
- Ordered transcript sections: `crates/harness-tui/src/ui_transcript_sections.rs`
- Transcript rendering: `crates/harness-tui/src/ui_transcript*.rs`
- Structured diffs: `crates/harness-tui/src/ui_diff_model.rs`, `crates/harness-tui/src/ui_diff_render.rs`, and `crates/harness-tui/src/ui_tool_diffs.rs`
- Follow, anchors, and scroll transitions: `crates/harness-tui/src/transcript_scroll/`
- Frame scheduling: `crates/harness-tui/src/scheduling/`
- Transcript caches and measured layout: `crates/harness-tui/src/app/transcript_cache.rs`, `crates/harness-tui/src/app/transcript_state.rs`, and `crates/harness-tui/src/ui_transcript_layout.rs`

---

## UI-01: Make the default Harness theme use the full GrokNight role system

### What this changes for the user

The default interface will have the same visual weight and contrast relationships as Grok Build: background, raised surfaces, primary and secondary text, borders, thinking color, tool states, diffs, questions, markdown, and scrollbars will feel like one coherent theme instead of a partial Grok-like shell over Harness colors.

### Current difference

`ThemeFamily::HarnessChat` is already the default, but `Theme::harness_chat()` starts from `Theme::harness_dark()` and overrides only part of that palette. Several roles still inherit Harness values. For example, the shell background is aligned to `#141414`, while primary text, accents, lifecycle colors, markdown colors, borders, and some status colors still come from the older Harness theme. The exact Grok diff roles exist under `Theme::GROK_TERMINAL_COLORS`, but not every visible semantic role is bound to the reference palette.

### Reference behavior

Grok Build's default is GrokNight. Important anchors include:

- base background `#141414`;
- elevated/code surface `#1c1c1c`;
- highlighted surface and scrollbar thumb `#242424`;
- primary text `#e1e1e1`;
- secondary gray `#6c6c6c`;
- active composer border `#505058`;
- thinking/running violet `#bb9af7`;
- blue `#7aa2f7`, green `#9ece6a`, yellow `#e0af68`, red `#f7768e`, orange `#ff9e64`, and cyan `#7dcfff` for their defined semantic roles;
- diff insertion background `#063806` and deletion background `#420e14`.

### Harness files

- `crates/harness-tui/src/theme.rs`
- `crates/harness-tui/src/theme_system/bindings.rs`
- `crates/harness-tui/src/theme_system/palette.rs`
- `crates/harness-tui/src/theme_system/focus.rs`
- `crates/harness-tui/src/theme_system/family.rs`
- `crates/harness-tui/tests/theme_system_test.rs`
- `crates/harness-tui/tests/theme_family_test.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/groknight.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/md_style.rs`

### Done state

- The default `harness-chat` theme has a documented one-to-one mapping for every visible GrokNight role used by the chat shell.
- No default chat-shell role silently falls back to a different HarnessDark value.
- Startup, live transcript, tool running/success/failure, thinking, permission/question, markdown, selection, scrollbar, and diff captures use the expected RGB values in truecolor mode.
- ANSI/limited-color fallbacks remain readable and deterministic rather than pretending to be exact RGB.
- Harness logos and text remain Harness-branded.

---

## UI-02: Make the startup and empty-state shell reference-shaped without fake actions

### What this changes for the user

The first screen will look deliberate and consistent with the later chat shell. Typing will smoothly move from the welcome state into an empty transcript instead of leaving welcome content behind.

### Current difference

The repository's current design contract records that the older centered, compose-first Harness welcome was not equivalent to Grok Build. Grok Build uses a bordered welcome region at larger sizes, a bordered composer, top breadcrumb context, and a type-to-dismiss transition. Compact layouts collapse the welcome panel but retain real actions and the composer border.

### Reference behavior

- At the primary `120x32` startup size, the order is breadcrumb, optional warning, bordered welcome, body gap, bordered composer, footer.
- The first typed grapheme removes the welcome and warning while preserving breadcrumb, composer, draft, and shortcut footer.
- At compact sizes, the large welcome box may collapse into unboxed content, but the composer remains bordered.

### Harness files

- `crates/harness-tui/src/ui.rs`
- `crates/harness-tui/src/ui_chrome.rs`
- `crates/harness-tui/src/ui_startup_logo.rs`
- `crates/harness-tui/src/welcome_surface/`
- `crates/harness-tui/src/layout.rs`
- `crates/harness-tui/src/theme.rs`
- `crates/harness-tui/DESIGN.md`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/welcome/`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/prompt_widget/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`

### Done state

- Larger startup viewports use the measured shell order and spacing from `crates/harness-tui/DESIGN.md`.
- Harness logo art and title occupy the identity region; no Grok identity remains.
- Only real Harness actions are shown. Missing Grok capabilities do not produce placeholders or dead rows.
- The first typed grapheme clears welcome-only content without moving or rebuilding the composer incorrectly.
- The compact startup surface degrades intentionally and never unboxes the composer merely to save space.

---

## UI-03: Align the live chat shell, composer, and footer geometry

### What this changes for the user

Every live state will feel like the same stable application. The transcript will occupy the full width above a consistent bottom composer, and status changes will not make the screen jump.

### Current difference

Harness has the intended transcript-first topology, but its presentation still contains mixed shell grammars: some surfaces use Harness cards, rails, or legacy spacing while the reference uses a flat transcript and a rounded three-row composer. Footer vocabulary and alignment also vary between states.

### Reference behavior

- Full-width transcript above a bottom composer.
- No persistent right-hand operator sidebar in the primary chat shell.
- Single-line composer is exactly three terminal rows: top border, content row, bottom border.
- `❯` appears one cell inside the left border; the model badge is embedded in the bottom border.
- The composer grows for wrapped drafts but caps its content height.
- Footer shortcuts remain left-aligned in live/draft states.

### Harness files

- `crates/harness-tui/src/ui.rs`
- `crates/harness-tui/src/ui_chrome.rs`
- `crates/harness-tui/src/ui_composer.rs`
- `crates/harness-tui/src/layout.rs`
- `crates/harness-tui/src/theme.rs`
- `crates/harness-tui/tests/shell_topology_contract_test.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/prompt_widget/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`

### Done state

- Idle, draft, streaming, tool, permission, question, cancelled, failed, recovered, and completed states keep identical composer placement and width for the same viewport.
- A single-line composer is three rows; wrapped drafts grow inside the border and stop at the agreed cap.
- The model badge, queue count, and shell-mode labels fit the border without breaking corners or shifting the cursor.
- Operator details remain available through secondary surfaces, not a permanent sidebar.
- Empty live chat has no card-style marketing copy or placeholder panel.

---

## UI-04: Match user-message rendering and new-turn page-flip behavior

### What this changes for the user

When a message is sent, it becomes the starting point of the new turn. Older content moves above the visible area but remains available by scrolling upward. The user gets a clean visual reset without losing history.

### Current difference

Harness has structured user-message sections and generic follow/anchor primitives, but the current `FollowState` only models following versus detached distance from the bottom. It does not expose Grok Build's one-shot “preserve the submitted prompt at the top” state in the current chat path.

The user-message renderer also needs to remain flat and rail-free. The measured reference uses a simple user marker and a selected/active surface, not an outer message card.

### Reference behavior

- The submitted user message is positioned at the top of the viewport.
- Older content remains in scrollback above it.
- New assistant/tool content fills the area below the submitted message.
- Once new content outgrows the preserved view, normal bottom-following resumes.
- User rows are flat and visually distinct without becoming chat bubbles or cards.

### Harness files

- `crates/harness-tui/src/ui_transcript_sections.rs`
- `crates/harness-tui/src/ui_transcript_render.rs`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/src/transcript_scroll/follow.rs`
- `crates/harness-tui/src/transcript_scroll/anchors.rs`
- `crates/harness-tui/src/transcript_integration/`
- `crates/harness-tui/src/app/composer.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/user.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/nav.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/dispatch/prompt.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/page_flip_on_send_pty.rs`

### Done state

- Sending a message places that user row at the viewport top when enough history exists.
- The immediately preceding turn is no longer visible but is reachable with upward scroll.
- Streaming begins below the pinned user row without a jump or blank frame.
- The preserve state is consumed only when content overflow requires normal bottom following.
- User rows use the reference marker, spacing, surface, wrapping, and long-message collapse behavior while keeping Harness content.

---

## UI-05: Make follow, manual scrolling, resize anchoring, and return-to-live exact

### What this changes for the user

The transcript will stay where the user expects. Reading older output will not be interrupted by new tokens, resizing the terminal will not lose the current place, and returning to live output will be obvious and reliable.

### Current difference

Harness already has `FollowState`, logical anchors, scroll transitions, and tests for detaching and reattaching. The remaining parity concern is the complete interaction: wheel behavior during streaming, bottom overscroll re-engagement, page-flip preservation, selection stability, resize, fold/unfold, and the visible return-to-live affordance must all cooperate in the actual shell.

### Reference behavior

- Any real upward/manual scroll detaches follow.
- New content does not drag a detached viewport downward.
- Reaching or deliberately jumping to the bottom re-enables follow.
- Width changes preserve the logical top block and within-block position while detached.
- Fold/unfold and tool expansion preserve the viewed content and selection anchor.
- A live-output affordance appears when detached from an active stream.

### Harness files

- `crates/harness-tui/src/transcript_scroll/follow.rs`
- `crates/harness-tui/src/transcript_scroll/anchors.rs`
- `crates/harness-tui/src/transcript_scroll/easing.rs`
- `crates/harness-tui/src/transcript_scroll/scrollbar.rs`
- `crates/harness-tui/src/transcript_integration/`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/tests/transcript_scroll_test.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/nav.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/layout.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/wheel_scrolls_viewport_during_streaming_turn.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/resize_preserves_scroll_position.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/wheel_overscroll_at_bottom_reengages_follow_mid_stream.rs`

### Done state

- Manual upward scroll during streaming detaches immediately and remains stable as tokens arrive.
- A bottom jump and the defined bottom-overscroll gesture reattach follow without requiring repeated input.
- Resize, diff disclosure, tool disclosure, and long-line rewrap preserve a logical anchor while detached.
- Selection remains attached to the same transcript content through those layout changes.
- The return-to-live control appears only when it is meaningful and disappears after reattachment.

---

## UI-06: Make assistant streaming text settle like Grok Build

### What this changes for the user

Assistant output will grow smoothly without flicker, duplicated spacing, unstable wrapping, or visual resets when markdown becomes complete.

### Current difference

Harness correctly preserves event order between reasoning, assistant text, tools, and errors. Its assistant body still needs parity-level streaming behavior: stable markdown wrapping, minimal invalidation, rail-free presentation, predictable spacing between interleaved parts, and a clean transition from partial to finished content.

### Reference behavior

- Assistant text is a full-width, rail-free markdown body.
- Chunks append in place rather than remounting the turn.
- Incomplete markdown is handled by a streaming renderer and is finalized once the message completes.
- Tool and reasoning blocks remain in event order relative to text.
- Diagrams and richer finished-only interpretation do not cause mid-stream layout thrashing.

### Harness files

- `crates/harness-tui/src/ui_transcript_sections.rs`
- `crates/harness-tui/src/ui_transcript_render.rs`
- `crates/harness-tui/src/ui_markdown.rs`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/src/app/transcript_state.rs`
- `crates/harness-tui/src/app/transcript_cache.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/agent.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/markdown_content.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/acp/tracker.rs`

### Done state

- Incremental text appends without clearing or redrawing unrelated transcript sections.
- Wrapping before and after completion is stable unless final markdown semantics genuinely change it.
- No assistant card, outer border, or permanent rail is introduced.
- Reasoning, tool calls, assistant text, and errors appear in source event order.
- A long streaming response remains responsive to typing, scrolling, cancellation, and resize.

---

## UI-07: Match thinking and reasoning-trace presentation

### What this changes for the user

Reasoning will be recognizable but quiet. A running trace will communicate activity, and a finished trace will collapse into a short duration summary instead of occupying the chat permanently.

### Current difference

Harness builds reasoning as a labeled text section and can hide/show it, but the presentation is less stateful than Grok Build's dedicated thinking block. The reference distinguishes running, truncated, expanded, and finished states and gives the running state its own motion and elapsed-time treatment.

### Reference behavior

- Running header reads `Thinking…`.
- Finished collapsed header reads `Thought for Xs` with a real elapsed duration.
- Running content is visible in a truncated form.
- Finished content defaults to collapsed and can be expanded deliberately.
- The body is visually de-emphasized.
- An animated one-cell accent communicates active reasoning; finished state is static.
- Empty reasoning blocks disappear rather than producing `Thought for 0.0s`.

### Harness files

- `crates/harness-tui/src/ui_transcript_sections.rs`
- `crates/harness-tui/src/ui_transcript_render.rs`
- `crates/harness-tui/src/ui_transcript_types.rs`
- `crates/harness-tui/src/app/transcript_state.rs`
- `crates/harness-tui/src/scheduling/`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/thinking.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/wrappers/entry_renderer.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`

### Done state

- Running, truncated, expanded, and finished reasoning states are visually and behaviorally distinct.
- The elapsed label is monotonic while running and stable after completion.
- Finished reasoning collapses without changing the anchor of content below it.
- Empty traces do not leave a header behind.
- Reduced-motion mode replaces the traveling effect with a static but clear active state.

---

## UI-08: Simplify tool rows, grouping, and disclosure

### What this changes for the user

Tool execution will be easier to scan. A long turn will read as a concise sequence of actions instead of a stack of verbose cards and raw output blocks.

### Current difference

Harness has structured tool-call sections and specialized renderers, but several paths still expose card shells, extra details, or inconsistent expansion. Grok Build uses flat rows, compact grouped summaries, and explicit disclosure. It does not show every result body by default.

### Reference behavior

- Individual tool rows use a flat `◆` marker.
- Compatible consecutive tool rows may collapse into a verb summary, such as several reads or searches.
- Command groups can use a `◈` summary row.
- Collapsed is the normal finished presentation.
- Expansion is explicit and preserves scroll position.
- Failed collapsed rows communicate failure through color/accent without adding a redundant `command failed` paragraph.
- Tool output appears only when it is useful and supported by the actual tool result.

### Harness files

- `crates/harness-tui/src/ui_transcript_tool_sections.rs`
- `crates/harness-tui/src/ui_transcript_tool_render.rs`
- `crates/harness-tui/src/ui_transcript_types.rs`
- `crates/harness-tui/src/app/tool_call.rs`
- `crates/harness-tui/src/theme.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/groups.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/verb_group.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/block.rs`

### Done state

- Common read/search/list/skill runs produce concise, correct group summaries.
- Tool rows are flat and card-free in collapsed mode.
- Default disclosure rules are consistent by tool kind and lifecycle state.
- Expanding and collapsing a row does not move the user's logical viewport anchor.
- Errors remain visible without duplicate explanatory lines.
- Generic raw output is not rendered when a structured, cleaner representation exists.

---

## UI-09: Match tool-call animation and state choreography

### What this changes for the user

Running tools will feel active and understandable rather than merely changing text. Success, failure, waiting for permission, and completion will each have a clean visual transition.

### Current difference

Harness has an animation phase, lifecycle state types, a frame scheduler, and tool status colors. The active tool experience is still not a full match for Grok Build's one-cell traveling accent, pending-user-input treatment, finish flash, and settled state discipline.

### Reference behavior

- Only active/running entries animate.
- A one-cell traveling wave moves through the active accent rail at the measured cadence.
- Pending user input freezes the running wave into a clear paused state and uses a distinct pulse/selection cue.
- Success and failure settle to static semantic colors.
- A short finish flash confirms the transition without leaving permanent motion.
- Off-screen running rows do not force unnecessary animation work.
- Idle UI requests no animation redraws.

### Harness files

- `crates/harness-tui/src/app/transcript_state.rs`
- `crates/harness-tui/src/ui_transcript_render.rs`
- `crates/harness-tui/src/ui_transcript_tool_render.rs`
- `crates/harness-tui/src/scheduling/scheduler.rs`
- `crates/harness-tui/src/transcript_integration/lifecycle.rs`
- `crates/harness-tui/src/design_contract/`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/wrappers/entry_renderer.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/types.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`

### Done state

- Frame traces show the active accent advancing at the intended cadence while finished siblings remain static.
- Permission/question waiting is visually different from ordinary execution.
- Success and failure transition once and settle; no perpetual animation remains.
- Reduced-motion mode renders one deterministic state-transition frame and then settles.
- With no active turn, tool, toast, or transition, the scheduler produces zero idle redraws.
- Motion never delays keyboard, mouse, cancellation, or streamed output handling.

---

## UI-10: Bring edit and diff rendering to Grok Build's disclosure model

### What this changes for the user

File changes will be immediately understandable. The transcript will stay compact, while expanding an edit will reveal a readable, syntax-aware diff with proper line numbers and context.

### Current difference

Harness already parses unified diffs into a structured model, supports per-file sections for multi-file patches, and uses semantic diff colors. Its transcript behavior still differs in summary format, default folding, line-number/gutter presentation, hunk separation, same-file edit coalescing, progressive highlighting, and error treatment. Harness also has a `Diff preview unavailable` fallback that should not become a normal visible placeholder state.

### Reference behavior

- Collapsed row: `Edit <basename> +N/-M` or an equivalent creation label.
- Expanded body: line-number gutter, insertion/deletion backgrounds, syntax-aware content, wrapped long lines, and `… N unchanged lines` between separated hunks.
- Multiple safe edits to the same file can merge into one row with combined diff statistics.
- Multi-file patches retain clear per-file disclosure.
- The first paint can use hunk-local highlighting and upgrade in place to full-file highlighting without changing text.
- Error state collapses to a red/error marker with concise muted error text.
- Copying an edit yields a valid unified patch.
- No per-character diff typing animation is expected.

### Harness files

- `crates/harness-tui/src/ui_diff_model.rs`
- `crates/harness-tui/src/ui_diff_render.rs`
- `crates/harness-tui/src/ui_tool_diffs.rs`
- `crates/harness-tui/src/ui_transcript_tool_sections.rs`
- `crates/harness-tui/src/app/tool_call.rs`
- `crates/harness-tui/src/theme_system/bindings.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/diff.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/tool/edit.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/edit_highlight_worker.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/acp/tracker.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`

### Done state

- A successful single-file edit initially renders as one concise collapsed row with correct `+N/-M` counts.
- Expansion reveals stable line numbers, change markers, colors, syntax styling, wrapping, and unchanged-line gap markers.
- Multi-file apply-patch output is organized by real files and contains no fake or empty section.
- Missing optional preview data degrades to a truthful file summary, not a prominent placeholder message.
- Same-file coalescing never merges unsafe or failed edits.
- Highlight upgrades change style in place without changing text, selection, or scroll anchor.
- Error, copy, expand, collapse, and narrow-width behavior are covered by rendered-state tests.

---

## UI-11: Match the complete turn lifecycle without moving the shell

### What this changes for the user

The user will always know whether Harness is thinking, responding, using tools, waiting, cancelled, failed, recovering, or done. These changes will happen in place instead of creating temporary cards or shifting the composer.

### Current difference

Harness has rich lifecycle data and ordered transcript parts, but visible lifecycle treatment is distributed across headers, footers, tool rows, toasts, and shell state. Grok Build presents these as one coherent progression with stable geometry.

### Reference behavior

- Streaming state shows active motion, elapsed information, and a cancellation affordance.
- Tool execution replaces or complements responding state without duplicating progress UI.
- Cancellation removes active motion and leaves a clear terminal state.
- Failure uses error semantics without adding a permanent error card.
- Recovery clears stale failure chrome before normal activity resumes.
- Completion settles durations and content, then removes spinner and cancel hints.

### Harness files

- `crates/harness-tui/src/ui_transcript_sections.rs`
- `crates/harness-tui/src/ui_transcript_render.rs`
- `crates/harness-tui/src/ui_chrome.rs`
- `crates/harness-tui/src/app/lifecycle.rs`
- `crates/harness-tui/src/app/transcript_state.rs`
- `crates/harness-tui/src/transcript_integration/lifecycle.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/acp/tracker.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/blocks/session_event.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs`

### Done state

- One recorded turn can visibly and correctly pass through streaming, tool running, success/failure, and completion without moving the composer.
- Cancelled, failed, recovered, and completed captures have distinct semantics and no stale active indicator.
- Elapsed labels are monotonic while active and stable after completion.
- Status text is not duplicated across header, body, footer, and toast.
- Replay derives the same settled transcript from events without replaying animation side effects.

---

## UI-12: Make permission and question interactions feel attached to the active turn

### What this changes for the user

Requests for permission or clarification will feel like a paused part of the conversation, not a separate application screen. The transcript and draft will remain visible and stable.

### Current difference

Harness already has permission and question surfaces and preserves coordinator authority. The remaining difference is visual and interaction continuity: overlay dimensions, choice styling, active-tool pause treatment, focus restoration, and bottom-shell stability need to match the reference behavior.

### Reference behavior

- The active tool visibly changes from running to waiting for the user.
- Selected and unselected choices use the measured `●` and `○` grammar and semantic colors.
- The transcript remains visible behind or alongside the prompt as defined by the reference surface.
- The composer, current draft, and scroll anchor are not destroyed.
- Dismissal or completion restores the previous focus owner.

### Harness files

- `crates/harness-tui/src/ui_overlays/`
- `crates/harness-tui/src/ui_permission_dock.rs`
- `crates/harness-tui/src/ui_overlays/permission_modal.rs`
- `crates/harness-tui/src/app/permissions.rs`
- `crates/harness-tui/src/app/question_prompt.rs`
- `crates/harness-tui/src/overlay.rs`
- `crates/harness-tui/src/layout.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/wrappers/entry_renderer.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/selection.rs`

### Done state

- Permission and question states preserve transcript content, draft text, scroll position, and composer geometry.
- Keyboard and mouse selection use deterministic hit regions and visible focus.
- Completing, denying, or dismissing the surface restores the correct prior focus.
- Tool motion pauses/changes while waiting and resumes or settles after the decision.
- No unsupported option or backend is shown as a disabled placeholder.

---

## UI-13: Match redraw cadence, scroll pacing, and long-transcript performance

### What this changes for the user

Typing, scrolling, streaming, and animations will stay responsive at the same time. Long sessions will not make the interface sluggish, and fast wheel input will not produce ghost frames or sudden jumps.

### Current difference

Harness already has a `FrameScheduler`, redraw coalescing, transcript render keys, bounded caches, measured layouts, reduced-motion support, and 16 ms drag autoscroll. Grok Build additionally demonstrates an integrated pacing strategy: independent animation and scroll clocks, bounded event draining, capped movement per flush, visible-only exact measurement, and explicit no-ghost-frame tests.

### Reference behavior

- Active visual animation runs near the measured 30 fps cadence.
- Scroll dispatch uses its own roughly 16 ms clock and is not slowed by animation fps.
- Input is serviced during provider streaming.
- A single flush cannot teleport more than a bounded portion of the viewport.
- Cadence-suppressed input does not cause empty/duplicate paints.
- Visible rows receive exact layout; off-screen history uses cheaper estimates/caches.
- Trailing streaming updates avoid rebuilding all history.
- Settled idle state produces no redraws.

### Harness files

- `crates/harness-tui/src/scheduling/scheduler.rs`
- `crates/harness-tui/src/scheduling/coalesce.rs`
- `crates/harness-tui/src/app/transcript_cache.rs`
- `crates/harness-tui/src/app/transcript_state.rs`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/src/transcript_integration/cache.rs`
- `crates/harness-tui/src/transcript_scroll/autoscroll.rs`
- `crates/harness-tui/src/runtime.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/event_loop.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/input/mouse.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/input/scroll_log.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/display_refresh_startup.rs`

### Done state

- Streaming plus continuous wheel input remains responsive and deterministic.
- A wheel flood produces no more rendered frames than meaningful dispatched movement/state changes.
- Animation work does not starve flush/input deadlines.
- Layout cost for appending a token or tool update does not grow linearly with the entire transcript.
- Large transcript, long-line, CJK, diff, resize, and tool-expansion stress cases stay within the repository's frame and memory budgets.
- Idle state produces zero scheduled animation redraws.

---

## UI-14: Make responsive and compact terminals use the same hierarchy

### What this changes for the user

Harness will remain recognizable and usable from a small terminal to a wide desktop. Compact mode will remove low-value decoration before it damages the composer or transcript.

### Current difference

Harness has breakpoint and topology tests, but the visual contract remains incomplete across all reference sizes. The main risk is that individual renderers make local width decisions that produce inconsistent borders, clipped metadata, or different hierarchy at each size.

### Reference behavior

The checked-in comparison set covers `120x50`, `120x40`, `100x30`, `80x24`, `79x24`, `60x20`, and `140x40`. The composer remains the stable bottom anchor. Welcome, metadata, tool detail, and overlay content collapse in a defined order.

### Harness files

- `crates/harness-tui/src/layout.rs`
- `crates/harness-tui/src/theme.rs`
- `crates/harness-tui/src/ui.rs`
- `crates/harness-tui/src/ui_transcript_layout.rs`
- `crates/harness-tui/src/ui_overlays/`
- `crates/harness-tui/tests/shell_topology_contract_test.rs`
- `crates/harness-tui/tests/reference_parity_*_test.rs`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/`

### Done state

- All seven reference viewport sizes render without clipped borders, hidden cursor, overlapping footer, or inaccessible controls.
- The composer remains bordered and usable at compact sizes.
- Long paths, model labels, timestamps, diff gutters, CJK text, and overlay choices have explicit truncation or wrapping behavior.
- Compact mode removes secondary information before primary interaction controls.
- The same lifecycle state remains recognizable at every tested viewport.

---

## UI-15: Define exact terminal-capability and reduced-motion fallbacks

### What this changes for the user

Users on terminals without full Unicode, truecolor, or smooth animation support will get a deliberate, readable version of the same interface rather than broken glyphs or misleading claims of visual parity.

### Current difference

Harness has theme families, capability negotiation, a real reduced-motion scheduler path, and some fallback glyph handling. The parity target needs a single explicit fallback contract connecting those systems to every chat-shell component.

### Reference behavior

Grok Build quantizes its theme for terminal color support, substitutes legacy-safe glyphs, can lower redraw cadence, and avoids demanding animation ticks for settled or off-screen content. Its current readable source does not expose an OS-level `prefers-reduced-motion` equivalent, so Harness should retain its stronger explicit reduced-motion behavior rather than regress.

### Harness files

- `crates/harness-tui/src/theme_system/`
- `crates/harness-tui/src/theme.rs`
- `crates/harness-tui/src/scheduling/scheduler.rs`
- `crates/harness-tui/src/capability_matrix/`
- `crates/harness-tui/src/terminal/`
- `crates/harness-tui/src/runtime_integration.rs`
- `crates/harness-testkit/`

### Grok reference files

- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/color_support.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/mod.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/glyphs.rs`
- `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/display_refresh_startup.rs`

### Done state

- Truecolor, 256-color, ANSI-16, reduced-motion, and legacy-glyph scenarios have explicit expected cells and styles.
- Unsupported glyphs are replaced with stable semantic alternatives rather than missing boxes.
- Reduced motion preserves every status distinction while removing continuous movement.
- Color reduction preserves contrast and meaning even when exact RGB parity is impossible.
- Evidence clearly labels capability-reduced output and never presents it as pixel-identical truecolor parity.

---

## What “close enough” means for non-chat surfaces

The user's strict one-to-one target is the chat shell and the mechanics that make it feel fluid: transcript, composer, tools, diffs, thinking, lifecycle, scrolling, focus, overlays attached to the conversation, frame scheduling, and performance.

Other surfaces do not need to become exact Grok copies merely for visual completeness. Session navigation, model switching, status/details views, dashboard-like operator views, and future features may use the same theme and interaction principles while remaining Harness-native. They should only receive a dedicated difference item when both products have a real, user-reachable equivalent.

This boundary is important: it prevents another endless whole-product parity loop while still setting a strict, observable standard for the part of Harness the user spends most of their time using.
