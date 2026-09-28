# TUI rewrite record

This rewrite preserves the terminal interface at
`1bb0f98988670a5f4b48cdf749b455a79cfdaa82`. The coordinator and the backend's
public contracts remain authoritative. Work happens on `codex/tui-rewrite`.
Nothing has been pushed.

## Reference

The isolated reference checkout is
`/home/urbanbreach/.codex/worktrees/tui-reference/agent-harness`.
Its release executable is built with:

```sh
cargo build --release --locked -p harness --bin harness
```

The reference checkout must remain available until the comparison and independent
review finish. Its original TUI source must not change. Additional measurement
fixtures may be copied into its tests or examples, with their hashes recorded.

The initial deterministic run passed 1,881 tests, with six excluded by the
existing configuration. It used:

```sh
cargo nextest run --profile ci -p harness-tui --all-features
```

Raw evidence lives under `artifacts/tui-rewrite/reference`. Browser captures
use `.omo/evidence/tui-rewrite/reference`, the existing capture tool's accepted
evidence directory. These directories are ignored by Git. Before completion,
publish the sample data, comparison receipts, and selected screenshots with the
reproducible commands. An ignored local artifact alone is not published evidence.
The initial samples and CLI happy-path recording are committed under
[`evidence/tui-rewrite/reference`](evidence/tui-rewrite/reference).

The original Linux PTY lane initially passed 18 of 22 checks. The four failures
came from stale fixtures. Correcting them also exposed R4 below. The fixtures now use
canonical completed-assistant events, expand collapsed errors before inspecting
them, check the current Settings footer, and wait for a complete resize frame.
Markdown checks assert rendered emphasis; xterm supplies grapheme-aware visual
evidence that the small VT100 test parser cannot provide. R4 was exempted only
while recording the reference; the candidate now passes the full journey.

The CLI journey now enters the saved session and submits another prompt after
the startup-to-live terminal handoff. The runtime probe also exercises rewind
loading, confirmation, stale responses, failure recovery, and success through
real `LiveUpdate` messages and `UiIntent` callbacks. Three release PTY/xterm runs
passed that journey and exited naturally with termios, protocol modes, sockets,
process groups, and browser profiles restored or closed.

Published evidence includes raw ANSI archives, compressed capture manifests,
539 cell/cursor/intent checkpoints, raw renderer/runtime/browser samples, and
selected PNGs. [`files.json`](evidence/tui-rewrite/reference/files.json) records
file hashes. Full local PNG and browser-cell captures remain available in the
isolated evidence directories for the final comparison. Unpack `*.tar.gz` with
`tar -xzf` and read `*.json.gz` with `gzip -dc`.

## Behavior inventory

The inventory comes from `app`, `keybindings`, the terminal runtime, the existing
integration tests, and the Grok Build reference in `inspirations/grok-build`.
Grok's additional features do not imply that Harness implements them.

| Area | Behavior to preserve | Existing evidence |
| --- | --- | --- |
| Shell | Home, empty/live session, replay, completed session; model/context chrome; narrow layouts | `deterministic_render_test`, `shell_topology_contract_test`, `startup_polish_test` |
| Motion | Startup reveal, shimmer, live reasoning/tool pulse, completion rail, reduced motion | `grok_parity_render_test`, `motion_demand_app_state_test`, P1-03/P1-04 PTY fixtures |
| Composer | Grapheme editing, selection, undo/redo, multiline, paste preview, history, stash, file and agent mentions | Composer tests, `production_composer_reachability_test`, P0-04 PTY fixtures |
| Submission | Send, queue, interject, cancel-and-replace, queued-entry navigation, shell command mode | Prompt queue tests, live turn tests, CLI intent tests |
| Commands | Palette and slash search, aliases, key remapping, leader chords, simple/Vim modes | `keybindings`, `help_browser_test`, slash completion fixtures |
| Dialogs | Help, themes, model/preset selection, auth/connect, toggles, settings, status/usage/extensions | Modal chrome fixtures, model switcher tests, dashboard tests |
| Session tools | Resume/replay, rename/delete/pin, tree, fork/clone, import/export, rewind, worktree picker/new worktree | Session navigation, lineage, foreign import, CLI replay tests |
| Permissions | Decision, rejection feedback, approval confirmation, parked focus, draft preservation, coordinator acknowledgement | Permission snapshots, interaction captures, PTY happy path |
| Questions | Single/multiple selection, freeform answer, multiple questions, fullscreen, focus, dismiss/copy | `question_focus_visual_test`, tool interaction captures |
| Transcript | Markdown, open/closed code fences, syntax, tables, diffs, Mermaid, reasoning, timestamps, folds, tool grouping | Tool body/order captures, streaming settle tests, Grok alignment tests |
| Navigation | Follow/detach, stable anchors, turn/block/hunk navigation, search/viewer, selection/copy, mouse drag, scrollbar | Transcript tests, runtime input tests, P0-01/P0-02/P0-03 PTY fixtures |
| Tasks | Child status/actions, foreground/background, cancellation, notifications, todo pane | Background status tests, tool order and interaction captures |
| Terminal | Capabilities, input decoding, focus, paste, mouse, titles/notifications, clipboard, output pressure, teardown | Terminal and runtime tests, PTY and xterm captures |
| Security | Sanitized text/links/paths, redacted evidence, replay without network/tool execution, conservative unavailable states | CLI replay read-only, runtime side-effects, capture security tests |

The final coverage table must distinguish tested behavior, recorded defects,
unsupported features, and unverified environments. A passing renderer check
does not establish runtime reachability.

The public-boundary oracle records 539 frames with complete cell colors,
underline colors, modifiers, cursor positions, input sequences, and emitted
intents. It covers populated plans, memory, settings, model selection, stash,
resume/replay, permission submissions, file/subagent mention selection and
submission, and queued-entry navigation with exact draft restoration.

Reachability was checked separately from rendering:

- Child/parent/sibling session navigation already has public integration coverage
  in `session_navigation_keybindings_test`; live-parent stream isolation and
  typed child-fragment settlement have useful behavior checks to retain.
- Mouse ownership, stale presses, selection release, and clipboard selection
  have existing behavior tests. Keep those observable assertions when replacing
  their current private fixtures.
- Attachment ingestion, MCP resource mentions, and queue edit/reorder APIs have
  injected tests but no shipped CLI/input callers. Preserve required submission
  data contracts; do not claim those helper APIs are reachable UI features.
- The real runtime rewind probe covers acknowledgement and stale generations;
  the CLI PTY journey covers a successful preserved-terminal handoff. The
  restoration probe now covers a failed successor initialization too.
- Marketplace and Feedback lead to Help; Plugins leads to toggles. Export shows
  the CLI command. Account credits/billing and extension installation report
  unavailable. These are existing limits, not features to invent in the rewrite.

## Existing defects and decisions

These findings precede the replacement. Confirm each with a behavioral check
before changing the implementation.

| ID | Finding at reference commit | Decision |
| --- | --- | --- |
| R1 | `runtime.rs` initializes fallible presentation/scheduling telemetry after terminal setup but before installing `TerminalRestoreGuard` | Fix in the replacement. An initialization error must restore the terminal. |
| R2 | `teardown_terminal_session` returns on its first failed restore operation | Fix in the replacement. Attempt the remaining restoration operations and return the error. |
| R3 | Detaching completed history at 40×24 changes the viewport between the first and second paints, without an input or clock change | Record both buffers. Compare the settled reference viewport; the replacement must resolve navigation before painting. Candidate checks require its first paint to match. |
| R4 | Shift-K/J response navigation positions the answer under its sticky header, hiding a short answer and preventing reliable advancement | Fix in the replacement. The selected answer's first line must remain visible and navigation must clamp at both ends. |
| R5 | Direct `tui --scenario ... --exit-on-finish` preserves the terminal but its CLI route never closes it | Move final preserved-terminal cleanup to the shared CLI exit path. The PTY check first failed on active alternate-screen/paste modes. |
| R6 | Setup flush failures can leave enabled modes untracked; shutdown can hide the first error or leave synchronized output open after a partial frame | Track completed keyboard pushes before flushing, arm idempotent mode cleanup before setup, end synchronization, and attempt every cleanup while retaining the first error. |
| R7 | Selection drops the sticky prompt separator from its screen-row map, so a drag below the prompt selects the following source row | Keep an empty slot for the separator. Verify painted highlight placement, release, and copying after the selected text scrolls offscreen. |
| R8 | A viewport anchor in a blank gap resolves to a neighboring content row. Repainting can move the viewport without input or trap one-row scrolling at that boundary | Preserve the signed gap distance from the content anchor. Require repeated preparation and paint to preserve position, including a width round trip. |
| R9 | The margin timeline sums scalar widths, so joined emoji produce different jump offsets from ASCII with the same display width | Measure string display width. A public navigation journey fails on the original and passes on the replacement; its 12 paired post-jump frames remain identical, so this is a numeric geometry correction without a demonstrated visual improvement. |
| R10 | Terminal-panel Home reads a scroll limit written by the previous paint, so it stays at the bottom before the first paint and can use stale wrapping after resize | Derive the limit from the current wrapped rows when handling Home. Retain the last drawable geometry during frame preparation for temporarily hidden panels; painting stays immutable. |
| R11 | The hand-written selection segmenter splits a decomposed Hangul syllable and lets viewer search match an interior jamo | Use the installed Unicode grapheme segmenter for layout and search boundaries, and measure cluster widths as painting does. Existing selection and search fixtures reproduce both failures on the original; a spacing-mark fixture protects painted highlight and copy alignment. |
| R12 | Plan painting, summary counts and pointer geometry read the filesystem independently, so identical state can paint different buffers after a directory change | Read one plan snapshot before painting and hit testing. Refresh during surface opening, frame preparation and plan actions; keep public diagnostic queries fresh. The extended plan journey fails on the preceding implementation, whose plan state/renderer sources still matched the pinned original. |
| R13 | Plan rows and previews budget Unicode scalars rather than cells; shared UI clipping also undercounts emoji presentation sequences | Replace plan painting and use Ratatui-compatible grapheme widths. Eight original/candidate frames cover metadata, combining text, joined emoji and VS16. Terminal and Bash title wrapping keep whole graphemes, including zero-width prefixes, without adding or undercounting rows. |
| R14 | Long transcript tokens undercount VS16 cells, whitespace tokenization splits combining clusters, and selection uses different widths from painting | Replace styled wrapping with borrowed grapheme tokens and align selection with Ratatui widths. Four paired records restore all missing clusters; copy/highlight checks use painted coordinates. Zero-width prefixes remain on their content row. |
| R15 | A first local file/subagent mention can remain absent from a warmed empty transcript until another update invalidates it | Preserve the recorded first-echo timing during the settlement optimization. Explicitly invalidate later local echoes, whose changed selection index previously caused the refresh. A separate correction needs an intentional first-echo behavior change; the original and unconditional-invalidation captures are retained under `evidence/tui-rewrite/settlement-suffix/diagnostics`. |
| R16 | Rewinding a later turn can leave a retained older tool labeled Patch or showing a discarded rejection, because prior display state overwrites the active-history edit proposal | Fold edit state from active events after successful complete-history or untrimmed-inline reconstruction. The pinned original fails the public rewind check. Capped inline child histories retain their prior edit state because the proposal may no longer be available; exact edit rewind there remains unverified. |
| R17 | Tool-row assembly eagerly parses todo data for every visible tool and may read unrelated JSON/text artifacts before discarding the result | Remove the eager todo call and unreachable todo row branches. Production transcript dispatch already hides todo calls; retain the active todo pane parser. This is a source-traced side-effect correction with unchanged visible output, not a syscall-count claim. |
| R18 | Default bindings assign Ctrl+Y to Redo and then overwrite it with AllowPermission, so it does nothing in the ordinary composer | Record the unchanged no-op on both builds. Use the working Ctrl+Shift+Z binding for redo parity. Defer the shortcut-policy correction separately from snapshot ownership; the failed predecessor capture is retained in undo-sharing evidence. |
| R19 | Default bindings assign Ctrl+Home to MoveBufferStart and then overwrite it with FirstMessage, leaving no default buffer-start shortcut | Preserve the existing default. Exercise MoveBufferStart through a configured F12 binding in the public keyboard test; use ordinary Home/End for the navigation workload. Defer the shortcut-policy correction; preparation failures are retained in prompt-editing evidence. |
| R20 | When public prompt fields diverge from the atom editor while both remain prediction prefixes, ghost eligibility follows the prompt fields but its displayed suffix follows the editor. For example, an editor containing `inspect ` and a prompt mirror containing `inspect the ` can paint `inspect the the workspace`. | Reproduced through the public AppState API at `16ab0230`. Preserve it in the composer projection replacement and retain a behavioral check. Correct it when unifying draft state; no shipped keyboard sequence reproducing this divergence has been established. |
| R21 | Raw CRLF in the public prompt mirror has different geometry in the atom row budget and painted string layout. At width 20, `one\r\ntwo` can paint `onetwo` plus an empty row; the cursor still counts the CRLF grapheme. | Reproduced at `9681f532` through the public AppState API. Preserve it during the string viewport rewrite and record exact frame/cursor behavior. Resolve when unifying draft geometry; no shipped keyboard sequence establishing this raw mirror value is known. |

`check-tui-restoration.py` reproduces R1 with a trace path whose parent is a file:
the original exits with raw mode and alternate-screen/paste/mouse modes enabled.
Normal exit restores all checked modes. Injected-writer checks reproduce R2 and
verify that accepted setup bytes are undone after a flush failure. The replacement must pass normal exit, telemetry initialization failure, failed
successor handoff, and `/dev/full` setup failure. With `/dev/full`, only termios
can be verified: no output escape can reach the terminal.

The required R5 compatibility edit is confined to `crates/harness/src/tui.rs`: the CLI
closes any preserved terminal after dispatching every non-replay mode, including
errors, and retains the original failure. Coordinator behavior and contracts are
unchanged. Replay already owns and restores its terminal directly.

## Performance acceptance, set before replacement

The first serial release sample uses 1,000 complete turns (4,000 durable events),
10 warm-up frames and 200 measured frames, repeated three times. Median p95
costs are 140 µs for typing, 4,135 µs for streaming, and 4,996 µs for resize.
Post-workload RSS is 49,224, 51,604, and 66,064 KiB respectively. These cover
public handlers, layout/paint, Ratatui diffing, and Crossterm encoding to a
counting sink. They are not end-to-end terminal latency.

The separate glibc `memusage` run records whole-process allocation totals,
including fixture construction and startup. Streaming allocated 741,881,764
bytes with a 30,042,765-byte heap peak; resize allocated 668,453,171 bytes with
a 40,266,345-byte peak. The same fixed fixture cost stays in both measurements.
CPU ticks are coarse for short static runs; use sustained runtime workloads
for idle and typing CPU conclusions.

The later serial run in `performance-final` retained the same workload and
similar RSS but took roughly twice as long across every renderer scenario. The
cause is not established; the host uses its `powersave` governor. Both sample
sets are retained. This does not relax the original timing limits. Run the
reference and candidate again on the same host immediately before final
performance signoff and report variability.

The three serial browser runs each contain 120 samples per input/stream/resize
workload. Median p99 browser observations are about 94/68/82 ms; raw PTY frame
p99 values are about 9.4/2.3/34.3 ms. The browser values include DOM observation
and two animation callbacks. They are not renderer timings. The sustained
runtime baseline records zero settled-idle CPU ticks and redraws.

Freeze these acceptance rules before changing production source:

- Exact recorded cells, styles, cursor, and intents, except documented defect
  decisions with their own checks. No discarded events or omitted content.
- At least 50% fewer Rust source lines under `crates/harness-tui/src`; every
  replacement source file at most 500 lines. Report test code separately.
- At least 30% lower streaming and resize CPU cost, allocation totals, and
  post-workload RSS, using median serial release runs and identical inputs.
- Streaming and resize renderer p95/p99 at most 70% of reference. Other renderer
  p99 values may not increase by more than 100 µs or 10%, whichever is larger.
- Idle remains quiescent after settling. Startup animations keep their recorded
  cadence and appearance. Sustained typing/burst CPU must fall at least 30%.
- Browser-visible input, stream and resize p95/p99 may increase by no more than
  one 60 Hz display frame (16.7 ms). Report raw PTY timing separately from the
  browser observer, which includes DOM polling and two animation callbacks.
- Successful and failing exits restore terminal state and leave no fixture
  subprocess, task, socket or browser process running. Unsupported environments
  are reported explicitly.

Reproducible measurement commands (finish compilation first, then run serially):

```sh
python3 scripts/measure-tui-rewrite.py --root REFERENCE_OR_CANDIDATE --output OUTPUT --allocations
python3 scripts/measure-tui-runtime.py --binary ROOT/target/release/examples/resource_probe --output OUTPUT/runtime.json
node scripts/qa/measure-rewrite-latency.mjs ROOT/target/release/examples/rewrite_probe .omo/evidence/OUTPUT
python3 scripts/check-tui-restoration.py --binary ROOT/target/release/examples/resource_probe --output OUTPUT
```

The original needs `--record-reference-defects` for the last command. The
candidate must pass without that exemption. Preliminary browser runs used to
debug the observer overlapped builds and are diagnostic only; authoritative
latency samples must be collected without concurrent compilation or capture.

## Terminal ownership replacement

`terminal/session.rs` owns enabled modes from the first setup call through exit
or explicit handoff. It replaces the duplicate capability/teardown state and
unused preserved frame buffer. Shutdown joins the reader and writer before
restoring the terminal, attempts every cleanup, and keeps the original error.
The shared CLI exit closes preserved sessions, including the scenario route.

The writer-failure checks, four PTY restoration scenarios, resumed CLI journey,
539-frame oracle, scoped Clippy run, and workspace check pass. The two injected
writer regressions were observed failing before their fixes. Raw results are in
[`evidence/tui-rewrite/terminal`](evidence/tui-rewrite/terminal).
This stage retained the documented R3 settled-frame allowance for the unchanged
renderer. The viewport replacement below removes that allowance. The original
renderer and most of the state engine still need replacement.

## Event loop replacement

The new runtime waits on terminal input, live updates, frame acknowledgements,
and the next required deadline. Input and live work each have bounded turns;
submission, resize, and clicks paint before provider work. A completed input
barrier gives queued live work a turn, then returns to input. Queued input keeps
its order across that handoff. Idle waits without drawing; animation keeps the
existing motion deadlines. Wheel batches that change nothing skip rendering.

This replaces the old runtime loop, input dispatcher, pacer, arbiter, presenter,
and scheduler. Their structural tests are removed; the public cell oracle,
input/resize/motion PTY journeys, frame-writer behavior, and terminal restoration
checks remain. The original state engine and renderer are still present during
this migration and must be replaced before completion.

Review found and corrected acknowledgement/readiness ordering, starvation in
both directions, prefetched-input ordering, expiry wakes during quit, and final
write-error precedence. A real PTY/xterm check queues 1,000 clicks, delivers a
live notice, and then quits. Disabling the live-work handoff delayed that notice
2,149 ms and failed its 1,000 ms bound.

The same check exposed a separate issue in the unchanged Crossterm Mio reader:
it could wait in edge-triggered epoll with 2,821 bytes still unread from the TTY.
Quit keys in that tail never reached the application. Enabling Crossterm's
existing `use-dev-tty` feature selects its level-triggered reader and fixes the
burst. The unused `event-stream` feature is removed. No package version changed;
`filedescriptor` was already locked. This is terminal-adapter compatibility,
not a coordinator or backend change.

Reproduce the added workflow after building the examples:

```sh
cargo build -p harness-tui --example rewrite_probe --example resource_probe
node scripts/qa/measure-rewrite-latency.mjs target/debug/examples/rewrite_probe .omo/evidence/tui-rewrite/runtime --workflow-only
```

The burst timing is a regression bound in a debug-build workflow, not the final
release performance comparison. Runtime review does not establish completion
of the state and renderer rewrite.

Validation passed: 1,802 deterministic TUI tests, including the 539-frame oracle;
16 native PTY input/Unicode/resize/motion checks; all four restoration scenarios;
the resumed CLI workflow; scoped all-target Clippy; workspace check; and test-suite
gates. The independent reviewer found no remaining issues in this runtime slice.
The final browser workflow delivered the live notice in 64.77 ms and exited
naturally with terminal modes and fixture resources restored.
Raw results, ANSI recordings, and the click-burst PNG are published under
[`evidence/tui-rewrite/runtime`](evidence/tui-rewrite/runtime), with hashes in
[`files.json`](evidence/tui-rewrite/runtime/files.json).

## Viewport ownership replacement

`TranscriptViewport` replaces the parallel follow flag, bottom offset, measured
viewport, and legacy snapshot. Extent, reading anchor, page-flip transitions,
selection anchors, and visible-tool motion are committed during `set_frame_area`
before input or paint. Painting reads those values. The terminal backend receives
visible hyperlink metadata from the same preparation step; the thread-local link
transfer is removed. Startup, empty, and hidden transcript surfaces clear links.

The public oracle now rejects every first/second-paint difference and checks that
paint leaves the interaction snapshot unchanged. Removing the R3 allowance failed
on the original implementation's detached 40×24 history; frame preparation fixes
it without changing the 539 recorded expectations. Five private viewport tests
were removed; public scrolling, reflow, selection, disclosure, and return-to-live
checks retain the behavioral coverage.

Review found capture helpers that had relied on paint to commit geometry and
benchmarks that would have omitted preparation from their timers. Capture helpers
now prepare before navigation and each size. Both benchmarks include preparation
in cold and warm samples, including visible-link extraction. Use the updated
public benchmark on both builds for the final paired comparison; the frozen
acceptance limits remain unchanged.

Validation passed: 1,797 deterministic tests with seven skips, all 539 reference
frames without the R3 waiver, scoped Clippy, and test-suite gates. Eighteen of 19
PTY checks passed; the remaining failure is the recorded R4 defect. The browser
workflow delivered the burst notice in 64.12 ms, restored terminal modes, and
closed its process group, socket, and browser profile. This debug timing is a
regression check, not a resource-improvement claim. Independent review approved
this slice after the benchmark and capture corrections.

The corrected release checks also pass: the 10,000-block resize contract preserves
its anchor (p95 1,342 µs against its existing 8,333 µs limit), and the public
1,000-turn streaming workload keeps both current output and oldest history
reachable. These are smoke checks, not the final paired improvement result.

```sh
HARNESS_REWRITE_SCENARIO=stream HARNESS_REWRITE_HISTORY=1000 HARNESS_REWRITE_FRAMES=100 \
  cargo nextest run --release --profile perf -p harness-tui --all-features --lib \
  --test rewrite_performance_test -j 1 --success-output immediate \
  -E 'test(perf_rewrite_public_boundary_workloads) | test(perf_resize_to_render_p95_stays_within_one_frame_and_preserves_detached_anchor)'
```

Raw reports are in [`evidence/tui-rewrite/viewport`](evidence/tui-rewrite/viewport).

The viewport migration left whole-history projection, global layout caches, and
the original rendering engine in place. The next transcript change replaces the
layout and selection caches.

## Response navigation correction (R4)

Shift-K/J now positions the selected answer below its sticky prompt. Repeated
jumps use the selected response while the viewport remains at its navigation
position, so clamping cannot select the same answer repeatedly. Manual scrolling
resumes position-based navigation. This is the documented R4 correction; the
reference cells for working behavior remain unchanged.

The real PTY journey previously failed waiting for `Harness 2/3`. It now passes
without the reference-defect exemption and requires the first answer to be visible,
advancement in both directions, and clamping at both ends. All 287 scoped
transcript/response checks pass, including the 539-frame oracle; scoped Clippy
passes. The existing keyboard journey also checks navigation after manual scrolling.
Independent review found no blocking issue.

An xterm.js run captures all three selected responses and the return to the first.
Its terminal metadata shows restored modes after palette exit, and cleanup removed
the process group, browser profile, and temporary directory. This used a debug
fixture built from the uncommitted change, with source and executable receipts;
it is not release performance evidence.

```sh
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci --run-ignored all \
  -p harness-tui --all-features --test p0_02_pty_recorded
cargo nextest run --profile ci -p harness-tui --all-features \
  -E 'test(response) | test(transcript) | test(recorded_terminal_journeys)'
```

Raw checks, browser actions, ANSI, cell buffers, screenshots, and hashes are in
[`evidence/tui-rewrite/response-navigation`](evidence/tui-rewrite/response-navigation).
The red PTY report is in [`viewport/pty.log`](evidence/tui-rewrite/viewport/pty.log).

## Prepared transcript replacement

`PreparedTranscript` owns semantic sections and measured rows in `AppState`.
Frame preparation updates the changed activity suffix and retains at most four
width/surface layouts. Session replacement releases the old layouts. This removes
the thread-local layout cache, instance IDs, whole-history content hashes, deep
section comparisons, and selection snapshot cache.

Every measured block now retains compact selection text, cell bounds and links.
Selection queries collect the visible rows and the explicit selected range;
dragging no longer copies the whole history or rebuilds long fallback surfaces.
The R7 check drags below a sticky prompt, checks the highlight after release,
scrolls the answer offscreen, and copies the selected text. Paired PTY/xterm
captures reproduce the defect in the reference: before selection, all cells match;
after release, only the selected 21 cells change foreground and background.

Paired resource measurements exposed R8. A fresh anchor in a blank gap snapped
to neighboring content on its next resolution. The viewport migration had made
paint immutable but still allowed its effective top to differ from the committed
position and prepared links. A signed row bias now preserves the gap. Selection
anchors keep their existing source-position semantics. The public oracle requires
repeated frame preparation to preserve interaction state and painted cells; the
existing Ctrl-Up/Down check crosses section gaps and includes width round trips.

The original 539 recordings remain unchanged. Exactly two require the documented
R8 correction: `detached-history-40x24` and `detached-append-40x24`. Their transcript
body retains one blank row above the next prompt. The comparison derives those
expected cells by moving the original body down one row; chrome, scrollbar,
colors, cursor, inputs and intents retain their original checks. All other
recorded frames compare directly. In the separate 80-step public trace, corrected
frames equal the reference's first paints, and extra preparation produces the
same trace. The reference stalls at a gap when those extra paints occur.

Validation passes: 1,783 deterministic tests with six skips, seven gated PTY
checks, scoped all-target/all-feature Clippy, formatting, and test-suite gates.
The final deterministic run is serial. An earlier parallel run timed out in the
viewer capture; its log remains published and no timeout was relaxed.

The four-paint selection cycle improves from 11,293 to 1,842 µs p95 against the
intermediate whole-history selection implementation. The paired original/current
public runs show 31.3% fewer streaming allocations, but only 9.8%/13.2% lower
streaming/resize RSS. Tail latency is roughly unchanged, resize allocations rise
2.3%, and cold preparation rises about 25%. These miss the frozen whole-rewrite
requirements. All timed output byte counts match. The scrolling report's extra
post-timing screen preserves the R8 gap; other final screens match directly.

Raw checks, paired screenshots and cells, reproduction fixtures, resource samples,
and source/binary receipts are in
[`evidence/tui-rewrite/prepared-transcript`](evidence/tui-rewrite/prepared-transcript).
The original composite state engine and render-surface builders remain during
migration. Streaming and resize resource acceptance is still outstanding; this
slice does not establish completion of the rewrite.

## Direct transcript entry construction

Turn rendering now borrows semantic parts and scans tool groups once. Ten grammar
modules, repeated owned specifications, unused lifecycle/accent/group metadata,
and the identity-conversion trait are removed. Measured sections no longer retain
a second flattened copy of their rows. Nine tests for the removed grammar and
field forwarding are deleted; the behavioral checks remain.

The existing oracle caught an introduced streaming-prose rail. The correction
keeps the original blank accent column, and all 539 checkpoints pass with only the
previously approved R8 corrections. The two xterm streaming PNGs are byte-identical
to the reference. A live PTY selection capture matches the prior approved
candidate and confirms natural exit, terminal restoration and resource cleanup.

Validation passes: 1,774 serial deterministic tests, seven gated PTY checks,
all-target/all-feature TUI Clippy, workspace check, formatting, and test-suite
gates. The independent source review finds no remaining correctness issues.

Paired release measurements show 36.5% fewer streaming allocations and 18.5% less
streaming RSS. Resize RSS falls 26.2%, but resize CPU rises 2.7% and tail latency
is roughly unchanged. Cold preparation remains about 5–6% slower. Typing CPU also
remains above the paired reference. Resource acceptance is still unmet; the
original frozen limits are unchanged.

This slice removes 2,033 Rust source lines, including the obsolete tests. The
source tree now contains 176,400 lines, 3.0% below the original 181,882. The old
composite state engine and lower-level formatters remain during migration. Raw
samples, source/binary receipts, red/green logs and browser comparisons are in
[`evidence/tui-rewrite/direct-entries`](evidence/tui-rewrite/direct-entries).

## Transcript outline replacement

The old composite, block/event mirrors, cache/invalidation layers and duplicated
timeline state are removed. A compact outline retains only identities, markers
and row geometry. `AppState` owns the viewer; pager and dashboard content is
materialized when requested. The existing activity projection remains the content
source. There are no backend or dependency changes.

Independent review caught lost status updates while the terminal height was zero.
The public navigation journey fails on that regression, passes on the original,
and passes after the replacement retains and updates its last valid geometry.
The same journey exposes R9: scalar width sums mismeasure joined emoji. String
display widths fix the numeric jump offsets; all 12 paired post-jump frames remain
identical. The pager check passed before the rewrite and retains exact export and
terminal-restoration assertions.

All 539 frozen checkpoints pass with the same two R8 corrections. Both streaming
xterm screenshots are byte-identical to the reference, and the real PTY selection
capture matches the previous candidate and restores terminal and process state.
The full run passes 1,737 checks and times out one viewer capture during concurrent
release compilation. That capture passes in 16.8 seconds on its isolated rerun;
both logs are retained. Seven gated PTY checks, scoped Clippy, workspace check,
formatting and test-suite gates pass.

Paired release measurements show resize CPU down 63.4%, allocations down 42.1%,
RSS down 30.4%, and p95/p99 down 43.0%/41.9%. Streaming allocations fall 38.0%,
but its tail latency barely improves. Typing CPU remains above the reference.
The frozen whole-rewrite targets are unchanged and still unmet.

The slice removes 3,252 source lines. The source tree now has 173,148 lines,
4.8% below the original, including inline tests. Whole-history projection and
lower-level formatters still remain. Evidence and reproducible commands are in
[`evidence/tui-rewrite/transcript-outline`](evidence/tui-rewrite/transcript-outline).

## Terminal-panel scroll ownership (R10)

Terminal painting no longer writes a scroll limit into `AppState`. Home measures
the same rows and padding used by painting at the current size. Frame preparation
retains the last drawable content rectangle so Home also works during a
zero-height round trip. This keeps geometry ownership outside the renderer.

The existing navigation journey now uses real interactive-PTY output and checks
Home before painting, reflow at 140 and 60 columns, PageDown, temporary zero height,
and independent transcript scrolling. It fails before the correction and passes
after it. The row builder, styles and wrapping remain unchanged; this is a purity
correction during migration, not a replacement of the remaining terminal formatter.
Evidence is in [`evidence/tui-rewrite/terminal-panel`](evidence/tui-rewrite/terminal-panel).

## Borrowed text geometry

Rendering and selection now measure borrowed grapheme slices, preserving the
existing segmentation and display-width rules. Scalar width queries avoid
constructing temporary Ratatui lines. Inline Markdown measures preceding spans
only for linked tokens; unlinked text no longer repeats that unused scan. Output
rows retain ownership where needed. No cache, dependency or backend change was
added.

All 1,738 deterministic checks and seven gated PTY checks pass. The 539 frozen
frames match the preceding candidate exactly. Both streaming xterm PNGs are
byte-identical to the reference; real PTY selection retains its approved cells
and restores terminal and process state. Clippy, workspace check, formatting
and test-suite gates pass. Existing behavioral tests cover the changes.

Against the paired original, streaming p95/p99 fall 70.8%/71.8%, CPU 64.9%, and
allocated bytes 48.9%. Streaming RSS falls only 24.0%, missing its 30% target.
Resize CPU, allocations and RSS fall 64.3%, 45.3% and 30.6%; its p95/p99 fall
42.5%. Typing and scrolling latency increase slightly within their allowance.
All timed output byte counts and oldest screens match.

The frozen absolute resize limits, source reduction and sustained typing/burst
CPU targets remain outstanding. This removes temporary allocations within the
remaining formatters; it does not establish their replacement or completion of
the rewrite. Raw samples, source and binary receipts, and comparisons are in
[`evidence/tui-rewrite/borrowed-text`](evidence/tui-rewrite/borrowed-text).

## Borrowed settled projection

Settled presentation now borrows the canonical transcript and run summary. Inline
child projections remain locally owned; only the final compaction checkpoint is
copied before whole-state mutation. The pass-through event-vector wrapper is
removed. No backend contract or rendering logic changes.

The five settlement checks, all 1,738 deterministic tests, seven gated PTY checks,
Clippy, workspace check, formatting and suite gates pass. All 539 buffers match
the preceding candidate exactly. The change removes 38 source lines.

History workloads allocate about 2.1 MB less than the preceding candidate run;
peak heap and RSS do not measurably improve. Against the paired original,
streaming p95/p99 fall about 71% and allocations 49.2%, but RSS only 23.1%.
Resize RSS falls 30.3%. Small startup, idle, typing and scrolling regressions
remain within the paired-reference allowance. Startup cold preparation rises
9.8%; coarse idle and scrolling frame CPU also rise.

The original absolute timing limits are unchanged: streaming passes, while
resize and the other four workloads still miss their earliest limits. This is
reported separately from the current paired comparisons. Whole-history
presentation and the retained event mirror still need replacement. No fresh
browser or end-to-end latency claim is made. Evidence is in
[`evidence/tui-rewrite/settled-projection`](evidence/tui-rewrite/settled-projection).

## Selection layout replacement (R11)

Selection now stores one grapheme vector with row ranges, replacing the custom
segmenter and separate keyboard engine. Viewer layout reuses its measured row
count; search reads Unicode byte boundaries directly. Public selection APIs and
soft-wrap copy behavior remain intact. The change removes 188 source lines.

R11 corrects decomposed Hangul splitting and partial-syllable search. Independent
review caught a spacing-mark width regression during migration. Measuring whole
graphemes as painting does resolves it; the existing viewer test verifies both
the painted search highlight and exact copying at the following cell.

The full 1,738-test run and final focused, viewer and matrix checks pass. All 539
frozen buffers match the previous candidate. All 51 viewer ANSI frames match the
original, and two xterm screenshots and terminal snapshots are exact matches.
The debug viewer journey is slower on the candidate, so release measurement is
needed before attributing that difference. No new resource-performance or PTY
latency claim is made. Evidence is in
[`evidence/tui-rewrite/selection-layout`](evidence/tui-rewrite/selection-layout).

## Viewer projection and compact geometry

The viewer now materializes visible rows only, while its public owned surface
API still exposes the complete content. Selection and search keep absolute
positions. The viewer and dashboard share immutable geometry backed by one text
string, compact byte/cell ends and row ranges. Public owned-grapheme inspection
remains available. Paint advances through style spans once per row.

The [frozen viewer workload](evidence/tui-rewrite/viewer-baseline/README.md) uses
2,000 Unicode lines and public AppState input/preparation/paint/diff/ANSI paths.
The [visible-row slice](evidence/tui-rewrite/viewer-window/README.md) reduced frame
cost but still failed resize RSS. The
[compact geometry slice](evidence/tui-rewrite/compact-layout/README.md) passes all
viewer limits: resize RSS is 44,784 KiB versus the original 90,672 KiB, allocations
are 2.45 GB versus 16.13 GB, and p95 is 32,777 µs versus 71,036 µs. These are
renderer/process measurements, not terminal latency or runtime-idle results.

All 1,739 TUI tests pass. The 539 recorded frames and 51 viewer ANSI captures
retain their approved output; two representative xterm PNGs match byte for byte.
Independent review caught a long-line search regression in the first compact
attempt. The retained failure and corrected public journey protect the indexed
replacement. Whole-rewrite source, state/formatter replacement, runtime and
absolute timing requirements remain outstanding.

## Status fallback removal

The unreachable centered status fallback and 55 tests of its private summaries
are removed. Public status commands still open the full dashboard. Its compact
summary directly counts the same 68 probe presences and distinct literal edit
paths, without the old temporary diagnostic strings or edit-file digest reads.
Three uncompiled orphan files and an unused private setter are also removed.

All 1,684 remaining TUI tests pass. Four original dashboard-details checkpoints
extend the matrix to 543 without changing any prior reference record; cells,
ANSI and all four xterm PNGs match. The public dashboard journey checks empty,
absent and unavailable probes plus repeated edit paths, and its mutation check
fails as intended. The source tree shrinks by 3,867 lines including the removed
tests. Plan-list reads still need to move out of painting. Evidence is in
[`evidence/tui-rewrite/dashboard-cleanup`](evidence/tui-rewrite/dashboard-cleanup).

## Plan filesystem preparation (R12)

Plan painting, pointer geometry and dashboard presence counts now use one
prepared directory snapshot. Opening either surface and preparing a demanded
frame refresh it; no watcher or timer is added. Public plan rows and summary
queries still read current files. Preview, copy and delete actions reread the
directory, and deletion retains replay, confinement and symlink checks.

The existing multi-plan journey verifies that a directory change between paints
does not change the prepared frame, while the next preparation reveals it. Three
redundant open/close/palette tests are removed; the navigation journey and frozen
matrix retain those routes. Plan rendering and its scalar-based text truncation
still need replacement. This is a paint-purity step, with no new resource or
terminal-latency claim. Evidence is in
[`evidence/tui-rewrite/plan-preparation`](evidence/tui-rewrite/plan-preparation).

## Plan painter replacement (R13)

The plan painter now uses the prepared entries and shared cell clipping. Its
203 lines become 145, preserving popup geometry, row styles, scrolling and
interaction. Wide paths retain their metadata, and previews retain whole
graphemes and visible ellipses. Shared UI clipping now measures VS16 sequences
as Ratatui does. Terminal and Bash title wrapping retain whole oversized
graphemes; terminal rows flush only after consuming display cells.

The original and pre-change candidate match all eight new Unicode plan records.
Their documented differences after replacement stay within the path or preview
rows in both terminal cells and xterm pixels. The other 543 records remain
unchanged. Independent review caught the VS16 mismatch and a zero-width-prefix
row regression; retained red/green checks cover both. This slice makes no new
performance or end-to-end terminal claim. Evidence is in
[`evidence/tui-rewrite/plan-geometry`](evidence/tui-rewrite/plan-geometry).

## Borrowed event inspection

Healthy sessions now borrow canonical events and the pending tail instead of
retaining an event mirror after settlement. Inline child slices and rejected
histories keep their inspection buffer. Prefix offsets preserve the existing
retention cap; navigation, snapshots and intents still expose the retained view.
The backend's canonical validation and event history remain unchanged.

All 1,681 TUI tests and seven gated PTY checks pass. The 543 main and eight plan
records remain exact matches; both fresh xterm PNGs and their ANSI match the
original. The wide browser capture differs only by one asynchronous render.
Two existing behavior tests now cover retention through settlement/rejection
and navigation through invalid replacement histories.

Streaming RSS falls from 39,464 to 35,732 KiB versus the preceding candidate;
resize falls from 45,916 to 42,136 KiB. Both now pass the frozen 30% RSS reduction
limits. Allocation totals barely change because loading still constructs the
temporary inspection buffer. Scrolling and resize timing/CPU regressions are
reported; frozen absolute timing and CPU limits, source reduction, sustained
runtime targets and the remaining implementation rewrite are still outstanding.
Evidence is in
[`evidence/tui-rewrite/event-history`](evidence/tui-rewrite/event-history).

## Styled transcript wrapping (R14)

Styled wrapping now borrows tokens and source link clusters. The old token
buffer, per-cluster link strings, duplicate long-token paths and duplicate
clipping helper are removed. The 234-line replacement uses Ratatui-compatible
cell widths; selection uses the same measurements. Four new paired records
show every VS16 cluster surviving long-token wrapping. The other 551 complete
records remain unchanged, and xterm differences stay within the reply rows.

All 1,682 TUI tests, seven gated PTY checks and quality checks pass. Existing
behavioral tests cover space/combining clusters, zero-width styled prefixes and
copy/highlight at painted link coordinates. Production shrinks by 54 lines and
inline tests by seven; integration fixtures add 55 lines.

Streaming allocation falls 7.0% and resize allocation 1.7% versus the preceding
candidate. Timing is mixed: streaming p99 rises 2,639→2,709 µs, resize rises
5,890→5,921 µs, and scrolling falls 435→405 µs. Frozen CPU and timing failures
remain explicit. Source reduction, state/formatter replacement and sustained
runtime targets are still outstanding. Evidence is in
[`evidence/tui-rewrite/styled-wrap`](evidence/tui-rewrite/styled-wrap).

## Unused presentation paths

Compiler diagnostics and caller searches identified unreachable tool-input,
per-line diff-highlight, startup-card, secondary-layout, status and rail helpers.
Removing them and 11 obsolete private tests reduces production by 817 lines and
test source by 283 lines. The retained limited-color check now exercises the
actual diff renderer and fails if its shared quantization is bypassed.

All 1,671 remaining TUI tests, seven gated PTY checks and quality checks pass.
The 555 complete frame records and their ANSI output exactly match the preceding
candidate. This cleanup makes no new resource or browser claim; the active
state engine and remaining lower renderers still need replacement. Evidence is
in [`evidence/tui-rewrite/unused-presentation`](evidence/tui-rewrite/unused-presentation).

## Compact selection rows

Selection preparation now writes one string and cell bounds per row. The old
per-cell string model and its join/clone conversion are removed across semantic,
fallback and reasoning paths. Copy and highlight rules remain unchanged. All
3,136 diagnostic row comparisons, 555 frame/ANSI records, 1,670 TUI tests and
seven gated PTY checks pass. An existing mouse journey now protects Unicode/link
copy through reflow, with a retained failing anchor mutation. Fresh xterm captures
preserve the accepted R7 highlight correction and terminal restoration.

Production shrinks by 52 lines. In fresh paired release runs, streaming p99 falls
1,242→1,102 µs and allocated bytes fall 7.6% against the preceding retained build.
Resize p99 rises 2.0%, and long-history RSS remains 0.5–1.3% higher after trimming
excess vector capacity. Both streaming and resize RSS remain more than 30% below
the original. All frozen main-workload limits pass in this run. The retained
executables also ran about twice as fast as in the earlier recording, so this
slice does not account for the entire absolute timing improvement.

A separate trace confirms fixed-viewport resize invokes `tput` without a terminal;
that diagnostic is excluded from acceptance samples. These remain renderer/encoder
measurements, not sustained runtime or end-to-end latency evidence. The source tree
is only 7.61% smaller than the original, and the broader implementation rewrite
remains open. Evidence is in
[`evidence/tui-rewrite/compact-selection`](evidence/tui-rewrite/compact-selection).

## Borrowed transcript painter

The transcript painter reads prepared spans directly into the frame buffer.
It removes visible-row/String clones, mutable animation copies, temporary
Paragraph/rail buffers and repeated style fills. The 317-line replacement keeps
Ratatui clipping/alignment and animation precedence. Existing row formatters
remain in a 445-line module and still need replacement.

Independent review caught a halfwidth dakuten/handakuten regression in the first
version. Text and rail clipping now use Ratatui's CellWidth. The extended paint
check fails on the rejected version and passes on both the preceding painter
and the fixed version. All 10,788 diagnostic cell records, 555 full frames and
733 exact-clock chat/tool captures match. Six paired xterm screenshots are
byte-identical; the gated PTY checks and all 1,666 TUI tests pass.

Fresh paired release measurements put streaming p99 at 998 µs versus 1,098 µs
for the preceding build, with 3.8% fewer allocated bytes. Resize p99 is essentially
unchanged, with 1.3% fewer allocated bytes. All fourteen frozen main-workload
limits pass. RSS stays within 0.5% of the preceding build. These are counting-
sink measurements; sustained runtime CPU and end-to-end latency remain open.
The rejected version's measurements are retained and excluded from acceptance.

The change removes 20 production and 55 test lines. Whole TUI source remains
167,968 lines, a 7.65% reduction. The state engine, retained formatters, broader
source reduction and final signoff are unfinished. Raw evidence is in
[`evidence/tui-rewrite/surface-painter`](evidence/tui-rewrite/surface-painter).

## Terminal polling timeout correction

Fresh runtime measurements exposed an idle CPU regression from the earlier
level-triggered reader choice. `filedescriptor` truncated a positive fractional
millisecond to zero, and Crossterm retried until its deadline. A local patch to
the existing dependency rounds waits up and clamps overflow. The parser, owned
reader, event order, bounded queue and joined shutdown remain unchanged. The
reader still checks shutdown through blocking 50 ms waits; removing periodic
terminal polling remains unfinished.

Three paired release repeats restore idle CPU from 1.5% of one core to zero,
with zero output or redraws. The diagnostic poll trace falls from 99,094 calls
to 70 over about 3.5 s. Browser input/stream/resize p95 and p99 pass the unchanged
+16.7 ms allowance against both fresh and frozen references. Nine screenshots
and the final terminal cells match across all six browser runs. The burst and
rewind workflow, restoration checks, 1,666 deterministic tests and seven gated
PTY checks pass. An extended existing PTY journey verifies exact ordered
submission of a single 1,800-byte Unicode burst.

The runtime resource target remains unmet. Fresh original/candidate typing CPU
is 11.75%/13%, with unequal injected input counts; burst CPU is 11.25%/10.75%.
Neither demonstrates the required 30% reduction. Startup's short-window frame
counts also differ from the original and do not establish cadence parity.

The patch adds 1,563 upstream Rust lines plus 29 net local lines outside the
TUI, including a 22-line test block. Its original platform files exceed 500
lines. This is dependency maintenance, not source reduction. TUI source remains
167,968 lines and the broader rewrite is unfinished. Windows and macOS are
unverified. Raw samples, source provenance, comparisons and commands are in
[`evidence/tui-rewrite/poll-timeout`](evidence/tui-rewrite/poll-timeout).

## Assistant-part assembly replacement

The 401-line assembler consumes tool sections, builds fallback text only when
needed, and checks rendered prefixes without concatenating temporary Strings.
Stable sequence sorting replaces manual insertion indices. The old assembler
and argument bundle are removed; the surrounding turn-header and edit-coalescing
code remains. The change removes 351 production lines and three test-formatting
lines. Whole TUI source is 167,614 lines, a 7.85% reduction.

All 1,666 tests, 555 complete frame records, 733 exact-clock chat/tool captures
and seven gated PTY checks pass. Six paired xterm screenshots match. Independent
source review confirms fallback/event order, live suffixes, reasoning identities
and error placement. The initial collector reserved spare vector capacity;
measurements caught its roughly 288 KiB heap increase over 1,000 turns. Explicit
output capacities remove that regression. Initial samples remain published.

Final streaming p99 is 996 µs versus 993 µs for the preceding build; resize p99
is 2,751 versus 2,766 µs. Allocated bytes fall 0.10% and 0.08%, with essentially
unchanged peak heap. All fourteen frozen renderer limits pass. This is a code
simplification with small allocation savings, not a substantial speed or memory
improvement. Sustained typing/burst CPU and the broader rewrite remain open.
Raw evidence and commands are in
[`evidence/tui-rewrite/assistant-parts`](evidence/tui-rewrite/assistant-parts).

## Durable settlement suffix

Plain durable turns now convert the changed suffix and retain prepared layouts
for a proven prefix of completed text turns. Complex ownership, tools,
permissions, live fragments, rewinds and task resurrection still use full
conversion. Canonical validation and coordinator contracts are unchanged.
Local echoes and session resets now explicitly invalidate the affected layouts;
the recorded first-empty-view mention delay remains documented as R15.

All 1,666 tests, 555 frame records, 733 controlled animation frames and seven
gated PTY checks pass. Six xterm screenshots match. Existing settlement and
`/new` tests were extended, with red/green evidence and pinned-original checks.
For the new 1,000-turn settlement workload, CPU falls from
65.85 to 1.2 ms/frame and allocated bytes from 37,900,677,305 to
347,790,939. Peak heap is essentially unchanged. All nineteen frozen
checks pass; these renderer/encoder measurements are not end-to-end latency.

The slice adds 278 production and 42 unit-test lines. Whole TUI source is
167,934 lines, a 7.67% reduction from the original. Full-history
eligibility scans, complex-turn conversion and the old state engine remain;
this does not complete their replacement. Evidence and limitations are in
[`evidence/tui-rewrite/settlement-suffix`](evidence/tui-rewrite/settlement-suffix).

## Edit presentation ownership

One reducer now owns proposed, applied and rejected edit transitions for live
input and retained histories. Both duplicated transition blocks are removed.
Complete-history reconstruction no longer copies an older tool's edit back over
the event-derived result. This fixes R16. Capped inline child views keep the
existing carry behavior because their inspection buffer may have discarded the
proposal; the remaining tool timestamp and identity enrichment is unchanged.

Two behavioral checks cover discarded applied/rejected results after a real user
rewind point, fresh replay, and proposal metadata surviving a capped child slice
and a later rewind. The original reproduces R16 and passes the capped-slice
contract. All 1,668 deterministic tests, 555 reference records, 733 controlled
animation frames and seven gated PTY checks pass. Six xterm screenshots match.
The real xterm rewind/click-burst workflow and terminal restoration checks pass.

All nineteen frozen performance checks pass across 84 serial release samples.
Streaming p99 is 989 µs versus 981 µs before this change; resize p99 is 2,740
versus 2,729 µs. Allocations are essentially unchanged. These are general
renderer/encoder workloads, not an edit-throughput or end-to-end speed claim.
The change removes 45 production lines and adds 117 unit-test lines, plus 129
integration-test lines. Whole TUI source is 168,006 lines, 7.63% below the
original. The source-reduction target, remaining state/formatter replacement,
polling removal and runtime resource targets are unfinished. Evidence and
commands are in
[`evidence/tui-rewrite/edit-owner`](evidence/tui-rewrite/edit-owner).

## Tool-row assembly replacement

Tool-row construction now fills one section and applies shared detail and subtitle
rules. The nested six-value tuple builder, duplicated output construction, six
uncalled subagent helpers and unreachable todo branches are removed. Diff/error
helpers retain their behavior in focused files. R17 records the removed eager
todo artifact read. The todo pane's own parser remains.

Production assembly falls from 1,435 to 1,063 lines. Every replacement file is
below 500 lines. Including moved tests and one removed import, TUI source falls
by 379 lines to 167,627, 7.84% below the original. Five existing checks move without
changed assertions. All 1,668 deterministic tests, 555 reference records, 733
controlled animation frames and seven gated PTY checks pass. Six xterm screenshots
match pixels/cells/modes. The real rewind/click workflow and restoration checks
also pass, with the documented output-failure limitation.

A new tool-disclosure workload uses the same fixture on original, preceding and
candidate release executables. Its six limits were frozen before implementation;
all pass. Allocations fall 0.38% from the preceding build, while memory and CPU
are essentially unchanged. Candidate p99 is 15.45 ms versus 15.16 ms immediately
before this change and 12.05 ms on the original. The original-relative tool-heavy
latency/CPU gap remains open. These forced projection/renderer measurements do
not establish input-to-visible-response latency or a speedup for this slice.

The remaining state/formatter replacement, source target, polling removal,
sustained runtime resource work and final feature review are unfinished. Raw
samples, reproduction commands, source receipts and limitations are in
[`evidence/tui-rewrite/tool-assembly`](evidence/tui-rewrite/tool-assembly).

## Tool-disclosure selection cost

Release profiling attributed about 6.9 ms of each disclosed tool frame to preparing
selection rows. The shared row builder now copies printable ASCII spans in row-sized
slices; Unicode/control spans retain grapheme handling. Prepared selection ownership
remains unchanged, preserving bounded drag queries on long surfaces. No cache or
index is added, and no intended UI behavior changes.

All 1,668 TUI tests, seven gated PTY checks and quality checks pass. A temporary
independent old/new oracle matches 41,120 row projections. The 555 reference records,
733 animation records and six xterm captures remain exact. Paired real-terminal
selection captures preserve highlighting, terminal restoration and resource cleanup.

Final paired tool p99 falls from 15,639 to 10,031 µs and CPU from 10.70 to 7.10 ms per
frame, closing the original-relative disclosure gap. Allocation/RSS savings are
small. All six tool limits and fourteen existing general-workload limits pass;
measurements exclude keyboard/emulator latency. Source grows by 32 lines, leaving
the total only 7.82% below the original. The remaining implementation replacement,
sustained runtime targets and final feature/removal review are still outstanding.
Evidence is in [`evidence/tui-rewrite/tool-layout`](evidence/tui-rewrite/tool-layout).

## Borrowed tool detail painting

Tool rendering now shares ordinary headers and reads nested detail blocks directly.
Synthetic cloned tool sections and the unreachable nested-card, transcript Markdown/todo
and ignored bash-hint paths are removed. Task/shell interactions and stable formatting
rules remain. A shared hit-bound guard avoids measuring non-clickable shell rows.

All 1,668 TUI tests, seven gated PTY checks and quality checks pass. The 555 reference
records, 733 animation frames and additional body/order/interaction captures are exact.
Six paired xterm tool captures and actual-runtime selection captures match, with terminal
restoration and cleanup confirmed. All twenty existing release limits pass. Tool p99 is
8,939 µs versus 9,056 immediately before this change; allocated bytes are
7,927,932,588 versus 7,928,203,764. These measure projection/rendering
and encoding, not keyboard-to-screen latency.

The change removes 577 lines; replacement files stay below 500 lines. Whole TUI source
is 167,082 lines, only 8.14% below the original. State/text replacement, runtime resource
and polling work, and final feature/removal review remain unfinished. Evidence is in
[`evidence/tui-rewrite/tool-paint`](evidence/tui-rewrite/tool-paint).

## Interruptible Unix input wait

The terminal reader now uses Crossterm's existing wake pipe to join on shutdown
without a periodic Unix timeout. One read per readiness event keeps incomplete
UTF-8 and paste interruptible; EOF returns a typed error. A small vendored API
exposes the existing parser and blocking wait without starting an EventStream
worker. Raw mode, input normalization and bounded delivery remain unchanged.

Real PTY checks change roughly 20 idle reader wakeups per second to zero. Partial
input and hangup shutdown complete in about 10 ms, and preserved handoffs restore
terminal state. Both builds already record zero CPU ticks and redraws in six
serial eight-second idle samples. Three paired browser runs pass all six frozen
latency limits. Eight screenshots match exactly; one resized baseline capture
has a documented 53-versus-54-second label difference confined to two digit cells.

All TUI tests, seven gated PTY checks, workspace Clippy and suite gates pass.
The workspace run still has 20 CLI failures, all reproduced on the predecessor.
Isolating global configuration leaves the same five stale fixture failures in
both versions. Their raw results are retained; the full suite is not reported
green. No user configuration or backend implementation was changed.

This adds 49 owned TUI lines and 66 local dependency lines. The 12,332-line
upstream copy is additional maintenance, not source reduction. Whole TUI source
is 167,131 lines, only 8.11% below the original. The non-Unix timed path and other
platform verification remain open, along with state/text replacement, sustained
resource targets, startup cadence and final feature/removal review. Evidence is
in [`evidence/tui-rewrite/reader-wake`](evidence/tui-rewrite/reader-wake).

## Focus transition replacement

An 82-line focus module replaces the duplicated forward/backward handlers and
normalization. Unreachable PostRun and replay operator-rail branches are removed.
Startup welcome selection, drawer transitions, remapped shortcuts and Help's
saved focus retain their behavior. Production shrinks by 137 lines; extending two
existing behavioral tests adds 80 test lines. Whole TUI source is 167,047 lines,
only 8.16% below the original.

All 1,668 TUI tests and seven gated PTY checks pass. Fresh before/after runs match
543 complete frames and ANSI outputs, 288 diagnostic transitions, and ten actual
PTY/xterm screenshots with identical cells, styles and cursor states. Both browser
runs restore terminal state and clean up their processes and sockets. Clippy,
workspace compilation and suite gates pass. This consolidation makes no new
resource improvement claim; the remaining state engine and full rewrite targets
are unfinished. Evidence is in
[`evidence/tui-rewrite/focus`](evidence/tui-rewrite/focus).

## Unreachable post-run controller removal

The retired handoff's private selection controller and three state fields are
removed. Its key path was guarded by an always-false lifecycle query; the flags
were write-only or used solely inside that path. Public contracts, live empty-state
geometry, replay failure text and active session navigation remain unchanged.

All 1,668 TUI tests, seven gated PTY checks and quality checks pass. The existing
completion tests reject a temporary mutation that restores the retired handoff.
No tests or snapshots changed, and no resource improvement is claimed. This removes
133 production lines, leaving 166,914 TUI source lines, only 8.23% below the original.
The full rewrite remains unfinished. Evidence is in
[`evidence/tui-rewrite/post-run-cleanup`](evidence/tui-rewrite/post-run-cleanup).

## Composer text projection

Atom text now writes directly into one output string. Rendering and suggestion
updates also reuse the current draft where the old paths reconstructed identical
bytes. This adds one net production line and preserves the public editor contract.
All 1,668 TUI tests, seven gated PTY checks and quality checks pass. Nine paired
actual-runtime xterm screenshots and complete terminal states match, with cleanup
and terminal restoration confirmed.

The fixed long-draft release workload reduces malloc calls by 38%, allocated bytes
by 2.1%, and CPU from 0.44 to 0.38 ms/frame in the confirmation pair. Its seven
frozen limits pass. The short-draft p99 limit remains unmet in both builds:
188 µs before and 187 µs after, against 179.3 µs. Both paired runs and the original
limits are retained; the cause of that initial-baseline timing shift is unverified.
These are input/render/encoding measurements, excluding terminal-emulator latency.
Duplicate editor state and undo history, the source target and full rewrite remain
unfinished. Evidence is in
[`evidence/tui-rewrite/composer-text`](evidence/tui-rewrite/composer-text).

## Shared undo snapshots

Adjacent edits now share immutable snapshots when their complete buffer, cursor,
selection and prompt history match. Restoring shared values preserves independent
editing; public owned snapshot APIs, delete grouping and redo invalidation retain
their behavior. This adds 38 production lines in two small files. Whole TUI source
is 166,953 lines, only 8.21% below the original; the source target remains unmet.

The identical long-draft release workload cuts peak heap from 21.9 MB to 12.0 MB
(45.2%) and RSS from 40,608 to 25,248 KiB (37.8%). CPU remains 0.38 ms/frame.
All fourteen original/current effective limits pass in this paired run. Earlier
short-draft latency failures remain recorded and their cause remains unverified;
this passing pair does not establish a lasting resolution. Unique undo history
is still unbounded, and other editor mirrors remain.

All 1,668 TUI tests, seven gated PTY checks and quality checks pass. Fifteen paired
actual-runtime xterm captures match exactly, including undo/redo and restored
selection, with natural exit and terminal/resource cleanup. Capture preparation
also exposed the existing Ctrl+Y default-binding conflict with AllowPermission;
it is documented and left unchanged. The working Ctrl+Shift+Z binding exercises
redo. State/text replacement, sustained targets and the full rewrite remain open.
Evidence is in [`evidence/tui-rewrite/undo-sharing`](evidence/tui-rewrite/undo-sharing).

## Test-only completion stub removal

Removed 349 lines of completion code and ten tests that existed only in test
builds and called only that stub. The remaining 293-line file matches the old
non-test prefix exactly. No production behavior, public contract or snapshot
changed. Existing checks exercise real AppState and completion-controller paths:
a temporary ignored-insertion mutation fails two of those checks while all ten
stub tests still pass. All 1,658 remaining TUI tests and quality checks pass.

This is test maintenance removal, with no new runtime performance claim. Whole
TUI source is 166,604 lines, including tests under `src`, only 8.4% below the
original. The source target and full implementation replacement remain unmet.
Evidence is in [`evidence/tui-rewrite/completion-stub`](evidence/tui-rewrite/completion-stub).

## Atom parsing and composer reflow

Text construction and insertion now share a parser over borrowed line slices and
lazy graphemes. Atom-ID validation replaces repeated scans with set membership,
preserving the first conflicting ID in errors. Reflow moves its validated atoms,
and bordered rendering reuses its prepared editor model. Public contracts,
geometry and text behavior remain unchanged. This removes 23 production lines;
whole TUI source is 166,579 lines, only 8.41% below the original.

All 1,659 TUI tests, seven gated PTY checks and quality checks pass. Sixteen paired
actual-runtime xterm captures match exactly with terminal/resource cleanup. An
initial capture crossed the 800 ms Escape confirmation deadline between its cell
snapshot and PNG; it is retained. Explicit state waits resolve the fixture race,
and the corrected journey verifies the cursor resets after two acknowledged
Escape events. The cleared text returns as an italic history suggestion; captured
styles and the next unprefixed paste distinguish it from retained draft text. No production timer or expected cell value was changed.

Two identical release comparisons reduce long-draft CPU from 0.38 to 0.32 ms/frame
(15.8%), malloc calls by 7.7% and allocated bytes by 6.4%. All seven long-draft
bounds pass. Short-draft p99 fails both comparisons: 196 and 186 µs against the
unchanged 179.3 µs bound. All 62 runs, including baseline, intermediate and separate
CPU-affinity diagnostics, remain available. The diagnostic does not establish a
cause or clear the failed gate. These input/render/encoding timings exclude
terminal-emulator latency and do not establish the sustained runtime target.
The short-draft gate, state/text replacement and full rewrite remain unfinished.
Evidence is in [`evidence/tui-rewrite/atom-buffer`](evidence/tui-rewrite/atom-buffer).

## Prompt editing controller

Borrowed string scans replace temporary character vectors in word and line
navigation. Shared movement, deletion and undo/redo paths preserve selection,
newline handling, overlay updates and the typed-editor/fallback-history order.
The controller shrinks from 293 to 227 lines; two public keyboard tests replace
26 private tests. Whole TUI source is 166,128 lines, 8.66% below the original;
the 90,941-line target and full state/text replacement remain unfinished.

All 1,635 TUI tests, seven gated PTY checks and quality checks pass. Twenty-three
paired actual-runtime xterm captures match exactly, with natural exit and terminal
and resource cleanup. A failed preparation expected three-line paste to collapse;
existing behavior expands it, and the corrected fixture waits for that text. R19
records the existing Ctrl+Home binding conflict without changing shortcut policy.

The fixed navigation workload passes all seven frozen limits: malloc calls fall
by 517 and allocated bytes by 1.7 MB (0.52%) across 510 actions. CPU remains
0.32 ms/frame. The timing covers input handling, rendering and ANSI encoding,
excluding terminal-emulator latency. Four baseline and eight final paired runs
are retained. Earlier typing latency failures and the sustained-runtime targets
remain open. Evidence is in
[`evidence/tui-rewrite/prompt-editing`](evidence/tui-rewrite/prompt-editing).

## Undo journal

The atom editor now stores changed atom ranges and state metadata around one
complete tip per undo/redo stack. Bridges preserve changes between recorded
edits, and immutable entries share payloads across editor clones. Undo depth,
delete grouping, atom allocator state, cursor, selection, history and the public
snapshot contracts remain intact. This adds 120 production lines; whole TUI
source is 166,248 lines, 8.60% below the original and above the reduction target.

All 1,636 TUI checks, seven gated PTY checks and quality checks pass. Thirty-four
paired actual-runtime xterm frames match exactly, including deep undo, redo
branching and single-step restoration of grouped deletion. Terminal and resource
cleanup pass. The earlier eight-frame deep-undo capture is also retained.

Long typing reduces peak heap by 81.2% and RSS by 60.4%; CPU stays at 0.32 ms/frame.
Deep undo/redo reduces peak heap by 81.2%. Grouped deletion allocates 4.4% more
bytes and takes 10% more CPU, within its frozen aggregate limits. The last hundred
deletions are 15.8% slower at the median, so grouping remains a measured cost.
The final comparison passes 26 of 28 limits: short typing fails p95 at 140 µs
against 135.3 and p99 at 197 µs against 179.3. All 80 raw reports, including the
earlier passing comparison, remain available. Timing excludes terminal-emulator
latency. Earlier latency failures, source/state replacement and sustained-runtime
targets remain open. Evidence is in
[`evidence/tui-rewrite/undo-journal`](evidence/tui-rewrite/undo-journal).

## Composer projection

Runtime composer painting and pointer geometry now borrow atoms and project only
the text, badges and ghost suffix they consume. The full owned view-model clone,
editor clone, reflow and mirror adapter leave that path. Public owned models and
validation remain. Row sizing measures borrowed Unicode width directly, removing
per-grapheme Ratatui line allocations. Atom row budgets and string paint/cursor
geometry retain their distinct behavior. R20 records the unchanged ghost defect
when public prompt mirrors diverge from the editor.

All 1,635 TUI checks, seven gated PTY checks and quality checks pass. Existing
public checks cover completion badges, narrow attachment geometry and divergent
ghost text; a predecessor mutation demonstrates the attachment assertion fails.
Thirty-eight paired PTY/xterm frames match exact PNGs, cells, styles and cursor,
with terminal restoration and resource cleanup. Production line count is
unchanged. Removing one structural test brings TUI
source to 166,232 lines. The retained 1,356-line layout engine remains unfinished.

Long-draft allocation bytes fall 77.7%, malloc calls 97.7% and CPU 31.3%.
Grouped-deletion CPU falls from 0.44 to 0.30 ms/frame. The corrected comparison
passes 25 of 28 bounds. Short-draft p95 is 168 µs against 133.1, p99 is 200 µs
against 179.3, and CPU is 0.12 ms/frame against 0.11. The candidate is slower
than its paired short-draft reference; the cause remains unverified. All 80
raw reports and the first candidate's failed comparison remain available.
Measurements exclude terminal-emulator latency. Evidence is in
[`evidence/tui-rewrite/composer-projection`](evidence/tui-rewrite/composer-projection).

## Composer viewport replacement

The string composer layout now wraps borrowed slices and stores absolute scalar
starts, then allocates only visible strings. The old per-grapheme records and
all-row string construction are removed. Explicit LF segments preserve dropped
spaces, hard breaks, trailing empty rows and cursor endpoint rules. Atom row
budgets remain separate. R21 records the preserved public-mirror CRLF mismatch.

The engine shrinks from 231 to 111 lines; deleting its record type and alias
removes nine more lines. TUI source totals 166,103 lines, still above the target.
An existing public rendering check adds eight frame/cursor cases. Its predecessor
mutation fails as expected. A temporary independent oracle verifies 174,696
complete viewport results before removal from the normal test tree. All 1,635
TUI tests, seven gated PTY tests and quality checks pass. Forty-two paired
PTY/xterm frames match exact PNGs, cells, styles and cursor; restoration and
resource cleanup pass.

Long typing allocation bytes fall 12.5%; CPU remains at 0.22 ms/frame. Short
p95/p99 are 122/127 µs and CPU is 0.10 ms/frame in this comparison. Earlier
latency failures remain recorded. The candidate passes 27 of 28 frozen limits.
Long typing malloc calls fall 2.4%, missing the 10% target at 104,409 calls
against 96,273. All 48 raw performance reports remain available. Measurements
exclude PTY/emulator delivery. Evidence is in
[`evidence/tui-rewrite/composer-viewport`](evidence/tui-rewrite/composer-viewport).

## Composer row measurement replacement

Atom wrapping now yields borrowed slices and stored widths. Public `wrap()`
still materializes ID rows; runtime presentation counts up to its visible cap,
while completion geometry counts every row. Frame sizing shares the string
viewport's soft-wrap primitive and stops at its cap. The old per-grapheme height
vector and duplicate wrapping algorithm are removed. Distinct atom/string and
LF/CRLF policies remain, including R21.

Two existing behavioral checks cover zero-width and over-wide atoms, saturation,
newline ownership and hidden completion rows. The predecessor mutation fails as
expected. Three temporary differential checks verify 222,262 exact geometry
results, then leave the normal test tree. All 1,635 TUI checks, seven gated PTY
checks and quality checks pass. Forty-two paired PTY/xterm captures match exact
PNGs, cells, styles and cursor; restoration and resource cleanup pass.

Production falls by 45 lines to 166,058. The retained frame-layout file is still
1,306 lines. Long typing allocation bytes fall 72.6%, undo 54.1% and deletion
56.1%; CPU is unchanged at tick resolution. Only 25 of 28 frozen limits pass:
long malloc calls fall 5.4% and miss their 10% target, while short p95/p99 of
161/195 µs exceed 133.1/179.3 µs. Long p99 also rises within its frozen bound.
Timing variation remains unresolved; this does not establish improved latency.
All 48 raw performance reports remain available, with no gate relaxation or
unchanged rerun. Timings exclude PTY/emulator delivery. Evidence is in
[`evidence/tui-rewrite/composer-rows`](evidence/tui-rewrite/composer-rows).

## Composer painting replacement

Styled composer runs borrow source slices, and the bordered painter reuses its
body span vector and measured prefix/gutter. Paragraph clipping, ghost text,
cursor placement, and disabled/placeholder/collapsed-paste behavior remain.
The unused document fallback and its metadata module are deleted: replay returns
before that dispatcher, and its other two variants delegated to the bordered
painter. Live badge and provider-label checks remain.

Production falls by 443 lines; an existing painted-selection check grows by
32 lines. The TUI source tree totals 165,647 lines. The legacy dispatcher and
test file still exceed 500 lines. A style-replacement mutation fails the new
combined mention/selection assertion. All 289,152 frozen complete-Line
comparisons match. All 1,635 TUI checks, seven gated PTY checks and quality
checks pass after extracting the new assertion stage to meet Clippy's complexity
limit. Forty-two paired PTY/xterm frames match exactly in PNGs, cells, styles and
cursor; terminal restoration and resource cleanup pass.

Long typing allocation bytes fall 4.5% and malloc calls 4.2%, missing both frozen
5% targets. The candidate passes 26 of 28 limits. All latency bounds pass in this
comparison, with short p95/p99 at 121/128 µs; earlier failures remain recorded.
CPU is unchanged at process-tick resolution. All 48 raw reports remain available;
there was no gate relaxation or unchanged rerun. Timings exclude PTY/emulator
delivery. Evidence is in
[`evidence/tui-rewrite/composer-paint`](evidence/tui-rewrite/composer-paint).

## Verification sequence

1. Retain the executable, record the full behavior matrix, and capture terminal
   cells, styles, screenshots, input sequences, and controlled animation times.
2. Measure identical release workloads. Record raw samples and set acceptance
   limits before optimization. Separate renderer/encoder costs, PTY delivery,
   and browser-visible latency.
3. Add behavioral checks that fail when the replacement lacks the recorded
   behavior. Implement small vertical sections and commit code with its checks.
4. Remove the original state engine, renderer, event loop, obsolete structural
   tests, and temporary adapters. Keep assets and required public contracts.
5. Run the comparison matrix, scoped and workspace checks, real PTY/xterm
   exercises, and resource benchmarks. Explain every difference.
6. Resolve the independent review findings and obtain a final review.

The rewrite is not complete while a required interaction, unexplained visual
difference, resource measurement, or review remains outstanding.
