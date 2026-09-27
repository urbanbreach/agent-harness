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

The opt-in Linux PTY run executes 22 checks: 18 pass and four fail on the original
source (P0-02 navigation error marker, P0-03 Markdown visibility, P1-02 Settings
geometry, and P1-04 responsive readiness). These failures are being classified
against their captured screens. The separate CLI happy path passes through
prompt, permission, tool edit, resume picker and quit.

## Behavior inventory

The inventory comes from `app`, `keybindings`, the terminal runtime, the existing
integration tests, and the Grok Build reference in `inspirations/grok-build`.
Grok's additional features do not imply that Harness implements them.

| Area | Behavior to preserve | Existing evidence |
| --- | --- | --- |
| Shell | Home, empty/live session, replay, completed session; model/context chrome; narrow layouts | `deterministic_render_test`, `shell_topology_contract_test`, `startup_polish_test` |
| Motion | Startup reveal, shimmer, live reasoning/tool pulse, completion rail, reduced motion | `grok_parity_render_test`, `motion_demand_app_state_test`, P1-03/P1-04 PTY fixtures |
| Composer | Grapheme editing, selection, undo/redo, multiline, paste preview, history, stash, attachments, file/agent/resource mentions | Composer tests, `production_composer_reachability_test`, P0-04 PTY fixtures |
| Submission | Send, queue, interject, cancel-and-replace, queue editing/reorder, shell command mode | Prompt queue tests, live turn tests, CLI intent tests |
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

The new public-boundary oracle currently records 507 frames, including complete
cell colors, underline colors, modifiers, cursor positions, input sequences, and
emitted intents. It exercises populated plans, memory, settings, model selection,
stash, session resume, replay, and permission submissions as well as empty states.
The independent review identified missing coverage for child-session navigation,
attachments/mentions, queue editing, mouse routing, rewind acknowledgement, and
preserved-terminal handoff. Those remain required before source removal.

## Existing defects and decisions

These findings precede the replacement. Confirm each with a behavioral check
before changing the implementation.

| ID | Finding at reference commit | Decision |
| --- | --- | --- |
| R1 | `runtime.rs` initializes fallible presentation/scheduling telemetry after terminal setup but before installing `TerminalRestoreGuard` | Fix in the replacement. An initialization error must restore the terminal. |
| R2 | `teardown_terminal_session` returns on its first failed restore operation | Fix in the replacement. Attempt the remaining restoration operations and return the error. |
| R3 | Detaching completed history at 40×24 changes the viewport between the first and second paints, without an input or clock change | Record both buffers. Compare the settled reference viewport; the replacement must resolve navigation before painting. Candidate checks require its first paint to match. |

`check-tui-restoration.py` reproduces R1 with a trace path whose parent is a file:
the original exits with raw mode and alternate-screen/paste/mouse modes enabled.
Normal exit restores all checked modes. A write failure during initial setup
restores termios; recovery from a failure partway through teardown still needs
an injected-writer check for R2.

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
