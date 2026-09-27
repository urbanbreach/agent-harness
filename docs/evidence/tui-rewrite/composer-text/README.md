# Composer text projection

The predecessor is `92bd235d0d8a70e84ee3a7f333fcbb7a10f8aa30`.
Atom text now appends directly to one output string instead of allocating a
string per grapheme. Typed mention/attachment placeholders retain their format.
The application reads its prompt mirror directly where the former equality
branch returned those same bytes either way. Suggestion updates clone the draft
just reconstructed for the queue, avoiding a second atom traversal. Collapsed
paste, cursor geometry, editing, undo and public contracts are unchanged.

The change adds one net production line. Existing atom coverage now checks typed
placeholder text; no new test function is added. Seven atom tests pass on the
predecessor. Temporarily replacing text projection with an empty result makes
three of them fail; the source was restored before implementation. All 1,668
TUI tests, including the pinned frame/intent comparisons, and seven gated PTY
tests pass. No snapshots were updated.

## Measurements

The existing release fixture has an optional `typing-long` scenario. It pastes
32 repetitions of `plain 界 é 👩‍💻 ` and alternates insertion/backspace for ten
warmup frames and 500 measured frames. After two recorded samples it verifies
that insertion adds `x` and deletion restores the draft; the final draft is also
checked. The ordinary empty/one-character typing scenario remains a control.
Both use zero history, 160×48 geometry and reduced motion.

Before implementation, three timing runs and a separate glibc allocation run
established the limits in `acceptance-before-implementation.json`. The target
is at least 30% fewer malloc calls on the long draft; timing, allocated bytes,
peak heap and RSS have bounded regression limits. The independent reviewer
requested the two intermediate edit assertions to reject ignored input. Both
final executables include that identical fixture. Limits were not reset after
that correction; the initial records and executable receipt are retained.

Both paired sets reduce long-draft malloc calls by 38%. In the confirmation set,
calls fall from 8,775,352 to 5,440,072 and allocated bytes from 380,621,446 to
372,702,031 (2.1%). CPU falls from 0.44 to 0.38 ms/frame, and p99 from 492 to
434 µs. Peak heap and RSS remain effectively unchanged. All seven long-draft
limits pass in both sets.

The short-draft timing gate remains unmet. Its first paired set reports p95/p99
of 175/194 µs against limits of 135.3/179.3 µs; the paired baseline is 140/192 µs.
One confirmation set gives 124/187 µs, versus baseline 123/188 µs. The p99 miss
therefore persists in both builds. The cause of the shift from the initial
baseline is unverified; no all-limits-passed or short-draft speedup claim is made.
Both paired sets are retained in `performance.tar.gz`; root summaries describe
the confirmation set. CPU remains 0.10 ms/frame on that short-draft pair.

Timing samples cover public input handling, frame preparation, painting, diffing
and ANSI encoding to a counting sink. They exclude PTY and emulator latency.
Allocation totals cover the whole process, including fixture construction.
The existing whole-rewrite resource limits remain unchanged.

## Terminal parity

Nine paired actual-runtime PTY/xterm captures cover a Unicode draft, insertion,
deletion, editing within the draft, undo, Escape and collapsed/expanded multiline
paste. Screenshots are byte-identical; cells, styles, cursor, modes and scrollback
match. Observer callback counts may differ and are preserved in the comparison.
Both runs exit naturally, restore termios/protocol state and remove child
processes, sockets and browser profiles.

The browser baseline is the retained focus executable from `410779f3`, identified
in that slice's receipts. Only the separately verified unreachable post-run
controller cleanup lies between it and this slice's predecessor. The release
performance comparison uses the exact predecessor. Both browser runs use the
same Chromium, xterm.js, font, 140×40 geometry and reduced-motion fixture. PNGs,
full terminal snapshots, ANSI output and cleanup reports are in `browser.tar.gz`.

## Reproduction and limits

Run normal verification with:

```sh
cargo nextest run --profile ci -p harness-tui --all-features
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --ignore-default-filter -E 'binary(p0_04_pty_recorded) | binary(p1_04_pty_recorded)'
HARNESS_REWRITE_SCENARIO=typing-long HARNESS_REWRITE_HISTORY=0 HARNESS_REWRITE_FRAMES=500 cargo nextest run --release -p harness-tui --all-features --test rewrite_performance_test --profile perf -j1 --success-output immediate
cargo build --release -p harness-tui --all-features --example rewrite_probe
node docs/evidence/tui-rewrite/composer-text/capture-composer.mjs target/release/examples/rewrite_probe .omo/evidence/tui-rewrite/composer-text/reproduced
```

To recreate the paired measurement from a clean checkout of this change, first
build the release probe as above, then copy `rebuild.py`, `measure.py`, `compare.py`
and `acceptance-before-implementation.json` into one scratch directory. Write
`cargo metadata --format-version 1` to `cargo.json` there. Run `rebuild.py` from
the repository root: it temporarily restores three predecessor source files,
builds the first executable, restores the candidate in `finally`, and builds the
second. Run `measure.py before candidate`, then `compare.py`. The latter exits
nonzero when any frozen limit fails, as it did in both recorded paired runs.
`sources.json`, the source patch, binary receipts and `files.json` identify reviewed
bytes. Quality commands and results are in `checks.json` and `checks-final.json`.

The change does not remove the editor's duplicate state or bound undo history.
Whole TUI source is 166,915 lines, only 8.23% below the original. Full state-engine
replacement, sustained runtime targets, startup cadence, other platforms and
final whole-rewrite review remain unfinished. Existing CLI fixture failures are
recorded in the reader-wake evidence; the full workspace test suite was not rerun.
