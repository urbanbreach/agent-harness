# Native subagent rewrite and verification

Completed on 4 October 2026 against Grok Build pager 1.0.45, source
`2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`. The final implementation commit is
`efea90e4` on `codex/complete-grok-subagents`.

The completion criteria are the original request: replace the old subagent
implementation and match Grok's subagent logic, behavior, and views. OMO's
505-scenario process and its approval machinery are not acceptance requirements.

## Implementation

The coordinator's `public_subagents` runtime owns spawning, preparation,
foreground/background execution, waiting, cancellation, messaging, reactivation,
completion delivery, counters, and restoration. Native tools dispatch to that
owner. The old child executor is retired. Historical event decoding, replay
projections, compatibility aliases, and shared coordinator gates remain necessary
for existing sessions; they do not run a second subagent lifecycle.

The TUI uses the native Tasks pane, child-session view, transcript navigation,
and block viewer. The rewrite covers live progress and terminal status, model and
elapsed labels, parent/child navigation, draft restoration, search and filtering,
raw/rendered modes, wrapping, code and Markdown styles, selection, copy, follow,
scrolling, and viewer reopen state. Rendering remains pure.

Observed reference quirks are retained, including the requested raw/rendered
cursor jump, stale parent row height after a viewer-mode toggle, filtered Tasks
actions and adornments indexing unfiltered rows, and viewer scrollbar track drags
being consumed without movement.

Two user-approved exceptions remain:

- Definition-listed startup skills obey Harness's shared skill permissions.
- Copy uses Harness's shared clipboard integration. It does not persist every
  copied selection to a backup file or display Grok's backup-path notice. The
  existing disable-copy-on-select setting and clipboard error handling apply.

See [the operator contract](operations/generic-agent-and-tasks.md) for tool and
configuration behavior.

## Verification

The final workspace run passed **1,964 tests**, with 8 tests and 7 binaries skipped
by the existing Nextest profile. Format, workspace check, strict all-targets /
all-features Clippy, source-size/test gates, and the dogfood script self-test
passed. The branding gate still reports five existing references in
`plans/001-role-scoped-subagents.md` and
`docs/evidence/tui-rewrite/runtime-state/post-measure-host.json`.

The maintained tests exercise coordinator/tool boundaries, including definition
resolution, startup permissions, native output, ownership, cancellation, waits,
completion deduplication, live progress, persisted context, same-identity message
wakes, and replay. A regression reproduced the stale parent wake after a child
was reused; the fix suppresses that superseded wake and its transcript prompt.
Viewer interaction coverage includes release endpoints, empty-space clicks,
sticky selection, Escape, logical-line yank, copy expiry, and word/paragraph
selection.

The final paired captures use actual Crossterm input handlers and Ratatui output
through a PTY and xterm.js. They use deterministic session fixtures, not live model
responses. Images are compared without scaling or color normalization.

| Comparison | Result | Evidence |
| --- | --- | --- |
| Rich Markdown, raw modes, viewer reopen at 80×24, 120×40, 160×50 | 30 whole-terminal images exactly equal | [Measurements](evidence/subagents-20261004/rich-text-comparison.json) |
| Tasks filtering, Open/Kill, overlays, Ctrl-F and Escape | 27 reference-allocated Tasks pane images exactly equal | [Measurements](evidence/subagents-20261004/tasks-comparison.json) |
| Viewer press, drag, release and selection clearing | 15 comparisons pass; 11 whole-terminal images equal, 4 differ only in the accepted clipboard notice | [Measurements](evidence/subagents-20261004/pointer-comparison.json) |
| Viewer scrollbar and `y` | 5 whole-terminal images exactly equal at 120×40 | [Measurements](evidence/subagents-20261004/scrollbar-comparison.json) |
| Completed-source resume | New identity, source link, inherited messages, model, output, status, turns and executed tool calls equal | [Field comparison](evidence/subagents-20261004/resume-comparison.json) |

The resume comparison runs Grok through its production ACP/HTTP path and Harness
through its production coordinator and normalized provider boundary. It checks
observed messages and durable results. Harness requires declared model limits;
its fixture specifies the observed 256,000-token reference context fallback and
an independently chosen 4,096-token output limit. Grok's mock catalog has no
output cap. This comparison does not claim equivalent unknown-limit handling,
whole-request bytes, system-prompt bytes, token estimates, wall-clock durations,
or durable schemas. Unknown model limits still fail conservatively in Harness,
as required by the repository contract.

These are bounded reference comparisons plus maintained behavioral tests, not a
formal proof for every possible input, platform, font, provider, or terminal.
Unrelated parent-shell chrome is outside the subagent rewrite.

## Reviewable artifacts

The [manifest](evidence/subagents-20261004/manifest.json) records image, fixture,
and executable hashes. Captures use `476f09c6`; the subsequent `efea90e4` change
only preserves shared clipboard opt-out/error handling and passes the full test
and lint runs. It does not change rendering. Representative pairs are committed with this report:

| State | Grok | Harness |
| --- | --- | --- |
| Rendered rich text | [Reference](evidence/subagents-20261004/rich-rendered-reference.png) | [Candidate](evidence/subagents-20261004/rich-rendered-candidate.png) |
| Raw rich text | [Reference](evidence/subagents-20261004/rich-raw-reference.png) | [Candidate](evidence/subagents-20261004/rich-raw-candidate.png) |
| Filtered Tasks | [Reference](evidence/subagents-20261004/tasks-filter-reference.png) | [Candidate](evidence/subagents-20261004/tasks-filter-candidate.png) |
| Viewer drag selection | [Reference](evidence/subagents-20261004/viewer-drag-reference.png) | [Candidate](evidence/subagents-20261004/viewer-drag-candidate.png) |

Full local receipts, ANSI, images, fixtures, earlier comparisons and check logs
remain in `.omo/evidence/subagent-completion-20261003/`. That directory is ignored
by Git. The final run logs are `workspace-final-tests.log`,
`clipboard-policy-final-clippy.log`, `final-quality-gates/`, and `final-dogfood-self-test.log`.

Re-run maintained checks with:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
python3 scripts/check-test-suite-gates.py
bash scripts/harness-qa-dogfood.sh --self-test
```
