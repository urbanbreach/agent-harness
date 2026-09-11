# Grok Build presentation parity implementation

Implemented the research's A–G presentation and interaction changes with Harness
branding, coordinator authority, and recorded-data replay intact. Verification
completed on 8 September 2026 (Europe/Helsinki). The research folder is preserved.

The [11 September chat/tool follow-up](chat-tool-render-parity.md) supersedes
the chat palette, grouping, disclosure, syntax, and queued-tool projection
results below. It includes a new 504-frame xterm matrix, exact chat-cell
comparisons, and real edit/search/viewer terminal journeys.

Scope: [research synthesis](../20260906-192230/SYNTHESIS.md), compared against
`inspirations/grok-build` at `bc7f02eddd3d84085849dc19ed216f11c23b0571`
(upstream `SOURCE_REV`: `d5a0335a47221e8c9519936cb693e9b6450227ec`).
Harness implementation baseline: `60282c31862bb34e4565b7574d1ef7dbebf1f126`.

Open the [comparison gallery](../.omo/evidence/grok-parity-final/index.html)
for actual xterm.js screenshots and selectable animation samples. The
[evidence index](../.omo/evidence/grok-parity-final/README.md) links the raw
frames, source receipts, interaction journals, cleanup receipts, and check logs.
Evidence is local and ignored by Git; the implementation and capture drivers
remain reviewable in the working tree.

## Completed implementation

- [x] **A — Transcript and viewer.** Stable visual-entry selection independently
  navigates prompts, reasoning, tools and answers. The selected block opens a
  fullscreen viewer with paging, find, selection, copy, and raw/Markdown modes.
  Transcript-wide search, historical sticky prompts with progressive push-off,
  final-answer jumps, and stable singleton context groups are implemented.
  Simple-mode letter input still belongs to the composer; overlays own input.
- [x] **B — Dashboard.** Awaiting/Working/status groups contain multirow parents
  and their children; counts include parents once. A measured bottom peek renders
  Markdown and a real per-session reply editor. Recorded permission/question
  requests and resolutions drive actual controls and group changes. Compact
  views open full review before decisions. Resize/refresh preserve drafts and
  selection; Escape restores the exact transcript anchor, follow state and focus.
  Diagnostic details remain available through `d`.
- [x] **C — Composer and completion.** Ctrl+Enter sends now through one cancel
  followed by one replacement; an empty draft selects the correct queued item.
  Multiline Alt+Enter submits, Bash Enter runs, Ctrl+S/Alt+S stash and restore,
  and running Escape preserves the draft while requesting cancellation.
  Empty Up selects queue/history. Large paste chips expand without sending and
  retain grouped undo. Slash dismissal keeps the query; Shift+Enter does not
  execute, Tab accepts, all ranked results remain reachable, model/effort choices
  dispatch once, and wrapped hit maps separate hover from keyboard selection.
  File completion supports drilldown, paging and line-range selection.
- [x] **D — Secondary surfaces.** Modal chrome is centered, viewport-contained
  and undimmed. Memory supports filtering, value previews, fullscreen and copy.
  Settings supports human labels, Boolean/integer/string/enum editing, complete
  validation before persistence, and cancellation with exact palette return.
  Usage and Extensions have dedicated views over actual recorded/configured data.
- [x] **E — Tools.** Read, skill, empty-file, image and PDF displays use supplied
  metadata. Recorded code preserves indentation, tabs, numbering and 5+3 preview
  geometry. Typed search/web/MCP layouts retain modes, caps and full-view access.
  Diff syntax uses recorded pre-edit context, including scopes outside the hunk.
  A bounded recorded-artifact cache keeps filesystem reads out of painting.
  Command bytes become safe styled cells with SGR and carriage-return overwrite;
  empty output still reveals the command. Subagents retain one-line lifecycle
  rows and child navigation, including singleton context-group behavior.
- [x] **F — Startup and chrome.** Welcome fit measures actual Harness H artwork,
  notes, notices and actions. Controls appear immediately; the visible H uses
  the reference shimmer cadence, with bounded demand-driven scheduling and a
  static reduced-motion profile. Height selects the stacked H where it fits.
  Structured branch/detached/worktree facts avoid reverse-parsing display text.
  Compact density restores on growth. Harness identity appears once and notes
  describe actual Harness functionality.
- [x] **G — Markdown.** Shared preformatted wrapping preserves code indentation
  and tabs with matching source-aware selection. Structural semantics cover
  nested emphasis/links, CommonMark fences, tables/alignment, quotes and math;
  the baseline's missing parser dependency is declared. Open fences stay styled,
  prose supports soft breaks and hyphen wrapping, citation fences resolve syntax
  by extension, and syntax follows Harness theme roles.
- [x] **Verification.** Behavioral tests, full workspace checks, production
  renderer captures, native PTY owners, browser interactions, motion samples,
  reference captures and the release resize benchmark passed.

## Verification results

| Check | Executed result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo check --workspace` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo nextest run --profile ci --workspace --all-features` | 4,587 passed; 10 skipped by the configured suite; opt-in evidence run separately below |
| `scripts/test-lanes.sh quality-gates` | Static suite gates and forbidden-branding gate passed |
| `node --test scripts/qa/*.test.mjs` | 51 passed |
| `bash scripts/harness-qa-dogfood.sh --self-test` | Passed |
| Native startup PTY owner | Passed; 18 real-runtime sessions across 80×24, 120×40 and 160×50, Unicode/ASCII/reduced-motion variants |
| Native composer PTY owner | Passed, including actual Ctrl+Enter input |
| xterm.js browser journeys | 14 passed: startup ×4, resize ×4, modal interaction ×3, composer, slash happy and slash edge |
| Release 10,000-entry resize benchmark | 100 measured resizes after 10 warmups; p95 **1.279 ms**, below 33 ms; detached anchor preserved |
| Grok reference render crate tests | 1,143 passed; 1 ignored |

The selected emulator bundle contains **496 PNG/ANSI pairs**, all checked against
SHA-256 receipts: 320 Harness welcome frames, 44 final Harness surface frames,
42 native-runtime samples replayed through xterm.js, 40 Grok welcome frames and
50 Grok Markdown/streaming/tool/dashboard/settings frames. Welcome fixtures cover
80×24, 120×40, 160×50, 89×32, 90×32, 90×24, 120×32 and 200×60; four capability/theme
profiles; normal/reduced motion; and 0/100/300/1300/4000 ms. Additional production
Settings captures cover 20×8, 47×9 and 48×10. Tool/Markdown comparisons include
24/40-column reference boundaries and 40-column Harness tool rendering.

Native color-cell analysis confirms that the Unicode H changes at 120×40 and
160×50 while text and controls remain stationary. The measured 80×24 layout has
no H; monochrome ASCII is static. All six native reduced-motion variants have
identical first and settled cells. See
[motion verification](../.omo/evidence/grok-parity-final/native-motion/motion-verification.json).
Exact injected-clock frames and real elapsed-time native/reference frames are
separate evidence sets; actual timings are retained in their manifests.

## Authority mapping and comparison limits

| Reference operation | Harness implementation / retained boundary |
| --- | --- |
| Identity and styling | Harness name, H artwork, palette, version/provider facts and terminology remain. Geometry and semantic roles follow the reference; brand RGB and artwork do not. |
| Permissions and questions | Existing coordinator-owned `UiIntent::ResolvePermission` and question handlers. Compact dashboard review, always-allow confirmation and acknowledgement are preserved. Grok pattern editing is not exposed without a corresponding coordinator-approved scope contract. |
| Inactive session input | Existing `ContinueSession` / `ReplaySession` eligibility opens the selected session before replying; the current coordinator is not presented as owner of every recorded run. |
| Memory | Actual durable key/value data, selected value preview/copy in live mode; replay does not read current workspace memory. Grok file-memory deletion is not mapped to unrelated key/value operations. |
| Plans | Existing validated workspace plan preview/copy/delete. There is no provider-plan approval/comment intent, so no fabricated approval control. |
| Extensions and usage | Actual MCP configuration, manifest inventory, existing MCP toggle, recorded usage and context accounting. Commercial credits, billing and marketplace installation remain explicitly unavailable. |
| Lifecycle and safety | Explicit reopen, coordinator cancellation/queue ownership, safe controls/paths/links, secret handling, reduced motion and first-event resize anti-starvation remain. No ACP transport or automatic restoration is imported. |

The paired captures establish rendering and interaction evidence, not a claim
that every pixel or backend capability is identical. Grok's comparison drivers
call its production renderers: its tool samples exercise standalone content
rendering, and its dashboard sample exercises the roster without an ACP session,
child graph, permission request or live reply. Harness's pending-input scenarios
exercise real recorded events and coordinator intents. Those differences are
identified in the gallery and reference producer metadata. No provider/network
session was needed for deterministic comparison.

Browser QA runs from a clean verification checkout to bind captures to source
and freshly built executables. Manifests retain the tested revisions: welcome
`37e388ce`, native producer `d59cf496`, final surfaces and final completion
journeys `10237e2e`; intervening dashboard/performance checks also retain their
own receipts. Later source changes do not alter the welcome renderer tested at
those earlier revisions. The final full workspace suite covers the current
implementation. Reference-tree dirtiness consists of the local capture examples;
Grok production source was not edited. Reviewable copies of both examples are
in `scripts/qa/fixtures/`.

## Findings fixed during verification

The first release benchmark exposed a quadratic historical sticky-prompt lookup:
resize p95 was 450.897 ms on 10,000 entries. The lookup now selects the section at
the viewport with `partition_point`, and push-off scans stop at the visible
boundary. The same benchmark now passes at 1.279 ms with its anchor intact.
Both the failing and passing logs are retained.

Real event-ingestion fixtures exposed early ANSI stripping in shell blocks and
whitespace loss in read/shell wrapping. Recorded bytes now reach the safe cell
interpreter, and preformatted wrapping preserves indentation. Additional fixes
cover parent status grouping after permission resolution, typed question text,
read skill labels, style-boundary tabs, one-cell wide graphemes, and viewer
scroll positions beyond 65,535 rows.

Ten existing workspace failures were reproduced at the implementation baseline
with only its missing parser dependency added. Runtime-identity prefix insertion
now shifts the provider budget's pending-user index, including fallback paths;
existing model-override/compaction/child-model tests cover it. Stale shipped
catalog/example assertions and composed-prompt snapshots were updated for the
baseline's intentional Astra additions. No new prompt semantics were introduced.
One existing test was moved to keep its owner below the suite's 800-line limit;
the gate now recognizes the split native PTY support owners and lane dry-run test.

Initial color captures inherited host `NO_COLOR=1`; they were superseded by
explicit color captures. ASCII prompt assertions and direct test-executable
provenance were corrected in browser QA. The slash empty-required-argument case
now allows the intentionally unchanged frame after a no-op Enter, while still
checking visible prompt/error state. Final browser receipts all report cleanup.

## Reproduction

Generate production frames with an explicit deterministic workspace and color
profile, then replay the ANSI files through the bundled xterm.js driver:

```bash
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  HARNESS_TUI_TEST_WORKSPACE=1 HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/harness-frames \
  cargo nextest run --profile ci -p harness-tui --test grok_parity_render_test
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  HARNESS_TUI_TEST_WORKSPACE=1 HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/harness-frames \
  cargo nextest run --profile ci -p harness-tui --lib -E 'test(grok_parity_surfaces)'
node scripts/qa/render-recorded-frames.mjs /tmp/harness-frames .omo/evidence/parity-render
HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_P1_03_ARTIFACT_DIR=/tmp/harness-native \
  cargo nextest run --profile ci -p harness-tui --test p1_03_pty_recorded \
  --ignore-default-filter --run-ignored all \
  -E 'test(p1_03_native_pty_owner_records_startup_reveal_terminal_states)'
node scripts/qa/web-terminal-visual-qa.mjs --scenario slash-completion-edge \
  --cols 80 --rows 24 --evidence-dir .omo/evidence/parity-slash
```

Use a clean checkout for browser QA's fail-closed source/executable provenance.
Its Node dependencies come from `scripts/qa/package-lock.json`; captures used
xterm.js 6.0.0 and `/usr/bin/chromium`. For a shared build directory set
`CARGO_TARGET_DIR` explicitly. The reference drivers in `scripts/qa/fixtures/` call public renderer entrypoints
and record fixture limitations in their output metadata. Copy them to the
reference pager's `examples/` directory as `harness_parity_capture.rs` and
`harness_surface_capture.rs`, respectively, then use `cargo run` with
`--manifest-path inspirations/grok-build/Cargo.toml -p xai-grok-pager` and
`--example harness_parity_capture` or `--example harness_surface_capture`,
passing the output directory after `--`. Unset `NO_COLOR` and set the terminal
color variables as in the Harness commands.
