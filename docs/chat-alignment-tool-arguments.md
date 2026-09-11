# Grok Build alignment and task argument follow-up

The screenshot's displaced task diamonds came from a five-space task prefix
beside the three-space prefix used by other tool headers. Expanding todos also
introduced a separate nested shell. Both now use the same entry origin as chat,
reasoning, tool placeholders, and completed tools. Long headers stay on one row.

The source of truth is Grok Build revision
`d5a0335a47221e8c9519936cb693e9b6450227ec` in `inspirations/grok-build`:

- `crates/codegen/xai-grok-pager-render/src/appearance/config.rs`: two outer
  horizontal cells and two inner padding cells.
- `crates/codegen/xai-grok-pager/src/scrollback/layout.rs`: one accent cell.
- `scrollback/block.rs` and `scrollback/wrappers/entry_renderer.rs`: shared bullet
  and header origin across lifecycle, animation, selection, and disclosure.
- `scrollback/blocks/tool/{read,edit,execute,search,list_dir,web_search,other}.rs`:
  per-tool output gutters and separators.

All offsets below are display cells relative to the entry content origin. With
the two-cell outer margin, that origin is column 5 (zero based).

| Content | Copied offset |
| --- | --- |
| Header diamond / label | 0 / 2 |
| Read line number | 0; content follows the number field and two spaces |
| Diff line number | 2 |
| Search metadata / path | 2 |
| Search line number | Four-cell field after four spaces; content follows two spaces |
| Directory listing / web body | 2 |
| Command output / generic output | 0 |
| Question number / answer arrow | 2 / 5 |

The output separators also follow the reference: one blank row for reads,
generic output and listings; separate search metadata and result groups; an
additional inner top/bottom row for web content. Compaction and assistant error
surfaces use the shared origin, with wrapping budgets reduced by their padding.

## Tool argument failure

The recorded failing task requests supplied both `session_id: ""` and
`task_id: ""`. The executor treated the empty session ID as a continuation
target and rejected it as an unknown child session. Empty selectors now mean
"start a child"; an empty alias no longer hides a valid continuation ID supplied
through the other alias. Nonempty selectors still pass through the existing
lineage and permission checks.

The Responses request omitted `strict` while retaining native schemas with
optional/defaulted properties. Responses can normalize such schemas into strict
form and promote optional properties to required ones. Requests now explicitly
send `strict: false`, preserving the native optional-field contract. See the
[official strict-mode documentation](https://developers.openai.com/api/docs/guides/function-calling#strict-mode).
The durable events establish the empty-ID failure; schema normalization is the
wire-level explanation for why unused fields can appear as empty placeholders.

The UI also used the scheduler's cancellation record as the task outcome, even
when validation failed before a child was created. Such calls now display
`failed`, and cannot navigate to the parent session as if it were a child.

## Verification and reproduction

The [local evidence index](../.omo/evidence/alignment-20260911/README.md) links
the xterm.js comparisons, screenshots and real CLI journey. Evidence is ignored
by Git. The fixture and comparison programs are maintained source files.

The lifecycle fixture exercises thirteen tool families through public event
ingestion and input handling at 40×24, 80×24 and 120×40. It checks streaming,
queued, permission waits, running animation, successful/failed completion,
selection, expansion and collapse. Durable fixture sequences are contiguous,
and projection errors fail the test. This matters: gaps can prevent tool results
from settling, which would otherwise leave a misleading header-only capture.

```bash
env -u NO_COLOR TERM=xterm-256color COLORTERM=truecolor \
  HARNESS_PARITY_RENDER_ARTIFACT_DIR=/tmp/harness-alignment-frames \
  cargo nextest run --profile ci -p harness-tui --all-features \
  --test grok_parity_render_test -E 'test(all_tool_families)'
```

Copy `scripts/qa/fixtures/grok-alignment-capture.rs` to the upstream pager's
`examples/harness_alignment_capture.rs`. Run that example to produce reference
ANSI frames using its production `EntryRenderer` and tool block implementations.
Replay both frame directories with `scripts/qa/render-recorded-frames.mjs`,
passing `--reference-grok` for the reference. Compare the resulting directories
with `scripts/qa/compare-tool-alignment.mjs`.

The geometry comparison covers marker positions and relative header spacing in
481 paired frames, plus representative expanded body anchors. It excludes shell
chrome and differences in tool-specific wording; it is not a whole-application
pixel-identity assertion. The native task/coordinator regression covers blank
selectors and continuation identity, and the fake HTTP provider regression
checks the actual Responses payload. Offline dogfood and the real CLI PTY
journey provide separate runtime evidence; neither is a live-provider claim.
