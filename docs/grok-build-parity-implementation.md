# Grok Build presentation parity implementation

Source of scope: `20260906-192230/SYNTHESIS.md` and its bilateral source bundles.
Reference checkout: `inspirations/grok-build`. This log tracks implementation
and executed evidence; the research's proposed scenarios are not passing tests.

## Retained boundaries

Harness name, H artwork, palette, version/provider facts and terminology remain.
Coordinator authority, recorded-data replay, safe controls/paths/links, privacy,
global reduced motion, exact return anchors, bounded scheduling, first-event
resize debounce and explicit reopen remain. Backend-dependent surfaces use real
Harness data and intents; commercial or unsupported capabilities are identified
as unavailable. These are the exceptions already specified by the research and
the user's branding requirement.

## Work and verification ledger

- [ ] A: selected transcript entries, live block viewer input, transcript find,
  historical sticky prompts, response anchors, singleton context groups.
- [ ] B: grouped list-first dashboard, rich bottom peek, actual reply editor.
- [ ] C: send-now, multiline/Bash, stash, cancel, queue/history, long-paste chips;
  slash modifiers/dismissal, full ranked results, argument selection, wrapping,
  hover and file picker navigation.
- [ ] D: contained undimmed modals; memory search/preview and typed settings;
  inventory/map permission, plan, extensions and usage authority.
- [ ] E: recorded typed read/search/web/MCP bodies, stateful syntax/diffs,
  safe styled terminal output, empty-command disclosure, compact subagents.
- [ ] F: measured welcome fit, stacked H tiers, immediate controls/shimmer,
  structured workspace facts, compact policy, correct identity/notes ownership.
- [ ] G: indentation/tabs, structural Markdown/fences, table alignment/shapes,
  soft breaks/hyphens, quotes/math and theme-aware syntax.
- [ ] Verification: scoped behavioral tests, workspace format/check/lints and
  required suite gates; production PTY/xterm captures across sizes/profiles and
  injected motion times, with reference comparison and recorded limitations.

## Implementation status (verification in progress)

Baseline: Harness `60282c31862bb34e4565b7574d1ef7dbebf1f126`; reference checkout
`inspirations/grok-build`. The untracked research folder is preserved.

A: visual-entry IDs now own navigation/disclosure; the full-screen selected-entry
viewer owns paging, find, selection, copy and raw/Markdown modes. Historical
sticky prompts and final-answer jumps use measured layout entries. Context groups
include singletons and retain identity when another member arrives.

B: the dashboard has grouped multirow roster rows, a bottom Markdown peek, and a
real per-session reply editor. Refresh/resize preserve drafts and interaction;
replies and pending permission/question input use the existing intent boundary.
Opening inactive sessions uses existing continuation/replay routes.

C: migrated Ctrl+Enter, multiline Alt+Enter/Bash Enter, stash and mode-aware
Escape semantics. Empty-prompt Up uses queued entries then searchable history;
large bracketed pastes have expandable chips and grouped undo. Slash completion
keeps the typed query on dismissal, uses full ranked results, measures wrapped
rows, separates hover from selection, and supports actual model/effort choices.
File completion supports drilling, paging and line-range selection.

D: shared modal chrome is centered, contained and undimmed. Memory has filter,
selected Markdown preview, fullscreen and copy. Settings has human-readable
search plus Boolean, integer, string and enum editors; scalar drafts commit only
after full document validation and Escape discards them. Dedicated Usage and
Extensions views show recorded usage and actual configured integration state;
unknown backend/account capabilities remain explicit.

E: recorded search/web/MCP projections replace generic summaries. Code/read/diff
highlighting follows Harness theme roles, using recorded pre-edit context for
multiline scopes. A bounded session-artifact cache keeps filesystem reads out of
paint. Recorded command bytes are interpreted into safe styled cells with carriage
return overwrite; empty commands remain disclosable. Subagents use one-line
lifecycle rows and existing child navigation.

F: welcome layout measures actual H artwork, notes, notices and actions. Normal
mode shows controls immediately with a diagonal shimmer; reduced motion is static.
Workspace directory, branch, detached state and linked-worktree facts are carried
separately from discovery. Current release notes describe Harness features.

G: the baseline already contained structural Markdown, math and table improvements;
its missing parser dependency is now declared. The shared preformatted path now
preserves indentation and expands tabs, with matching source-aware selection.
Prose supports reference hyphen breaks; citation fences resolve syntax by extension.

Executed checks so far: repeated scoped TUI library runs (over 1,370 passing
behavioral tests), dashboard/live-viewer integration tests, and workspace lint
iterations. Regression coverage has been extended for real settings validation and
cancel, memory search/fullscreen/resize, send-now intent ordering, paste chips and
undo, safe ANSI overwrite, typed grep/web output, recorded multiline diff scopes,
and fenced-code paint/copy. The release-only resize benchmark is excluded from
debug runs and will be checked in its required lane.

Not signed off yet: final workspace checks and lanes, current production PTY/xterm
captures, paired reference captures, and the remaining acceptance-edge audit.
The native startup QA owner is being migrated from staged-reveal ordering to
immediate controls plus real runtime samples at 0/100/300/1300/4000 ms. Exact
injected-clock rendering is checked separately from runtime elapsed-time captures.
A reference render-crate build is also underway; no paired pixel certification
is claimed before actual captures are inspected.

## Authority mapping for conditional surfaces

| Reference operation | Harness source of truth and reachable action |
| --- | --- |
| Permission and question replies | `UiIntent::ResolvePermission` and the existing coordinator. Dashboard input reuses the live permission/question handler; a compact dashboard opens full review before accepting a decision. Always-approve retains the existing confirmation and coordinator acknowledgement. |
| Input for an inactive session | Recorded history and eligibility select `ContinueSession` or `ReplaySession`. The UI opens that session before sending a reply or resolving input; it does not pretend that the current coordinator owns every recorded run. |
| Memory preview/copy | Actual durable key/value memory in live mode. Search and Markdown preview use the value; copy copies that value. Replay does not read current workspace memory. There is no new file-memory delete UI. |
| Plan preview/copy/delete | Existing workspace plan browser and validated plan paths. Its operations remain distinct from approving a provider plan. There is no plan-approval/comment intent in Harness's live `UiIntent` contract, so no fabricated approval control is added. |
| Extensions | Actual configured MCP integrations, manifest inventory, and the existing MCP toggle owner. Marketplace/account installation is not implied. |
| Usage | Recorded request usage and current context accounting. Missing usage, account credits, and commercial billing remain explicitly unavailable. |

## Verification findings resolved during implementation

The baseline checkout also failed ten workspace tests. This was reproduced in
`/tmp/harness-grok-parity-verification` at baseline commit `60282c31`, adding only
the missing `pulldown-cmark` dependency needed to compile that baseline. The
baseline run is recorded in `/tmp/harness-parity-baseline-failures.log`.

Switching away from a profile's model inserts a runtime-identity system message.
The provider budget's pending-user index now shifts by that inserted prefix,
including the non-canonical fallback path. Existing model-override, fallback,
compaction and child-model tests cover this regression. Shipped example/catalog
assertions and composed prompt snapshots have been refreshed for the baseline's
intentional Astra model and prompt changes; no new prompt behavior was introduced.

The suite gate now recognizes the explicit split native-PTY support owners and
lane dry-run test. One existing question-answer validation test was moved into a
focused module to restore the 800-line test-owner limit; its behavior is unchanged.

Subagent spawn rows are excluded from semantic context grouping so their task
titles and lifecycle remain visible. The styled code wrapper now expands tabs
across syntax boundaries and advances safely when a wide grapheme meets a
one-cell viewport. Large viewer output no longer truncates scroll offsets to u16.
