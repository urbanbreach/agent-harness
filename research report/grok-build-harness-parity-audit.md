# Harness × Grok Build visual parity audit

STATUS: final research handoff

## Executive summary

Harness is not one rewrite away from Grok Build parity. Its event model, exact transcript anchors, global reduced-motion policy, command-palette ranking, question-card grammar, operator sidebar, and branded startup mark are strengths worth keeping. The gap is the presentation and interaction layer around those strengths: Grok Build treats dashboard, transcript, composer, modal, recovery, and terminal verification as one coherent visual system.

The recommended path is ten bounded changes. The first six are parity blockers: full-surface dashboard ownership with exact return restoration; richer transcript folding and position memory; rich content and links; a truthful composer state machine; truthful reconnect/recovery states; and emulator-backed PTY proof. Four P1 changes complete the polish: command argument UX, reusable modal chrome, Harness-branded startup motion, and responsive/terminal finishing.

“1:1” here means matching Grok Build's observable spatial hierarchy, interaction semantics, motion cadence, responsive behavior, and finish. It **doesn't** mean copying the Grok name, wordmark, logo, text voice, or palette. Preserve the Harness name, logo, brand assets, accent palette, operator terminology, stronger palette ranking, exact anchor behavior, and global reduced motion.

## What 1:1 parity means

| Dimension | Required parity | Harness constraint |
|---|---|---|
| Visual | Same information hierarchy, density, alignment, modal framing, table quality, and state legibility | Preserve Harness colors, logo, names, and iconography |
| Spatial | Full-surface dashboard, stable transcript/composer geometry, predictable pane ownership, responsive minimums | Preserve Harness's named viewport contract and operator sidebar |
| Interaction | Equivalent send/cancel/interject, folding, navigation, command completion, capability-gated mouse behavior, and modal behavior | Preserve deterministic Harness command ranking and accessibility shortcuts |
| Motion | Equivalent progress rhythm, follow feedback, startup treatment, and transition smoothness | Preserve global reduced motion; don't add decorative coasting without proof |
| Polish | Typed recovery copy, terminal-safe glyphs, rich links/tables, and real-terminal evidence | No visual claim without emulator-backed PTY artifacts |

## Prioritized findings

### P0-01 Make dashboard a true full-surface mode with exact return restoration

Claims: C021, C051.

**What**: Replace the capped centered dashboard overlay with a top-level surface and restore the exact transcript position on exit.

**Why**: Grok Build changes the active view and preserves pane-owned scroll state; Harness constrains the dashboard and currently stores `transcript_anchor: None`. The result is a smaller-feeling shell and weaker return continuity.

**Where**: Harness `crates/harness-tui/src/dashboard_integration/responsive.rs:62`, `crates/harness-tui/src/app.rs:908`, and `crates/harness-tui/src/dashboard_integration/state.rs:63`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/app_view.rs:4914`, `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/actions.rs:751`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/agent_view/panes.rs:507`.

**How**: Promote dashboard to an explicit shell mode; let it own the content viewport; capture `TranscriptContentAnchor` before entry; restore that anchor and focus on exit; retain the Harness operator sidebar as a composable rail rather than replacing it.

**Verify**: Add exact-render snapshots at 80×24, 120×40, and 160×50; in a PTY, open dashboard from a mid-transcript detached position, resize, close it, and assert that the viewport returns to the same transcript anchor and `display_column`.

**Dependencies**: P0-06 PTY evidence; existing dashboard state and transcript anchor APIs.

**Risks**: Overlay hit maps and focus precedence can leak into the full-surface mode. Don't copy Grok's logo, names, or dashboard labels.

### P0-02 Add dense transcript folding and response navigation; gate pin reserve on proof

Claims: C006, C008, C016, C044, C052.

**What**: Keep one authoritative Harness fold projection, expose its existing dense-run truncation as an “N more” expand/collapse affordance, and add final-response navigation and turn-position display. Add persistent pin-reserve state only after a deterministic detached-streaming test reproduces anchor failure.

**Why**: Harness already groups adjacent verbs, so a second aggregation layer would corrupt source-of-truth semantics. Grok Build's persistent pin reserve is a candidate for retaining a detached viewport during streaming, but its necessity in Harness remains unproved.

**Where**: Harness `crates/harness-tui/src/ui_transcript_block_grammar_normalize.rs:6`, `crates/harness-tui/src/transcript_blocks/folds.rs:13`, and `crates/harness-tui/src/transcript_scroll/follow.rs:28`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/groups.rs:56`, `inspirations/grok-build/crates/codegen/xai-grok-pager/src/scrollback/state/pin_reserve.rs:10`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/actions/defaults.rs:134`.

**How**: Extend the existing fold model with a visible-count contract and toggle action; render folded-count affordances at layout time; add next/previous final-response commands. First write a detached-streaming reproduction; introduce persisted pin-reserve state only if Harness's existing exact anchors don't hold. Don't port Grok `wrap_restore.rs`, which restores terminal modes rather than scrollback.

**Verify**: Deterministic tests for 0/1/many tool groups, expand/collapse, compaction, reflow, and streaming growth while detached; PTY assertions that follow mode remains detached and newly streamed bottom content stays outside the viewport.

**Dependencies**: P0-01 anchor restoration and P0-06 PTY lane.

**Risks**: Double folding and anchor drift. Preserve Harness's exact `display_column` anchor contract.

### P0-03 Bring tables, links, and open streaming fences to parity

Claims: C013, C014, C015, C045, C053.

**What**: Preserve inline table-cell formatting, wrap boxed tables, propagate OSC-8 links through production rendering and selection, and incrementally highlight open code fences.

**Why**: Grok renders rich content as a first-class terminal document. Harness currently strips table-cell markdown and underlines link labels while dropping destinations, despite already owning an OSC-8 encoder.

**Where**: Harness `crates/harness-tui/src/ui_markdown_table.rs:73`, `crates/harness-tui/src/ui_markdown.rs:66`, `crates/harness-tui/src/ui_streaming_markdown.rs:34`, and `crates/harness-tui/src/ui_fenced_text.rs:103`; reference `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/render.rs:736`, `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/render.rs:1493`, `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/parse.rs:1113`, and `inspirations/grok-build/crates/codegen/xai-grok-markdown/src/streaming.rs:281`.

**How**: Carry structured spans through table measurement; add boxed-width wrapping; thread link targets into transcript rows plus selection and copy metadata; retain Harness's structural diff fence handling; incrementally normalize the unfinished fence tail.

**Verify**: Golden tests for CJK, emoji, nested emphasis, links in table cells, malformed/open fences, and copied rich rows. In a PTY, assert that emitted OSC-8 targets and rendered table widths are correct.

**Dependencies**: Selection-row mapping and P0-06 terminal evidence.

**Risks**: Width accounting, unsafe link targets, and streaming flicker. Sanitize external targets at the rendering boundary.

### P0-04 Make existing composer modes visible and submission semantics explicit

Claims: C017, C022, C027, C046.

**What**: Surface Harness's existing persistent multiline mode in the composer geometry and define distinct actions for send, cancel-and-replace, and non-cancelling interject.

**Why**: Harness already toggles `composer.multiline_mode`, but the state and submission intent are less legible at the input boundary. Harness's current interject cancels before submission, while Grok Build's interject doesn't cancel.

**Where**: Harness `crates/harness-tui/src/app/key_interaction.rs:145`, `crates/harness-tui/src/app/key_interaction.rs:853`, `crates/harness-tui/src/app/prompt_input.rs:21`, and `crates/harness-tui/src/ui_composer/bordered.rs:54`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/agent_view/prompt.rs:531`, `inspirations/grok-build/crates/codegen/xai-grok-pager/src/agent_view/prompt.rs:575`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/commands/multiline.rs:1`.

**How**: Keep the existing multiline boolean or migrate it into a typed mode only if more modes land; route Enter by mode; show multiline and queued-state indicators; preserve buffer history. Treat send-now promotion/requeue as unproved until targeted tests exist; don't claim loss. Require an explicit runtime contract before exposing each separately named cancel-and-replace or interject action.

**Verify**: State-machine tests for idle/running/cancelling/disconnected, empty and non-empty multiline buffers, queue order, and cancellation failure; PTY tests for valid and invalid input paths.

**Dependencies**: P0-05 runtime state taxonomy.

**Risks**: Duplicate sends and lost drafts. Don't flatten Harness's bordered brand treatment merely to copy Grok's borderless prompt.

### P0-05 Replace false reconnect copy with a real recovery state machine

Claims: C032, C034, C037, C049, C055.

**What**: Distinguish Disconnected, Reconnecting, Reloading, Recovered, and Failed; either wire a re-entrant live-update reconnect path or remove “Reconnecting…” and require explicit reopen.

**Why**: Harness retries individual provider requests but doesn't re-enter live updates without a manual reopen while the TUI claims it is reconnecting. Grok has explicit re-entry and reload transitions.

**Where**: Harness `crates/harness-tui/src/runtime.rs:734`, `crates/harness-tui/src/ui_live_turn_status.rs:62`, `crates/harness-tui/src/app/lifecycle.rs:633`, and `crates/harness-core/src/coord/agent_turn_phases.rs:460`; reference `inspirations/grok-build/crates/xai-grok-shell/src/agent/mvp_agent/agent_ops.rs:3061`, `inspirations/grok-build/crates/xai-grok-shell/src/agent/mvp_agent/agent_ops.rs:2743`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/event_loop.rs:2836`.

**How**: Introduce typed runtime recovery states; feed them from coordinator/provider events; disable Stop while disconnected; map typed failures to actionable copy; add replay/rollback only in coordinator-owned code, never in replay inspection.

**Verify**: Deterministic disconnect-before-stream, mid-stream disconnect, retry success/exhaustion, reopen, and reload tests. In a live PTY, assert each label and each action's enabled state.

**Dependencies**: Coordinator event authority and provider typed errors.

**Risks**: Side effects during replay and duplicate provider work. Preserve event-sourced invariants.

### P0-06 Establish emulator-backed PTY visual evidence as the parity oracle

Claims: C036, C041, C050, C056.

**What**: Extend the existing TUI PTY owner with terminal-emulator screen assertions, durable screenshots/transcripts, terminal metadata, and artifact provenance.

**Why**: TestBackend snapshots can't detect CSI, alternate-screen, mouse-mode, wrap, or query-response failures. Harness's parser lane doesn't forward terminal query replies; Grok reconstructs terminal state.

**Where**: Harness `crates/harness-tui/tests/pty_e2e.rs:6`, `crates/harness-tui/tests/support/pty_e2e_impl.rs:1`, and `crates/harness-tui/src/terminal/frame_output/backend.rs:51`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager-pty-harness/src/screen.rs:34`, `inspirations/grok-build/crates/codegen/xai-grok-pager-pty-harness/src/screen.rs:163`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/tests/pty_e2e/common.rs:16`.

**How**: Feed PTY bytes into an emulator capable of replies; assert cells, cursor, alternate screen, modes, and scrollback; capture before/after surfaces at canonical sizes; record command, binary hash, terminal capabilities, dimensions, and artifact hashes.

**Verify**: `RUST_TEST_THREADS=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1` and `scripts/test-lanes.sh signoff-pty`.

**Dependencies**: Testkit artifact contract.

**Risks**: Timing-flaky tests. Subscribe to exact output/state transitions; never use fixed sleeps as proof.

### P1-01 Upgrade slash completion without weakening Harness ranking

Claims: C023, C047, C048, C054.

**What**: Add `takes_args` and `args_required`, argument-phase suggestions, match highlighting, and mid-text slash detection while preserving Harness's static deterministic registry and stronger palette ranking.

**Why**: Grok slash completion is richer, but its palette substring matching is not superior to Harness's Skim-based palette. The surfaces must not be conflated.

**Where**: Harness `crates/harness-tui/src/slash.rs:18`, `crates/harness-tui/src/app/session_navigation.rs:512`, and `crates/harness-tui/src/slash/commands.rs:26`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/matcher.rs:40`, `inspirations/grok-build/crates/codegen/xai-grok-pager/src/slash/mod.rs:761`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/agent_view/prompt.rs:156`.

**How**: Extend static metadata; score/highlight slash matches; switch to argument suggestions after matching a complete command name; recognize slash commands only at input start or immediately after whitespace, excluding URL-like tokens and escaped slashes; keep Tab as text acceptance and Enter as execution. Don't port dynamic ACP replacement without a real command source and collision policy.

**Verify**: Unit tests for aliases, incomplete required args, Unicode, input-start and post-whitespace slashes, URL-like tokens, escaped slashes, Tab/Enter, and deterministic ordering.

**Dependencies**: Composer routing.

**Risks**: Executing incomplete commands or degrading palette search.

### P1-02 Consolidate reusable modal chrome and tabbed settings

Claims: C020, C025, C040.

**What**: Centralize modal frame, optional tabs, breadcrumbs, shortcut footer, focus, and parent overlay snapshot restoration; adapt the settings editor to the reusable tab model.

**Why**: Grok's consistent modal grammar makes the interface feel finished. Harness already has a larger close target and a strong question-card grammar; those should be retained.

**Where**: Harness `crates/harness-tui/src/ui_overlays/settings_editor.rs:51`, `crates/harness-tui/src/ui_overlays/modal_interaction.rs:887`, and `crates/harness-tui/src/overlay.rs:190`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal_window.rs:429`, `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal_window.rs:464`, and `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/modal.rs:152`.

**How**: Create one geometry/chrome contract consumed by actual modal surfaces such as settings, help, model, and command dialogs; keep tabs optional; preserve per-modal content renderers and the 6-cell Harness close target.

**Verify**: Exact-render and hit-map tests at 80×24, 120×40, and 160×50; keyboard and mouse focus restoration tests.

**Dependencies**: Shell mode and key routing.

**Risks**: One oversized abstraction. Share chrome, not domain state.

### P1-03 Align material roles and animate the Harness startup identity

Claims: C026, C029, C035.

**What**: Align runtime contrast/elevation roles with Harness's canonical tokens, then give the existing Harness `H` and startup copy a short staged reveal with reduced-motion fallback.

**Why**: Grok's animated welcome screen establishes polish before interaction. Copying its lettering would violate the branding requirement; animating Harness's own identity achieves the same experiential role.

**Where**: Harness `crates/harness-tui/src/startup_logo.rs:6`, `crates/harness-tui/src/app/motion.rs:78`, and `crates/harness-tui/src/theme_tokens.rs:57`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/views/welcome/logo.rs:51`, `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/appearance/config.rs:536`, and `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/theme/groknight.rs:43`.

**How**: Route surface, border, muted-text, and elevation consumers through canonical Harness tokens; preserve Harness accents. Stage mark, product name, and first-input affordance on the existing scheduler; freeze to the final frame when reduced motion is active; keep Harness logo, name, palette, and voice unchanged.

**Verify**: Runtime-theme snapshots for contrast and elevation, frame-sequence snapshots, a reduced-motion static snapshot, and PTY captures at first paint and ready state.

**Dependencies**: Existing 33 ms scheduler, which already matches Grok cadence.

**Risks**: Startup latency and terminal flicker. Never block input on animation.

### P1-04 Finish responsive feedback: live resize debounce, follow dimming, and glyph fallback

Claims: C028, C029, C030, C031, C038; debate dispositions D2d and D2f.

**What**: Wire the dormant resize debouncer, blend the scrollbar thumb style toward the track style while following, and route status glyphs, spinner frames, footer diamonds, and scrollbar rail characters through terminal-safe glyph selection.

**Why**: These are small but visible discontinuities. Harness already owns exact width-sensitive anchors and matching animation cadence; the fix is integration, not a new architecture.

**Where**: Harness `crates/harness-tui/src/input/resize.rs:5`, `crates/harness-tui/src/runtime.rs:1079`, and `crates/harness-tui/src/ui_transcript_scrollbar.rs:139`; reference `inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/event_loop.rs:2604`, `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/render/scrollbar.rs:222`, and `inspirations/grok-build/crates/codegen/xai-grok-pager-render/src/glyphs.rs:225`.

**How**: Route live resize events through the existing debouncer; preserve `display_column` during reflow; blend follow thumb toward track; use Harness `GlyphMode` for live status, spinners, rails, and low-capability terminals.

**Verify**: Resize burst tests without sleeps, anchor preservation across width changes, `Basic`/`Unicode` terminal snapshots, and reduced-motion checks.

**Dependencies**: P0-02 transcript state and P0-06 PTY evidence.

**Risks**: Added input latency or anchor regression. Don't replace exact full-history measurement with Grok's estimated off-screen heights without separate performance proof.

## Detailed comparison by surface

| Surface | Harness today | Grok Build reference | Required disposition |
|---|---|---|---|
| Shell/layout | Named viewport and fixed dock (`crates/harness-tui/src/layout.rs:251`) | Swappable full-terminal views (`inspirations/grok-build/crates/codegen/xai-grok-pager/src/app/app_view.rs:221`) | Adapt full-surface ownership; preserve viewport contract |
| Dashboard/navigation | Capped responsive overlay; operator sidebar | Grouped full view and stable pane state | Promote the dashboard and compose the sidebar rather than replacing it |
| Transcript | Exact anchors and model-time grouping | Visible folding, response navigation, pin reserve | Add affordances in one projection layer |
| Rich content | Borderless tables, lost link targets | Formatted wrapped tables and propagated links | Port rendering semantics, keep diff grammar |
| Composer | Bordered Harness card; ambiguous modes | Legible mode/action routing | Make modes explicit; keep Harness visual identity |
| Commands | Strong palette ranking, sparse slash metadata | Rich slash scoring/highlights/args | Extend slash; preserve palette |
| Tools/tasks | Strong event truth, narrower presentation | Denser pane and status affordances | Deferred: define owned acceptance criteria after transcript grammar lands |
| Overlays | Strong question cards, uneven chrome | Reusable tabbed modal grammar | Share chrome only |
| Theme/material | Harness tokens and brand accents | GrokNight material hierarchy | Match contrast roles, not brand colors |
| Motion | Matching 30 fps status cadence; global reduced motion | Polished welcome/follow feedback | Add startup/follow feedback; preserve reduced motion |
| Responsive geometry | Exact full-history measurement; dormant debounce | Active debounce; estimated off-screen layout | Wire debounce; reject weaker estimation by default |
| Accessibility | Global reduced motion and capability fallback | Mouse override and per-block controls | Preserve capability-gated mouse behavior; defer an explicit override until a concrete requirement exists |
| Recovery | Raw/substring errors and false reconnect copy | Typed visible recovery states | Implement typed truthful state machine |
| Verification | TestBackend plus limited PTY smoke | Emulator-owned screen assertions | Make emulator PTY the parity oracle |
| Branding/assets | Static Harness `H` | Animated Grok wordmark | Animate only Harness assets |

## Implementation roadmap

1. **Evidence foundation**: P0-06 first. Add emulator PTY assertions and canonical 80×24, 120×40, 160×50 artifacts.
2. **Shell and state**: P0-01 and P0-05. Establish full-surface ownership and truthful recovery before polishing child surfaces.
3. **Transcript document**: P0-02 and P0-03. Land folding and navigation, adding pin reserve only if the detached-streaming reproduction exposes anchor instability; then land rich content and selection semantics.
4. **Input grammar**: P0-04 and P1-01. Make composer state explicit, then slash argument UX.
5. **Consistent chrome**: P1-02 and P1-04. Consolidate modal grammar and finish responsive/terminal feedback.
6. **Brand polish**: P1-03. Add Harness-native material integration and startup motion after scheduler and reduced-motion checks pass.

For each phase, run:

```bash
cargo fmt --all -- --check
cargo clippy -p harness-tui -p harness-testkit --all-targets -- -D warnings
cargo nextest run -p harness-tui
RUST_TEST_THREADS=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1
scripts/test-lanes.sh signoff-pty
bash scripts/harness-qa-dogfood.sh --self-test
```

## Verification matrix

| Gate | Observable proof |
|---|---|
| Visual fidelity | Canonical-size emulator screenshots reviewed against Harness-brand expectations |
| Spatial stability | Dashboard round-trip and resize preserve exact transcript anchor |
| Interaction | Composer/command/modal state-machine tests plus PTY tests for valid and invalid input paths |
| Motion | First/last frame captures, scheduler cadence, reduced-motion static output |
| Rich content | Golden CJK/emoji/table/link/open-fence fixtures and OSC-8 PTY bytes |
| Recovery | Deterministic disconnect/retry/reload states and action availability |
| Performance | On Linux x64 release builds with a 10,000-block transcript, run 100 alternating 80↔160-column resizes after 10 warmups; p95 resize-to-render stays within one 33 ms frame without weakening anchors |
| Accessibility | `Basic`/`Unicode` terminals, keyboard-only operation, reduced motion, and capability-gated mouse behavior |
| Branding | Automated checks that shipped product names and logo assets remain Harness-owned, plus visual confirmation that no Grok marks appear |
| Release evidence | Command, binary hash, terminal metadata, dimensions, screenshot/transcript hashes |

## Unresolved and refuted claims (claim dispositions)

- **Refuted**: Harness lacks aggregation. It already groups adjacent transcript verbs; add only visible folding.
- **Refuted**: Harness lacks OSC-8 support. The encoder exists but is not wired through production rich rendering.
- **Refuted**: Harness status animation cadence is behind Grok. Both use a 33 ms/30 fps base.
- **Refuted**: Grok's close target is larger. Harness's six-cell target is already larger.
- **Refuted**: Harness terminal-native quantization differs materially. The current `Basic` capability behavior matches.
- **Refuted**: Grok's estimated off-screen heights are automatically better. Harness exact anchors are a stronger contract.
- **Rejected transfer**: Grok `wrap_restore.rs`; it owns child terminal modes, not transcript navigation.
- **Rejected transfer**: Dynamic ACP command replacement without a Harness provider source and collision policy.
- **Conditional/dead-end until reproduced**: Persistent pin reserve should land only after a deterministic detached-streaming test reproduces anchor instability.
- **Excluded as unproved**: Send-now cancellation promotion loss and failed-send requeue loss; source review shows interrupt tracking but not the claimed failure.
- **Conditional**: Scroll coasting is a redesign, not a parity patch; follow dimming is the supported transfer.

## Methodology

The audit used an opening DAG with 64 nodes, a first EXPAND wave with 12 groups, a second wave with 11 leads, adversarial counterchecks, and direct review of source anchors in both trees. The claim graph classified statements as supported, refined, refuted, conditional, or unresolved. Named coverage included shell/layout, transcript, rich content, composer, navigation, overlays, tools/tasks, typography/color/material, motion, responsiveness, accessibility, performance, tests, architecture, assets/branding, and sequencing. Research artifacts and debate history live beside this report in `.omo/ulw-research/20260831-044447/`.
