# Chat and tool parity with Grok Build

Compared Harness directly with `inspirations/grok-build`, including its production
scrollback renderer, grouping state machine, tool blocks, Markdown renderer, and
syntax themes. Verification completed on 11 September 2026 (Europe/Helsinki;
artifact directories use the UTC date 20260910).

Reference upstream `SOURCE_REV`: `d5a0335a47221e8c9519936cb693e9b6450227ec`.
This follow-up builds on [the earlier implementation](grok-build-parity-implementation.md).
The subsequent [alignment and task-argument follow-up](chat-alignment-tool-arguments.md)
fixes the task/todo offsets and extends the lifecycle evidence to all tool families.

Open the [xterm comparison gallery](../.omo/evidence/chat-parity-20260910/index.html)
or [evidence index](../.omo/evidence/chat-parity-20260910/README.md). Evidence is
local and ignored by Git; the fixtures and comparison scripts are source files.

## Differences fixed

| Area | Resulting behavior |
| --- | --- |
| Context groups | Eager singleton groups; finished collapsed thoughts join the group without increasing its count; active/open thoughts and open context tools remain transparent; pending approval ends a group. |
| Group disclosure | Group expansion reveals member headers independently of each member's output. Expanding a member does not destabilize the group's collapse target. |
| Dense groups | The reference's greater-than-11 threshold, last-ten visible tail, hidden-prefix label, and expanded first-row replacement are reproduced. |
| Counts and selection | Web sources and child sessions deduplicate by identity. Searching a folded group opens the matching member and selects its stable tool ID. Selection/hover replaces the marker without overwriting text or splitting a grapheme. |
| Group appearance | One clipped header row, reference marker/label/error colors, and no extra rail or permanent disclosure arrow. |
| Tool lifecycle | Running, completed, and failed commands default to collapsed output. Failures do not force disclosure. Command descriptions omit redundant Run/Running prefixes, and error output uses the reference's red foreground and transparent background. |
| Commands | Reference marker colors, groupable collapsed-marker dimming, command/output alignment, muted dollar sign, syntax colors, running wave, and expanded output rail. Titles remain stationary. |
| Reasoning | Shared streaming Markdown and syntax highlighting, dimmed foreground, preserved code background, matching spacing, and source-aware selection. The separate incomplete reasoning parser was removed. |
| Themes and diffs | GrokNight/GrokDay neutral roles and exact TextMate palettes, including code, prompt, path, diff, and highlight colors. Terminal-native syntax uses adaptive ANSI/default foregrounds. Upstream assets and adapted mapping carry Apache-2.0 attribution. |
| Streaming code | A bounded append-aware syntax cache preserves parser state across complete lines, reparses the unfinished tail, and invalidates on edits/theme/language changes. An existing behavioral fixture verifies 30 appended lines require 30 parser rows and still match a fresh parse. |
| Terminal output | Emoji modifiers, flags, variation selectors, and ZWJ sequences retain grapheme boundaries. Carriage-return overwrite and erasing half of a wide cell leave valid terminal cells. |
| Spacing and paths | Matching Markdown/code separators and end-of-response spacing; paths appear once in pending, failed, and completed file headers, with the reference path colors and no permanent trailing arrow. |
| Edit approval | The filename stays in the review title; internal serialized request/argument rows are omitted, matching the reference. Human descriptions are retained. |
| Durable tool state | An assistant completion preserves already-requested tools and their permission/lifecycle facts. A later tool request updates authoritative identity and argument summaries. This fixes the filename disappearing when an edit was queued before the provider response. |

The last issue was found in a real `golden_path_interactive` PTY run: the edit
succeeded, but the header became `Edit +1/-1`. The assistant commit had cleared
the queued tool from the canonical projection, so its start event recreated an
incomplete row. The projection now keeps it, and the repeated terminal journey
shows `Edit demo.txt +1/-1` before and after resizing. Regression coverage also
checks a response that explicitly mentions the already-queued tool, preventing
its approval history from being reset.

## Reference and implementation map

Reference paths below are under `inspirations/grok-build/crates/codegen/`:

- `xai-grok-pager/src/scrollback/state/{verb_group,groups,selection,layout}`:
  grouping, disclosure, source counts, and selected-entry behavior.
- `xai-grok-pager/src/scrollback/wrappers/entry_renderer.rs` and `scrollback_pane`:
  marker/accent post-pass, groupable dimming, clipping, and animation.
- `xai-grok-pager/src/scrollback/blocks/thinking.rs` and
  `blocks/tool/{execute,read,edit,web_search}.rs`: tool and reasoning presentation.
- `xai-grok-pager/src/app/acp_handler/permissions.rs`: edit approval titles,
  human descriptions, and omission of native edit arguments.
- `xai-grok-pager-render/src/{theme,syntax,appearance}` and
  `xai-grok-markdown/src/{syntax,open_code_highlighter}`: theme roles, syntax,
  streaming state, and animation timing.

Harness shares one grouping scan in `crates/harness-tui/src/ui_transcript_groups.rs`
across painting, metadata, and navigation. Disclosure state lives in
`app/transcript_view.rs`; Markdown/syntax changes live in `ui_streaming_markdown.rs`,
`ui_reasoning_markdown/body.rs`, and `ui_syntax_highlight/`. The durable fix is in
`crates/harness-core/src/transcript_projection.rs`; edit approval measurement and
content share `crates/harness-tui/src/layout/permission.rs`.

## Verification

- Full workspace nextest: **4,588 passed**, with ten configured skips.
- Full workspace/all-target/all-feature Clippy with warnings denied, formatting,
  workspace checking, and the canonical quality gates passed.
- Browser QA driver tests: **51 passed**.
- Production renderer matrix: **504 xterm.js screenshots**, 252 per renderer.
  Fourteen scenes × three sizes (40×24, 80×24, 120×40) × three exact times
  (0, 330, 660 ms) × normal/reduced motion.
- [Automated comparison](../.omo/evidence/chat-parity-20260910/cell-comparison.json):
  **42 paired normal-motion frames / 4,098 identical nonblank chat cells** at
  120×40. Checks include characters, width, position, foreground/background
  values and color modes, plus the presence of RGB SGR. Reduced-motion stability
  is checked independently in all 42 scene/size combinations.
- Three real PTY/xterm journeys passed: provider-issued edit approval, disclosure
  and resize; two chat turns, search, block viewer and raw mode; and the formerly
  failing pre-response queued-edit scenario. Every journey binds a freshly built
  binary to a clean verification checkout and records cleanup.
- [Offline dogfood](../artifacts/qa-evidence/20260910-grok-chat-parity-settlement/README.md)
  passed through the real binary with isolated sessions and inspectable durable
  events. Its receipt confirms the user's Harness configuration was untouched.

`grok_parity_render_test.rs` constructs Harness state through events and public
input handling. The reference driver calls actual `EntryRenderer` and
`ScrollbackState`/`ScrollbackPane` entrypoints, including the production
`groupable` post-pass. The reference production source is unchanged.

The exact cell comparison translates Harness's chat origin by (-2,-3) and
excludes surrounding shell chrome. The smaller frames exercise viewport clipping
and resizing but have different available chat heights from standalone reference
entries. Sparse xterm snapshots compare nonblank cells; they are not a whole-app
pixel-identity assertion. Harness reduced motion uses its static accessibility
policy, while the reference fixture holds tick zero. No live-provider transport
or universal backend equivalence is inferred from these offline fixtures.

## Reproduction

From the repository root (with Node dependencies installed in `scripts/qa`):

```bash
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/harness-chat-frames \
  cargo nextest run --profile ci -p harness-tui --all-features \
  --test grok_parity_render_test -E 'test(chat_and_tool)'
node scripts/qa/render-recorded-frames.mjs \
  /tmp/harness-chat-frames .omo/evidence/chat-check/harness
```

Copy `scripts/qa/fixtures/grok-chat-capture.rs` to the reference pager crate's
`examples/harness_chat_capture.rs`, then run:

```bash
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  cargo run --manifest-path inspirations/grok-build/Cargo.toml \
  -p xai-grok-pager --example harness_chat_capture -- /tmp/grok-chat-frames
node scripts/qa/render-recorded-frames.mjs \
  /tmp/grok-chat-frames .omo/evidence/chat-check/reference --reference-grok
node scripts/qa/compare-chat-frames.mjs \
  .omo/evidence/chat-check/harness .omo/evidence/chat-check/reference \
  .omo/evidence/chat-check/cell-comparison.json
bash scripts/harness-qa-dogfood.sh --slug grok-chat-parity
```

The color environment is required: inherited `NO_COLOR` suppresses RGB SGR even
when Ratatui's in-memory cells are colored. The comparison fails if RGB is absent.

For real PTY journeys use `scripts/qa/web-terminal-visual-qa.mjs` with a clean
verification checkout and an isolated session directory. The final evidence
contains each exact command and ordered action list in its manifest and
`interactions.json`; the checkouts are copies of the current working tree so
pre-existing uncommitted work is included without committing the user's tree.
