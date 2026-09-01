# Wave 1 opening harvest — partial journal

## Returned nodes

- `shell-inventory`
- `shell-behavior`
- `navigation-inventory`
- `navigation-behavior`
- `transcript-inventory`
- `transcript-behavior`
- `rich-content-inventory`
- `rich-content-behavior`
- `transcript-parity-gap`
- `rich-content-parity-gap`
- `composer-inventory`
- `composer-behavior`
- `commands-inventory`
- `tools-tasks-inventory`
- `overlays-inventory`
- `shell-parity-gap`
- `navigation-parity-gap`
- `composer-parity-gap`
- `commands-behavior`
- `commands-parity-gap`
- `tools-tasks-behavior`
- `tools-tasks-parity-gap`
- `overlays-behavior`
- `theme-inventory`
- `theme-behavior`
- `motion-inventory`
- `motion-behavior`
- `responsive-inventory`
- `accessibility-inventory`
- `overlays-parity-gap`
- `theme-parity-gap`
- `motion-parity-gap`
- `responsive-behavior`
- `responsive-parity-gap`
- `performance-inventory`
- `performance-behavior`
- `states-inventory`
- `accessibility-behavior`
- `accessibility-parity-gap`
- `performance-parity-gap`
- `states-parity-gap`
- `branding-inventory`
- `verification-inventory`
- `states-behavior`
- `branding-behavior`
- `branding-parity-gap`
- `responsive-countercheck`
- `accessibility-countercheck`
- `verification-behavior`
- `verification-parity-gap`
- `navigation-countercheck`
- `shell-countercheck`
- `transcript-countercheck`
- `rich-content-countercheck`
- `composer-countercheck`
- `overlays-countercheck`
- `tools-tasks-countercheck`
- `theme-countercheck`
- `states-countercheck`
- `commands-countercheck` (bounded retry)
- `motion-countercheck` (four-file retry)
- `performance-countercheck` (four-file retry)

## Shell and layout

- Harness keeps one persistent three-band shell and parameterizes startup/live/replay/review inside it; Grok swaps top-level full-terminal views through `ActiveView`.
- Harness owns a global z-ordered hit map and overlay stack; Grok routes input through per-pane rectangles and composes modals inside each active view before app-level debug/dashboard overlays.
- Grok's segmented top status bar and pane/mode-aware shortcut row have no full Harness equivalent; Harness instead protects run/provider/model identity and `:send/:mode/:shortcuts` footer hints.
- Harness dashboard remains an overlay and its leaf action is still unwired; Grok dashboard is a first-class full-surface view.
- Grok uses a vertical optional-pane stack with a hard scrollback floor; Harness uses transcript plus a fixed-height dock and mostly mode-exclusive secondary panes.
- Grok snapshots and restores the viewport for dashboard peeks; Harness restores focus/follow flags but not an equivalent full viewport snapshot.
- High-value leads: full-surface dashboard, scrollback floor contract, viewport snapshot lease, configurable status row.

## Navigation and wayfinding

- Harness has one `Tab::Run`, a persistent operational sidebar, run-context breadcrumb, session lineage keys, and LSP/MCP/todo status; these are Harness-specific and must remain.
- Grok has first-class dashboard routes, directory/state grouping, stable-row pin/reorder, hover peek, row-exact return breadcrumbs, turn-position wayfinding, and a percentage context bar.
- High-value leads: exact dashboard return state, directory grouping/filter grammar, context percentage bar, turn position, unified pending-action hints.

## Transcript

- Harness re-derives transcript sections from event projections; Grok mutates an ordered entry store in place.
- Grok aggregates collapsed tool runs into verb headers and truncates dense runs behind “N more”; Harness groups adjacent tool families but lacks cross-call verb aggregation.
- Harness has strong logical anchors, follow/page-flip states, streaming markdown safety, sticky user prompts, and animated status.
- High-value leads: verb-run aggregation, turn-jump overlay, sticky per-turn headers, pin-reserve stream-growth stability, measured-only incremental layout.
- Grok pins every turn prompt with minimum-height collapse, clip/push behavior, and a timestamp overlay; Harness pins only the latest user section at a fixed height.
- Harness uses a fixed three-line wheel delta; Grok carries turn-aware navigation and richer scroll indicators.

## Rich content

- Grok tables are boxed, wrapped, width-constrained, and preserve cell formatting; Harness tables are borderless/gap-separated and strip cell markdown.
- Grok recognizes citation fences shaped like `start:end:path` and resolves syntax from the path; Harness resolves only the literal fence token.
- Harness has the stronger structural diff-fence renderer and must keep it.
- Harness drops link destinations in transcript markdown even though it has OSC-8 transport support; Grok renders label plus URL with link metadata.
- Harness's still-open streaming fences are flat text; Grok incrementally syntax-highlights the open tail.
- High-value leads: rich table contract, citation-fence parser, transcript hyperlink propagation, incremental open-fence highlighter.

## Composer, commands, tools, and overlays

- Grok binds direct final-response navigation; Harness's transcript handler exposes turn navigation but no equivalent response action.
- Grok's rich tables are bordered and width-aware; Harness emits borderless unconstrained text columns.
- Grok collapses multiline pastes into atomic summary chips; Harness keeps pasted multiline text inline.
- Harness exposes stop as a clickable status-row action; Grok also gives active turns a direct `Esc` cancel path.
- Grok owns a runtime-mutable slash-command registry; Harness uses a fixed static command table.
- Grok animates turn-progress spinner frames; Harness status glyphs are static.
- Grok can restore the previous palette on `Esc`; Harness pops the current overlay without parent-snapshot chaining.
- High-value leads: response navigation, paste chips, direct cancel, dynamic command sources, animation clock integration, overlay parent snapshots.

## Shell, theme, motion, and responsiveness

- Harness dashboard is a capped centered overlay; Grok dashboard owns the full top-level terminal surface and can activate/cycle sessions in visible order.
- Harness advertises a “Multiline Input” palette action that inserts a newline rather than toggling a persistent mode; Grok `/multiline` toggles behavior.
- Harness executes a slash command on `Tab`; Grok uses `Tab` for text-only acceptance and reserves `Enter` for execution.
- Grok supports token-local command and argument completion beyond Harness's fixed whole-composer completion.
- Grok has a production task-management pane and dedicated Sleeping/TasksComplete labels; Harness's task pane is narrower and its live status vocabulary lacks those labels.
- Grok shared modal chrome owns size presets, close hit targets, tabs, and hoverable shortcuts; Harness uses a uniform plain panel.
- Harness's default palette is blue-tinted; GrokNight is neutral. Matching Grok geometry/material should not replace Harness brand accents.
- Harness wraps the composer in a rounded bordered card; Grok uses a borderless `❯` prompt with accent/model-information lines.
- Grok provides tapered post-input scroll momentum; Harness stops when wheel/gesture input stops.
- Grok styles the scrollbar by follow/detached state; Harness changes it only while dragging.
- Grok rebalances markdown table columns to the available width; Harness sizes by content and wraps overwide rows.
- Harness already has a stronger global reduced-motion contract; preserve it rather than copying Grok's per-block-only animation switches.
- Grok centralizes modal sizing, titles, tabs, shortcuts, compact adaptation, inner geometry, and close behavior; Harness centralizes only border/title/close painting.
- Harness prompt bands are much higher-contrast than GrokNight's restrained elevated surfaces.
- Harness clamps wheel input and loses high-rate tail intent; Grok keeps a bounded backlog and drains it on a 16 ms cadence.
- Harness resize only requests redraw and defers geometry state; Grok updates dependent app state immediately.
- Harness remeasures all transcript blocks during resize and every frame before culling; Grok estimates off-screen history and binary-searches the visible paint window.
- Harness exposes raw provider failures; Grok maps them to a curated actionable “headline: why. what to do.” taxonomy.
- Grok exposes a user-level mouse-reporting toggle; Harness capability-gates capture without an explicit override.
- Harness reduced-motion support is stronger and must remain.
- Harness disconnects and requires manual reopening; Grok performs automatic reconnect with transactional replay/rollback.
- Grok animates its braille wordmark; Harness ships a static single-glyph `H`. Preserve the Harness mark, but adopt motion/timing treatment rather than Grok lettering.
- Harness captures emitted frame diffs; Grok's PTY harness reconstructs a user-visible terminal screen and answers terminal queries.
- Harness classifies errors with brittle banner substrings; Grok uses typed `WireErrorType`.
- Countercheck: Harness key accent and agent colors already overlap GrokNight/TokyoNight; the gap is surface contrast/elevation, not wholesale palette replacement.
- Harness's `H`/Harness identity is correctly distinct and is a protected invariant.
- Harness defines a 16 ms resize debounce but bypasses it in the production event loop; Grok's debounce is live.
- Harness drops every non-Press key event; Grok relies on Release events for some interactions, so event-kind policy must be addressed before direct key-behavior ports.
- Harness question-card grammar/anatomy is already a near-1:1 adaptation of Grok and should be preserved.
- Harness PTY tests assert screen state but do not package durable visual-evidence bundles.
- Countercheck: the two products' “sidebar” labels name different objects. Harness's operator/details rail must not be replaced by Grok's narrow per-turn timeline; parity requires composing both roles.
- Countercheck: Harness has the stronger named/tokenized/test-pinned viewport contract; copy Grok's pane feel, not its widget-local ownership model.
- Countercheck: Grok folds tool groups at layout time; Harness summarizes adjacent tools in the transcript model. A parity implementation must choose one authoritative layer and avoid double grouping.
- Countercheck confirmed the inverse table-cell guarantee: Harness strips inline formatting while Grok explicitly tests preservation.
- Countercheck: `InterjectPrompt` is not equivalent. Harness cancels the active turn then submits; Grok interjects without cancellation. Preserve both concepts as separate actions.
- Countercheck: Harness's command palette already has stronger weighted Skim fuzzy scoring/category ordering; Grok uses unranked substring matching. Copy modal chrome, not Grok search semantics.
- Countercheck refuted the earlier “Grok-only tool aggregation” claim: Harness already has semantic verb-run aggregation and dense-run truncation.
- Countercheck found an internal theme split: Harness canonical tokens track GrokNight, but the pinned runtime theme renders a materially different palette.
- Countercheck refined reconnect: Harness core has the capability, but it has zero callers while the TUI claims “Reconnecting live state…”.
- Bounded retry: Harness uses two different completion engines (explainable tiered slash ranking plus Skim palette fuzzy matching); Grok slash completion uses one nucleo engine with scores/highlights and argument metadata. Preserve deterministic rank guarantees while unifying metadata/highlighting.
- Four-file retry verified the motion distinction: Grok treats wheel input as timed streams with gap/direction boundaries and fractional carry; Harness has a stateless flush-scoped saturating accumulator. Harness's global reduced-motion veto remains the stronger accessibility contract.
- Four-file retry verified the performance distinction: Harness measures full transcript history for exact anchors, with section/block caches; Grok exact-measures only the visible window and keeps off-screen estimates.
- Terminal caveat: Grok documents macOS Terminal block-glyph striping and compensates with matching foreground/background thumb fill; any copied scrollbar treatment must retain terminal-specific cell-box coverage.

## EXPAND leads

1. Harness transcript viewport persistence across session navigation.
2. Harness mouse interaction matrix versus Grok pane hit testing.
3. Resize debounce/FPS and rendering performance comparison.
4. Configurable status-line sanitization and elision.
5. Terminal panel versus Grok PTY support.
6. Welcome/startup/auth/session picker parity.
7. Grok active-session refresh versus Harness session history projection.
8. `/jump` and rewind navigation parity.
9. Harness unread indicator paint path.
10. Modal/help tab behavior versus Grok Extensions/Agents tabs.
11. Transcript selection parity.
12. Subagent/background/workflow block rendering.
13. Session-event and system-message blocks.
14. Compaction transcript rendering.
15. Mermaid, OSC-8, and link rendering.
16. Scrollback search versus Harness block viewer.
17. Side-by-side theme token and appearance compactness table.
18. Grok welcome/hero geometry versus Harness lifecycle startup.
19. Segmented status-bar rect cache and hover behavior.
20. Pane-aware shortcut hint generation.
21. Rich-table width/wrapping behavior under narrow terminals.
22. Citation-fence path parsing and syntax fallback.
23. Transcript hyperlink targets through OSC-8.
24. Incremental highlighting for open streaming code fences.
25. Final-response navigation distinct from turn navigation.
26. Atomic multiline paste chips and expansion behavior.
27. Active-turn `Esc` cancel versus overlay dismissal arbitration.
28. Runtime command registration and extension-defined commands.
29. Shared animation clock for progress/status glyphs.
30. Parent-palette snapshot restoration.
31. Full-surface dashboard ownership and session activation/cycling.
32. Real multiline-mode toggle and visible state.
33. Slash completion acceptance versus execution semantics.
34. Production task pane and complete status vocabulary.
35. Shared modal chrome and close/tab/shortcut primitives.
36. Neutral material ramp with protected Harness accent tokens.
37. Borderless prompt treatment and compact model information.
38. Tapered scroll coast and follow-aware scrollbar styling.
39. Width-aware rich-table column balancing.
40. Complete reusable modal-window primitive.
41. Restrained surface contrast and elevation ramp.
42. Bounded wheel-intent queue drained by animation clock.
43. Immediate resize-dependent state update.
44. O(log n) viewport paint window with estimated off-screen heights.
45. Curated provider-error taxonomy and recovery copy.
46. User override for terminal mouse reporting.
47. Automatic reconnect with replay/rollback safety.
48. Harness-logo-preserving animated startup treatment.
49. Emulator-backed terminal oracle for real screen-state assertions.
50. Typed provider-error classification instead of banner substrings.
51. Activate the existing resize debounce in production.
52. Key-event-kind policy that can support release-dependent interactions.
53. Durable PTY screenshot/transcript/metadata evidence bundles.
54. One authoritative transcript grouping/fold projection layer.
55. Separate “cancel and replace” from “interject without cancellation.”
56. Wire the existing reconnect capability to the live TUI or remove misleading status copy.
57. Unify slash/palette command metadata without losing deterministic rank guarantees.
58. Decide whether timed gesture streams improve Harness enough to justify stateful wheel semantics.
59. Prototype estimated off-screen heights behind exact-anchor regression tests.

These leads remain open until the expansion waves investigate or close them.

## Refined/refuted opening assumptions

- Refined: “replace Harness's blue palette with Grok's palette” is too broad. Core accent/agent colors already overlap; parity work should target restrained surface contrast, elevation, and component treatment while preserving Harness tokens.
- Refuted: 1:1 parity does **not** require Grok logos, wordmarks, names, or product voice. Harness identity is an explicit protected invariant.
- Refuted: Grok's timeline is not a drop-in replacement for Harness's operator sidebar; the data roles differ.
- Refined: shell parity must keep Harness's stronger viewport specification and tests while adopting Grok pane hierarchy/material behavior.
- Refined: overlay parity should preserve Harness palette search/ranking while consolidating Grok-like window geometry and controls.
- Refuted: semantic tool-run aggregation/truncation is not absent in Harness.
- Refined: theme parity is primarily a token-to-runtime-theme integration drift, not missing color tokens.
- Refined: Grok palette substring matching and Grok slash nucleo matching are different surfaces; compare like with like.
- Verified: full-history versus visible-window transcript measurement is an architectural tradeoff, not merely a missing micro-optimization.
