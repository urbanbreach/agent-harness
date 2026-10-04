# Subagent activity parity

This follow-up covers the four requested areas: activity states, retry errors,
non-shell tool descriptions, and Unicode truncation. It compares the local
reference revision `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` with Harness.

## Behavior

| Area | Matched behavior |
| --- | --- |
| Activity states | Waiting, thinking, responding, compaction, blocking output waits, instant polls, task waits, sleep, and foreground child waits. Blocking output waits and sleep use the parked cue; foreground child waits retain the spinner and Stop control. |
| Streaming arguments | Tool-specific writing labels, qualified tool names, ordinals, name retention across nameless fragments, ten-second staleness, and phase timing across successive tool calls. Parent rows retain the last reported activity when a stream expires. |
| Retry errors | Classified headlines, HTTP status wording, attempt labels, warning color, and width clipping. The coordinator announces retries before backoff; resumed output clears the retry state. Raw provider error text is not added to durable metadata. |
| Non-shell descriptions | The first nonempty description line takes precedence for every tool, with internal spaces preserved. Blank descriptions fall back to the tool title. Compact labels and full child status use their respective reference rules. |
| Unicode | Activity subjects stop at 40 code points. Display-width truncation, ellipses, wide-character continuation cells, and control overlays follow the reference. Combining marks, CJK, and joined emoji are covered. This is the requested reference behavior, including its code-point cutoff inside a grapheme. |

The source audit is recorded in [source-check.json](source-check.json). The
capture checkout changes only elapsed-time instrumentation in the activity
sources. The capture adapter also permits a real eleven-second pause to verify
stream expiry. Reference activity decisions and text are not replaced by fixtures.

## Visual evidence

**1,060 of 1,060 activity-surface comparisons match pixel for pixel:**
63,581,000 pixels compared, zero different pixels. There are
212 distinct paired journeys, all using the same final Harness binary.
Results are recorded in [activity-comparison.json](activity-comparison.json).
The compared surfaces are the parent Tasks rows, child title row, and child
activity/status row. Each comparison includes its exact rectangles and PNG hashes.

Both programs render through their production Ratatui/Crossterm paths into real
PTYs and xterm.js 6 in Bun.WebView. Reference frames are frozen before Harness
captures. Keyboard input opens Tasks, selects a child, opens it, and returns to
the parent. No images are resized, recolored, or substituted for terminal output.

The matrix exercises all writing-label branches, classified retry types and
HTTP statuses, a live backoff notice, description fallbacks, Unicode boundaries,
wait states, compaction, and expired argument streams. It includes 48, 64, 69, 70,
72, 80, 120, and 160 columns; the phase-timer boundary is tested on both sides.

This is a parity claim for the four requested activity surfaces, not for entire
transcripts. Full terminal images are retained. Generic thought/tool transcript
rendering, prompt selection framing, and the reference's compaction transcript
notice are outside this comparison; a matching activity row does not establish
that those surrounding surfaces match.

| Example | Harness | Reference |
| --- | --- | --- |
| Classified retry | [120×40](retry-http-503-120x40-child-open-candidate.png) | [120×40](retry-http-503-120x40-child-open-reference.png) |
| Live backoff notice | [120×40](retry-backoff-120x40-child-open-candidate.png) | [120×40](retry-backoff-120x40-child-open-reference.png) |
| Non-shell description | [120×40](description-multiline-120x40-child-open-candidate.png) | [120×40](description-multiline-120x40-child-open-reference.png) |
| Second streamed tool | [120×40](writing-second-120x40-child-open-candidate.png) | [120×40](writing-second-120x40-child-open-reference.png) |
| Combining-mark cutoff | [80×24](description-combining-80x24-child-open-candidate.png) | [80×24](description-combining-80x24-child-open-reference.png) |
| Parked task wait | [80×24](wait-tasks-80x24-child-open-candidate.png) | [80×24](wait-tasks-80x24-child-open-reference.png) |
| Expired stream, retained parent label | [120×40](writing-expired-120x40-tasks-candidate.png) | [120×40](writing-expired-120x40-tasks-reference.png) |
| Expired stream, current child activity | [120×40](writing-expired-120x40-child-open-candidate.png) | [120×40](writing-expired-120x40-child-open-reference.png) |
| Phase timer at its width threshold | [72×24](writing-second-72x24-child-open-candidate.png) | [72×24](writing-second-72x24-child-open-reference.png) |

## Logical verification

The maintained regressions exercise native runtime-event ingress, navigation,
streamed names, nameless fragments, expiry, phase-timer continuity and width,
non-shell descriptions, Unicode cutoffs, waits, retry recovery, and compaction
start/end. Coordinator tests check delivery of safe tool names and retry notices
before redispatch. Existing redaction assertions remain in place.

Validation receipts are in [validation.json](validation.json). The workspace
Nextest run passed 1,962 tests, and the final TUI rerun passed 1,636 tests. Strict
workspace Clippy, formatting, diff checks, and test-suite gates passed. The
quality-gates lane still fails on five existing branding references: four in
`plans/001-role-scoped-subagents.md` and one in
`docs/evidence/tui-rewrite/runtime-state/post-measure-host.json`.

Full fixtures, PTY streams, terminal cells, input receipts, and cleanup receipts
are retained under `.omo/evidence/subagent-activity-20261004/`. The compact
[capture manifest](manifest.json) records the paired executions and binary,
font, and image hashes; [fixtures.json.gz](fixtures.json.gz) preserves their inputs.
