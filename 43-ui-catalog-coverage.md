# Grok Build UI Catalog QA Coverage

Candidate: detached HEAD `dabcf8b94efa704443b083e0c34ebce176d53c6d` in `/tmp/opencode/agent-harness-review-dabcf8b`.

Global limitation: the pinned Grok reference binary and the expected final reference corpus are absent. Exact pixel or motion parity therefore remains unprovable for every row; no Harness snapshot was substituted as the reference.

Status meanings:

- **PASS (bounded):** the available behavioral owners and fresh evidence satisfy the bounded checks listed here, but not unavailable pixel comparison.
- **FAIL:** fresh evidence directly contradicts at least one catalog done-state requirement.
- **INCOMPLETE:** positive evidence exists, but a required state or exact comparison is missing.

| Catalog item | Status | Fresh evidence | Result |
|---|---|---|---|
| UI-01 Full GrokNight role system | INCOMPLETE | Full owner suite theme tests; truecolor metadata in captures `11`-`41`; `40-terminal-capability-receipt` | HarnessChat is the default and truecolor/capability roles are exercised, but exact all-role comparison is blocked by the missing reference and the full owner suite contains palette/snapshot regressions. |
| UI-02 Startup and empty shell | FAIL | `11-production-startup-120x32`, `12-production-first-grapheme-120x32`; strict PTY startup tests pass | Hands-on startup and type-to-dismiss look structurally correct, but owner tests `startup_home_screen_renders_compose_first_shell` and `startup_typing_moves_to_quick_start_prompt` fail. |
| UI-03 Live shell, composer, footer geometry | FAIL | `14-production-multiline-80x24`, `37-long-unbroken-draft-60x20`; catalog-critical log `06` | Failed-state composer band is 3 rows instead of the required 4. Browser Ctrl+J input did not create multiline draft rows, Shift+Enter owner tests fail, and rapid compact input dropped the digit `4`. |
| UI-04 User rows and page flip | FAIL | Catalog-critical `tx_user_blocks_preserve_submission_order_across_turns`; full-suite P0 transcript test | The first user block is missing across turns. The full owner suite also reports the P0 PageUp offset contract failure. |
| UI-05 Follow, manual scroll, resize, return live | FAIL | `25-scroll-detached-120x40`, `41-scroll-return-details-end-120x40`; strict PTY resize/E2E | Hands-on PageUp detaches and `PageUp -> Tab -> End` reattaches with the down indicator removed; resize smoke passes. However `transcript_scroll_selection_and_tool_detail_toggle_under_full_width_shell` fails its PageUp offset assertion, so the complete interaction contract is not green. |
| UI-06 Assistant streaming settle | FAIL | `15-stream-120x40`, `38-stream-interrupt-120x40`; `40-capability-motion-stress.log` | Incremental 10,000-event update tests pass, but the browser interruption probe remains visibly active after Ctrl+C and grouped-stream owner snapshots fail. Smooth cancellation/settling is therefore not proven end-to-end. |
| UI-07 Thinking/reasoning presentation | FAIL | `16-thinking-120x40`; animation owners in log `40`; full-suite animation-cache failure | Running thinking is visible and fixed-tick owners pass, but the required running/truncated/expanded/finished visual set is incomplete and `transcript_layout_cache_invalidates_when_animation_frame_changes` fails in the full suite. |
| UI-08 Tool rows, grouping, disclosure | FAIL | `17-tool-running-120x40`, `18-tool-settled-120x40`; strict PTY tool test | Group summaries and disclosure are reachable, but visible rows retain the legacy `┃` rail and multiple tool-row owner tests fail. This contradicts the flat, concise done state. |
| UI-09 Tool animation choreography | FAIL | `32`-`35` transition captures; `40-capability-motion-stress.log` | Deterministic fixed-tick, finish-flash, reduced-motion, and idle-redraw owners pass. However all four fresh browser PNGs (pre-finish, edge, settled, reduced motion) have the same SHA-256, so hands-on evidence does not demonstrate the required transition; the full suite also has animation-state regressions. |
| UI-10 Edit and diff disclosure | FAIL | `19-diff-collapsed-120x40`, `20-diff-expanded-120x40`; two catalog-critical failures | Collapsed/expanded toggling works, but expanded output lacks explicit removed/added markers, the removed/added projection owner fails, and the legacy outer `┃` rail remains. |
| UI-11 Complete lifecycle without shell movement | FAIL | `21`-`24` failed/recovered/completed/cancelled captures; strict PTY lifecycle tests; log `06` | Lifecycle states are visually distinct in static captures, but failed-state composer geometry is wrong, lifecycle snapshots fail, and the Ctrl+C helper capture does not leave the active state. |
| UI-12 Permission and question interactions | FAIL | `27`-`30`; strict PTY and permission E2E | Draft preservation passes; selecting question option 2 visibly marks Green and shows `decision sent`. Nevertheless permission/modal snapshots fail in the full owner suite and complete focus/motion restoration against the reference cannot be signed off. |
| UI-13 Redraw cadence and long-transcript performance | PASS (bounded) | `40-capability-motion-stress.log` | 67/67 focused tests pass, including zero paints over 1,000 idle polls, bounded wheel flood, independent clocks, resize remeasurement, and single-block updates after 10,000 settled events. Exact reference cadence remains unavailable. |
| UI-14 Responsive and compact terminals | FAIL | Seven `31-resp-*` xterm captures; `36` CJK and `37` compact long draft; strict PTY resize | All seven required idle viewports have exact row/column counts, no overflow, and aligned borders; CJK wide cells align. Full-suite layout/breakpoint owners still fail, only idle was captured at all seven sizes, and compact rapid input dropped one character. |
| UI-15 Capability and reduced-motion fallbacks | PASS (bounded) | `40-terminal-capability-receipt`; focused 67/67 lane; `35` reduced-motion capture | Truecolor/reduced-color detection, legacy keys/mouse/clipboard, synchronized output, CJK widths, reduced-motion settle, and zero-idle-redraw behavior pass. The browser capture tool intentionally forces truecolor, so no reduced-color screenshot is claimed as pixel parity. |

## Aggregate evidence

- Full `harness-tui` owner suite: **2527 passed, 101 failed, 1 skipped** (2628 total).
- Catalog-critical deterministic set: **122 passed, 4 failed** (126 total).
- Strict reference-parity PTY: **50/50 passed**.
- Strict PTY E2E: **8/8 passed**, including real resize and clean exit.
- Focused capability/motion/performance: **67/67 passed**.
- Real xterm.js corpus: **36 captures**, each with PNG, plain text, raw ANSI, metadata, and PTY cleanup receipt.
- Offline dogfood self-test: **PASS** (`harness-qa dogfood OK`).

Overall catalog outcome: **FAIL**. Direct P0 failures in shell geometry, user-turn preservation, diff projection/rail removal, composer input, and owner-suite consistency block approval independently of the missing reference artifacts.
