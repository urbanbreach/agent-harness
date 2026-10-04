# Subagent lifecycle and interaction parity

This follow-up corrects gaps in the earlier subagent rewrite. Native completion
was displayed twice: once from its lifecycle transition and again from the
legacy background-notification projection. The notification also created a
synthetic queued activity, which explained why the extra row and surrounding
space disappeared after later updates.

Native completion delivery now wakes the parent without creating another
display activity. Background attempts retain their start row and append one
terminal row; foreground attempts replace their start row. Reactivation retains
the earlier attempt's history. Group counts use distinct children, and a group
with mixed states says `Running 1 subagent, 1 completed`.

## Source comparison

The audit followed the reference's lifecycle ingress, attempt reconciliation,
activity propagation, transcript blocks and grouping, Tasks input, child takeover,
selection, sticky prompts, scrollbars, and status/footer layout. The 19 reference
files checked against the capture build are recorded in
[source-check.json](source-check.json). The two clock-instrumented files differ
only by test-support elapsed-time overrides; their instrumented hashes are also
recorded. No reference product names were introduced into Harness symbols or UI.

| Area | Resulting behavior |
| --- | --- |
| Completion and replay | One terminal row per background attempt; no synthetic native wake activity; earlier attempts survive reactivation and replay. |
| Foreground work | Terminal status, elapsed time, failure details, and muted terminal marker replace the running row in place. |
| Groups | Starts and terminal rows form the same group; each child counts once; completed wording and failure suffix match. |
| Transcript input | Press focuses, release selects, and subsequent activation expands a group or opens a child. Selection frames cover the group only while its selected member is visible. |
| Tasks | Hidden-to-focused opening, unfocused-to-focused activation, focused-to-hidden closing, Escape, Tab, Space, completed filtering, empty copy, and contextual shortcuts match. |
| Child completion | Retained running tool indicators settle, a missing elapsed footer appears, and Cancel/status space disappears. Recorded terminal facts retain precedence. |
| Narrow terminals | Scrollbars occupy the outer gutter; group expansion does not change wrapping; long pinned prompts retain their full collapsed height. |
| Labels and status | Tagged descriptions remain in transcript history while display titles strip the tag. Resumed badges, elapsed values, retry timing/labels, shell descriptions, selection arrows, and Open/Expand hints match. |

All changes are presentation or input handling in `harness-tui`. Coordinator
permissions, cancellation, scheduling, event append, and lifecycle authority
remain the runtime boundary. Child inspection and replay do not execute tools.

## Visual evidence

**300 of 300 paired captures match exactly: 237,917,400 pixels compared, zero
different pixels.** This comprises 30 whole-terminal child views and 270 parent
comparisons using the scope below. All Harness captures use one unchanged final
binary; the reference frames were frozen before the final Harness recapture.

The final evidence uses real production renderers and input handlers through
Crossterm PTYs and xterm.js, displayed in Bun.WebView. Session fixtures enter
through native runtime events on the Harness side and ACP notifications on the
reference side. Images are captured after xterm acknowledges parsing and drawing;
they are compared without scaling, recoloring, or image replacement.

The matrix covers ten states at 80×24, 120×40, and 160×50: running, partial
completion, all completed, failed, cancelled, foreground, long Unicode
descriptions, reactivated, retrying, and executing a described shell command.
Each state captures the initial frame, mouse press/release, group expansion,
Escape, Tasks activation, completed filtering, task selection, child opening,
and return to the parent.

The entire child terminal is compared. Parent comparisons include the transcript,
Tasks pane, empty body space, scrollbar, subagent watcher, and contextual footer.
Unrelated root header/model/composer chrome, the ordinary initial footer, and the
root prompt's timestamp are excluded. Timestamp exclusion includes one adjacent
cell on each side for glyph rasterization; the outer scrollbar remains compared.
Exact comparison rectangles and image hashes are saved in
[comparison.json](comparison.json).

| Capture | Harness | Reference |
| --- | --- | --- |
| One completed, one running | [120×40](partial-120x40-group-expanded-candidate.png) | [120×40](partial-120x40-group-expanded-reference.png) |
| Both completed | [120×40](completed-120x40-group-expanded-candidate.png) | [120×40](completed-120x40-group-expanded-reference.png) |
| Reactivated child with earlier completion retained | [120×40](reactivated-120x40-group-expanded-candidate.png) | [120×40](reactivated-120x40-group-expanded-reference.png) |
| Completed child with long prompt | [80×24](long-80x24-child-open-candidate.png) | [80×24](long-80x24-child-open-reference.png) |
| Retrying child | [120×40](retrying-120x40-child-open-candidate.png) | [120×40](retrying-120x40-child-open-reference.png) |
| Executing command and selection hints | [120×40](command-120x40-child-open-candidate.png) | [120×40](command-120x40-child-open-reference.png) |

These are deterministic fixture comparisons, not live-provider captures or a
proof for every terminal, font, provider, and possible event sequence. The earlier
approved shared skill-permission and clipboard exceptions remain as documented
in [the original report](../../subagent-parity.md).

## Validation

The maintained native lifecycle regression now checks duplicate completion,
delayed parent delivery, completed-child inspection, retained attempt history,
distinct-child counting, and replay restoration. Existing navigation expectations
were updated for the reference's Tasks focus cycle.

The [initial failing run](completion-regression-before.log) reproduces both
completion rows before the fix; the same regression passes in the final suite.

The final workspace Nextest run passed **1,960 tests**, with 8 tests and 7 binaries
skipped by the existing profile. Formatting, strict workspace Clippy, test-suite
gates, and the dogfood self-test also pass. The quality-gates lane still fails on
five pre-existing branding references: four in
`plans/001-role-scoped-subagents.md` and one in
`docs/evidence/tui-rewrite/runtime-state/post-measure-host.json`.

The [capture manifest](manifest.json) records fixture, input, binary, font, source,
and image hashes. The [fixtures](fixtures.json), [image index](images.json), and
[validation receipts](validation.json) are retained with this report.
Full PTY streams, cell dumps, fixtures, and per-frame receipts are retained locally
under `.omo/evidence/subagent-lifecycle-20261004/final-verified/`.
