# Remove unreachable presentation code

Based on `8e7a3ca0`. An all-feature compiler diagnostic, forced past local
`dead_code` allowances, identified unused presentation paths. Caller searches
confirmed the removed roots have no runtime consumers. This slice deletes:

- The custom ordered-JSON tool-input parser and its four private tests.
- The old per-line diff highlighter, its separate syntax assets and prose-path
  fast-path test. The running diff renderer uses the shared highlighter over
  recorded before/after file content.
- Unused startup view-model helpers, card/secondary-split layout machinery,
  clipboard-warning painter and breadcrumb text helper. The working welcome,
  notice, folder-trust, breadcrumb, live-empty and modal paths remain in place.
- Unused tool status/disclosure/classification and transcript rail helpers,
  plus their obsolete private checks.

The welcome reads the same canonical release-note array directly. Public
`WelcomeLayout::for_area` keeps its signature and behavior while its private
forwarding layer is removed. No backend contracts, dependencies, assets or
runtime authority change. The shared syntax highlighter itself is unchanged.

The net reduction is 1,100 Rust source lines: 817 production and 283 test lines.
`source-counts.json` gives per-file counts and the classification convention.
Three files and 11 obsolete tests are removed; no integration tests are added.
The source tree is now 555 Rust files / 168,089 lines including inline tests.
This is removal of unused code, not replacement of the remaining state engine
or active lower renderers. The whole-rewrite reduction target remains unmet.

## Behavioral coverage

The existing limited-color test now renders an actual structured diff and checks
its nonempty content and span colors. Its old version exercised only the unused
per-line highlighter. Temporarily bypassing foreground quantization in the shared
highlighter makes the migrated test fail (`color-red.log`, `color-mutation.patch`).
That mutation was restored before final validation. The breadcrumb rendering
check still verifies blank-cell style, cached metadata and explicit refresh;
it now samples a known blank cell rather than calling a removed text helper.

All 1,671 remaining TUI tests pass with seven configured skips. Seven gated PTY
checks, all-target/all-feature TUI Clippy, workspace check, formatting and suite
gates pass. The early focused run selected three checks; the final full run
contains all reference journeys and the migrated color check.

All 555 complete frame records match the preceding candidate: 543 main states,
eight plan states and four Unicode-wrapping states. Their ANSI bytes match too.
`matrix.json` records hashes and points to the already published identical raw
buffers; `ansi-comparison.json.gz` records each matching ANSI hash. No new
snapshots are approved, and no browser captures or resource measurements are
claimed for this deletion. The previous slice's timing regressions and unmet
frozen limits remain open.

```sh
cargo rustc -p harness-tui --all-features --lib -- --force-warn dead_code --emit=metadata
HARNESS_TOOL_RUNTIME_HARNESS_DIR=/tmp/tui-unused-presentation-journeys HARNESS_TUI_REFERENCE_FRAMES=/tmp/tui-unused-presentation-matrix HARNESS_TUI_PLAN_FRAMES=/tmp/tui-unused-presentation-plans HARNESS_TUI_WRAP_FRAMES=/tmp/tui-unused-presentation-wrap cargo nextest run --profile ci -p harness-tui --all-features -j1
HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tui --all-features --run-ignored all --test p0_01_pty_recorded --test p0_02_pty_recorded --test p0_03_pty_recorded -j1
cargo clippy -p harness-tui --all-targets --all-features -- -D warnings
cargo check --workspace
cargo fmt --all -- --check
python3 scripts/check-test-suite-gates.py
```

`before-unused.log` and `unused.log` retain the compiler diagnostics. They are
investigation output, not a claim that all remaining warnings should be deleted.
The source hashes and archived patch bind the final change; deleted files retain
their base hashes. The test mutation is evidence of an effective check, not a
newly discovered product defect.

Independent review approved the source, removal scope, migrated test and evidence
for commit. `review.txt` records its scope; `files.json` binds the final artifacts.
