# Remove the unused status fallback

Based on `b3202dfc`. Public status actions assign the interactive dashboard before
opening its private visibility flag. Closing clears both, and a failed refresh
keeps the prior dashboard. `render_app` paints that full dashboard before overlay
dispatch. The old centered status fallback therefore had no public input/API
route. Its private rows, temporary summaries, tests and snapshot are removed.

The actual dashboard now counts probe presence directly and counts distinct
literal edit paths without reading file contents. The 68-probe denominator,
empty optional strings, unavailable probes, replay/settings distinction, banner
classification, sanitization, truncation and configured MCP availability remain
unchanged. Pane geometry and painting are retained in two files of 414 and 280
lines. This is cleanup during migration, not a claim that the dashboard's whole
state engine and renderer have been replaced.

The source tree shrinks by 3,867 lines, including removal of 55 private-helper
tests. One existing public dashboard journey gains 36 lines; the integration
oracle gains four. No new test is added. `removed-tests.json` lists the removed
tests, and the final list has no other removals. The tree now contains 556 Rust
files / 169,273 lines under `src`, including inline tests, and 118 files / 27,750
lines under `tests`.

Three uncompiled orphan files are also removed: `app/dashboard_roster.rs`,
`app/screen_mode.rs`, and `ui_transcript_reasoning_selection_tests.rs`. Neither
module/caller searches nor the existing all-feature debug dependency files
reference them. The real `dashboard_roster` and `transcript_identity/screen_mode`
modules and their public APIs remain. The unused private visibility setter is
removed. No backend or dependency changes.

## Verification

All 1,684 TUI tests pass, with seven skips. Scoped all-target/all-feature Clippy,
workspace check, formatting and suite gates pass. The original 539 golden
records remain byte-identical. The pinned original supplied four new `d`/details
checkpoints at 40×24, 80×24, 120×40 and 160×50 before production edits. All 543
candidate cell/cursor/intent frames match the preceding source; the four new
frames and ANSI captures match the original exactly. Existing R8 corrections
remain explicit in the oracle. All 51 viewer ANSI frames also remain exact.

The extended public dashboard journey passes on the preceding source. It checks
the displayed 68-probe count for an empty, absent and unavailable probe, and
checks that three edit events for `notes.txt`, `notes.txt` and `./notes.txt`
display two edited paths. A deliberate mutation that treated Landlock as unbound
fails on the rendered count. Restored source passes the full suite. This is a
mutation check, not a newly discovered original defect.

The four new ANSI frames were replayed through the same xterm/browser/font setup
and visually inspected. All four PNGs are byte-identical and every cell/display
field matches. Browser `renderCount` differs by one at 40×24 and 160×50; the
comparison retains those fields. These are static reduced-motion captures,
not PTY or end-to-end latency measurements.

No resource improvement is claimed from a new timing run. Plan-list presence
still reads disk through `plan_view_rows()` during summary construction. Moving
that read into owned plan preparation remains necessary for paint purity.
Whole-rewrite acceptance remains incomplete.

## Reproduction

```sh
HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-dashboard-final HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-dashboard-journeys cargo nextest run --profile ci -p harness-tui --all-features -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
node scripts/qa/render-recorded-frames.mjs /tmp/tui-dashboard-xterm-reference .omo/evidence/tui-rewrite/dashboard-cleanup/reference --source-root /home/urbanbreach/.codex/worktrees/tui-reference/agent-harness
node scripts/qa/render-recorded-frames.mjs /tmp/tui-dashboard-xterm-candidate .omo/evidence/tui-rewrite/dashboard-cleanup/candidate
```

Each xterm input directory contains the four `dashboard-details-*.ansi` frames
and `producer.json` naming the AppState slash/Down/d inputs and static fixture
clock. `frames` holds the identical ANSI data; `browser` retains both outputs.

`reference-fixture` holds the recording test and its two support files. The test
is the recorder from `36455057` with only the four details checkpoints added;
its original-only settled-frame allowance handles recorded R3. Install those
fixtures under `crates/harness-tui/tests` in the pinned checkout and run:

```sh
HARNESS_TUI_RECORD_REFERENCE=1 HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-dashboard-reference cargo nextest run --profile ci -p harness-tui --all-features --test rewrite_reference_test -j1
```

The recorder verifies the original TUI/core sources and lockfile against the
pinned base before writing. Only the four additional records were inserted into
the candidate golden file; all earlier records were checked for exact equality.
`source.patch.gz`, `source.json`, `binaries.json` and the file manifest identify
the implementation, tests, captures and tested executables.
