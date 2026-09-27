# Focus transitions

The predecessor is `7e720aebb22f1e062f0e1bf91d8c31bb69b8ceb4`.
The 82-line `app/focus.rs` replaces duplicated forward/backward focus handlers
and normalization. It removes 137 production lines, including unreachable
PostRun and replay operator-rail branches. The lifecycle returns no PostRun
state, and the operator rail is never interactive in replay. Public contracts,
keyboard bindings, overlay priority and backend code are unchanged.

Startup normalization still toggles composer/menu focus and updates the welcome
selection. Live Help moves terminal focus to Details; replay retains a visible
terminal's focus. Closing Help restores the saved focus directly. Default replay
Tab keys remain on the transcript, while remapped reverse-focus can reach the
terminal. Drawer transitions retain their original asymmetry.

Two existing behavioral tests were extended, without adding tests. They cover
keyboard focus, visible drawer state, replay edit rejection, and Help opened and
closed from a terminal pane. The extended tests pass against the predecessor.
The replacement's empty transition stub fails the focus journey in `red.log`;
the implementation passes it. Test source grows by 80 lines, including 27 outside
`src`. Total TUI `src` is 167,047 lines, only 8.16% below the original 181,882.

## Verification

- All 1,668 deterministic TUI tests pass, with seven excluded by configuration.
- Seven explicitly gated P0-04/P1-04 PTY checks pass.
- TUI Clippy, workspace compilation, formatting and suite gates pass.
- Fresh predecessor/candidate runs match all 543 complete frame records and their
  ANSI output. `frames.tar.gz` contains both runs; `frames.json` records hashes.
- A standalone diagnostic compares the actual transition method bodies across
  288 combinations. Predicates mirror the traced lifecycle/terminal methods.
  This supplements the public key tests; it does not replace them.
- Ten paired actual-runtime PTY/xterm screenshots are byte-identical. Terminal
  cells, styles, cursor, modes and scrollback also match. Browser callback counts
  differ because observer scheduling and output grouping vary; the raw values
  are retained in `browser-comparison.json`.
- Both browser runs exit naturally with termios and protocol modes restored,
  child process groups stopped, sockets closed and browser profiles removed.

The browser uses the same reduced-motion fixture, 140×40 geometry, Chromium,
xterm.js and font for both builds. `browser.tar.gz` contains screenshots,
terminal snapshots, raw ANSI, input sequences, executable receipts and cleanup
reports. The initial predecessor run was repeated to add complete cell snapshots;
the published pair uses the same capture script. No screenshot was masked or
snapshot expectation changed.

This is code consolidation, with no new CPU, memory or latency improvement claim.
The existing rewrite resource limits remain unchanged. Full workspace tests were
not repeated for this focus-only change; the preceding slice documents the
pre-existing CLI failures. Non-Linux environments, the remaining state engine,
source reduction and sustained-resource targets remain unfinished.

## Reproduction

Run these commands from the repository root:

```sh
cargo nextest run --profile ci -p harness-tui --all-features
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --ignore-default-filter -E 'binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'
cargo build --release -p harness-tui --all-features --example rewrite_probe
node docs/evidence/tui-rewrite/focus/capture-focus.mjs target/release/examples/rewrite_probe .omo/evidence/tui-rewrite/focus/reproduced
```

For the pure transition diagnostic, copy `compare.py` to a scratch directory and
run it from the repository root. It reads the predecessor from Git and the
current candidate, compiles standalone oracles with `rustc`, and writes raw
transition results beside the script. `checks.json` records the other commands
and environments. `checks.py` records the bounded before/after test procedure;
it temporarily restores the preceding key handler and restores the candidate in
`finally`. The original reference checkout and executables remain retained.

`sources.json`, `source.patch`, `binaries.json` and `files.json` identify the
implementation, executable and evidence bytes reviewed.

Independent review approved this bounded replacement after verifying the source,
raw comparisons and cleanup receipts. `review.json` records the decision; the
full rewrite is not approved or complete.
