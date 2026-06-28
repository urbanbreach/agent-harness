# Ctrl+P OpenCode Parity PRD

Provenance: Hyperplan team run `bb773baa-603e-4b6c-b3ac-078fd2acfb27`. Primary source anchors: Harness `crates/harness-tui/src/keybindings/palette_model.rs`, `crates/harness-tui/src/app/palette_controller.rs`, `crates/harness-tui/src/ui_overlays.rs`, `crates/harness-tui/src/app/tests/palette_parity_tests.rs`; OpenCode `inspirations/opencode/packages/tui/src/component/command-palette.tsx`, `inspirations/opencode/packages/tui/src/ui/dialog-select.tsx`, `inspirations/opencode/packages/tui/src/app.tsx`, `inspirations/opencode/packages/tui/src/component/prompt/index.tsx`, `inspirations/opencode/packages/tui/src/routes/session/index.tsx`.

## Objective

Reach exact, evidence-backed Ctrl+P command palette parity with OpenCode for Harness TUI.

The implementation loop must compare every Harness Ctrl+P palette item against the OpenCode source under `inspirations/opencode`, close every behavior/UI/UX gap, and leave a verified parity surface that can be maintained by tests rather than manual inspection.

Parity means:

- The set of OpenCode-compatible command palette entries exposed by Harness matches OpenCode’s reachable, visible command palette surface unless a documented scope decision explicitly excludes an OpenCode command.
- Every included OpenCode-parity command has real behavior or a deliberate documented exclusion. `PaletteDispatch::Placeholder` is a failure for included commands.
- Harness-only commands do not pollute the exact OpenCode-copy Ctrl+P palette unless they are moved behind a deliberate secondary/non-parity surface.
- Filtering, grouping, suggestion duplication, keyboard navigation, mouse behavior, footers, row semantics, and dispatch lifecycle match OpenCode’s `CommandPaletteDialog` plus `DialogSelect` behavior.
- Availability/hidden logic matches OpenCode’s route, session, provider, workspace, model, prompt, stash, share, revert, and editor-context state conditions.
- Parity is proven by deterministic tests, render tests, PTY/manual TUI evidence, live-provider dogfood, logs/events inspection, redaction checks, and documentation/evidence updates.

## Non-Goals

- Do not change files under `inspirations/opencode`; they are read-only reference source.
- Do not invent compatibility aliases or extra commands that OpenCode does not expose.
- Do not move runtime invariants out of `harness-core` or the coordinator to satisfy TUI behavior.
- Do not claim visual parity without deterministic snapshot or PTY provenance.
- Do not claim live-provider parity without logs/events/support-bundle evidence.
- Do not weaken, remove, or loosen existing palette tests to make parity pass.
- Do not implement speculative Harness-only features inside the exact OpenCode-copy Ctrl+P palette.

## Source-Of-Truth Inventory Model

### OpenCode Source Of Truth

OpenCode command palette behavior is defined by:

- `inspirations/opencode/packages/tui/src/component/command-palette.tsx`
- `inspirations/opencode/packages/tui/src/ui/dialog-select.tsx`
- `inspirations/opencode/packages/tui/src/app.tsx`
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx`
- `inspirations/opencode/packages/tui/src/routes/session/index.tsx`

The implementation loop must treat these files as the behavioral reference.

`CommandPaletteDialog`:

- Reads keymap-reachable palette entries with namespace `palette`.
- Filters out hidden commands and the command palette command itself.
- Applies `command.palette.show` visibility.
- Uses registered bindings when present.
- Maps options to `title`, `description`, `category`, `footer`, `suggested`, `value`, and `onSelect`.
- Duplicates suggested options into a `Suggested` category only when the filter is empty.
- Clears the dialog and dispatches the command on select.

`DialogSelect`:

- Uses `fuzzysort` over `title` and `category`.
- Uses score function `r[0].score * 2 + r[1].score`.
- Wraps up/down navigation.
- Moves page up/down by 10.
- Supports home/end navigation.
- Supports mouse move, mouse down, mouse up, and mouse-over selection behavior.
- Supports footer actions.
- Supports current/gutter/details rows generally.
- Has `flat` mode, but `CommandPaletteDialog` does not pass `flat`.

Required correction: because `CommandPaletteDialog` does not pass `flat`, Harness command palette must not flatten filtered results and must not use category-as-footer due to flat mode.

Required correction: current-dot/gutter exists in `DialogSelect`, but `CommandPaletteDialog` does not use it. If Harness shows current markers in the command palette, remove them. Do not claim OpenCode command palette has a current dot.

### Harness Current Inventory Facts

Harness source anchors:

- `crates/harness-tui/src/keybindings/palette_model.rs` is the registry.
- `crates/harness-tui/src/app/palette_controller.rs` controls availability, filtering, dispatch, suggested duplication, grouping, placeholder banners, and weighted handmade fuzzy subsequence behavior.
- `crates/harness-tui/src/ui_overlays.rs` renders the palette with dim backdrop, title/esc header, block-cursor input, grouped rows, section rows, and footer equal to action keybinding else category label.
- `crates/harness-tui/src/app/tests/palette_parity_tests.rs` covers basic open/close/filter/hidden-excluded/dynamic title/limited dispatch/rendering, but presence tests are not sufficient.

Verified current Harness counts by direct search:

| Registry Fact | Count |
|---|---:|
| Total palette entries | 59 |
| OpenCode-parity entries where `harness_only: false` | 46 |
| Harness-only entries where `harness_only: true` | 13 |
| `Placeholder` dispatches | 26 |

These counts are the baseline to drive closure, not the target state.

Target state:

- Total count is allowed to change only through documented OpenCode source comparison.
- OpenCode-parity entry count must equal the documented included OpenCode command set.
- Harness-only entries must be absent from the exact OpenCode-copy Ctrl+P palette or moved to a deliberate secondary surface.
- Placeholder count for included OpenCode-parity commands must be zero.
- Every excluded command must have an explicit scope decision linked to the OpenCode source anchor and test coverage proving it is excluded.

### Inventory Generation Requirement

Add or update an automated inventory test/tool that emits a table with:

- Command ID.
- Category.
- Title source.
- Description source.
- Suggested rule.
- Availability rule.
- Hidden rule.
- Binding/footer source.
- Dispatch target.
- Slash names/aliases, if any.
- OpenCode source anchor.
- Harness implementation anchor.
- Status: `implemented`, `excluded`, `harness_only_secondary`, or `failing_placeholder`.

This inventory must be used by tests and by this PRD’s acceptance matrix. It must fail CI when a command silently moves between statuses.

## Parity Matrix Requirements

### Global Requirements For Every OpenCode-Parity Command

For each included command:

- Command ID must exactly match OpenCode.
- Title must match OpenCode’s static or dynamic title behavior.
- Description must match OpenCode or have a documented product-language decision.
- Category must match OpenCode.
- Footer must show registered keybindings, not category labels.
- Suggested behavior must match OpenCode.
- Hidden behavior must match OpenCode.
- Availability/enabled behavior must match OpenCode.
- Dispatch must perform the real OpenCode-equivalent behavior.
- Dispatch must close/clear the dialog before executing when OpenCode does.
- Failure must produce a visible, non-placeholder error path and structured log entry.
- Tests must prove behavior, not just registry presence.

### Session Commands

Session command parity must cover route/session state, replay/session state, share URL, revert state, stash state, undo/redo state, transcript route, current session identity, and provider-connected constraints.

Commands requiring explicit behavior/exclusion decisions include:

| Command | Required Decision |
|---|---|
| `session.rename` | Implement real rename behavior or exclude with OpenCode scope rationale. Placeholder is failure. |
| `session.timeline` | Implement jump-to-message timeline UI/dispatch or exclude with scope rationale. Placeholder is failure. |
| `session.fork` | Implement fork session behavior or exclude with scope rationale. Placeholder is failure. |
| `session.unshare` | Implement unshare only when share URL exists or exclude with scope rationale. Placeholder is failure. |
| `session.undo` | Implement revert/undo previous message behavior gated by revert state or exclude with scope rationale. Placeholder is failure. |
| `session.redo` | Implement redo behavior gated by redo state or exclude with scope rationale. Placeholder is failure. |
| `session.toggle.conceal` | Implement conceal toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `session.copy` | Implement current session copy behavior or exclude with scope rationale. Placeholder is failure. |
| `session.move` | Implement move session behavior or exclude with scope rationale. Placeholder is failure. |

Acceptance:

- Commands appear only on OpenCode-equivalent routes/states.
- Unavailable commands are hidden or disabled exactly as OpenCode does.
- Live session-affecting commands write expected events and logs.
- Replay/session-state constraints do not execute providers, hooks, MCP, network, or CLI from replay-derived inspection.

### Agent Commands

Agent command parity must cover reachable palette entries derived from app and prompt command sources, plus slash names and aliases where OpenCode derives slash entries from palette commands.

Acceptance:

- Agent commands match OpenCode IDs, titles, descriptions, categories, footers, visibility, and dispatch.
- Slash aliases derived from these commands are included in the alias inventory and tested.
- Prompt-agent command availability respects prompt input and session state.
- No Harness-only agent commands appear in the exact OpenCode-copy palette.

### Workspace Commands

Workspace command parity must cover worktree/workspace state, path context, session directory filtering, and workspace selection/list behavior.

Commands requiring explicit behavior/exclusion decisions include:

| Command | Required Decision |
|---|---|
| `workspace.set` | Implement OpenCode-equivalent workspace selector/set behavior or exclude with scope rationale. Placeholder is failure. |
| `workspace.copy_path` | Implement path copy behavior with correct footer/logging or exclude with scope rationale. Placeholder is failure. |
| `workspace.list` | Implement workspace list dialog or exclude with scope rationale. Placeholder is failure. |
| `session.move` | If workspace-related in OpenCode behavior, verify workspace move semantics and route gating. |

Acceptance:

- Worktree workspace availability mirrors OpenCode.
- Workspace commands do not run outside a valid workspace state.
- Logs redact workspace paths when needed by the project’s redaction policy but preserve enough stable identifiers for debugging.

### Provider Commands

Provider command parity must cover provider connected state, model variants, switchable organization count, authentication/connect dialogs, and live-provider actions.

Commands requiring explicit behavior/exclusion decisions include:

| Command | Required Decision |
|---|---|
| `variant.list` | Implement OpenCode-equivalent model/variant picker or exclude with scope rationale. Placeholder is failure. |
| `console.org.switch` | Implement org switch behavior only when switchable org count allows it or exclude with scope rationale. Placeholder is failure. |

Acceptance:

- Provider commands are hidden/available exactly like OpenCode for disconnected, connected, single-org, multi-org, no-variant, and variant-capable states.
- Live-provider dogfood demonstrates provider/session-affecting command behavior before and after a prompt run.
- Logs/events contain redacted provider/model/session IDs and no secrets.

### Prompt Commands

Prompt command parity must cover prompt input state, editor context state, skills, stash, paste summary, file context, diff wrapping, and slash aliases.

Commands requiring explicit behavior/exclusion decisions include:

| Command | Required Decision |
|---|---|
| `prompt.editor_context.clear` | Implement clear editor context only when editor context exists or exclude with scope rationale. Placeholder is failure. |
| `prompt.skills` | Implement skills dialog/selection behavior or exclude with scope rationale. Placeholder is failure. |
| `app.toggle.file_context` | Implement toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `app.toggle.diffwrap` | Implement toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `app.toggle.paste_summary` | Implement toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |

Acceptance:

- Prompt command visibility mirrors OpenCode prompt input and editor-context state.
- Slash names and aliases are parity surface and must be inventoried.
- Alias tests must prove each OpenCode slash alias maps to the expected palette command or documented exclusion.

### System Commands

System command parity must cover debug/console/heap snapshot commands, terminal title, animations, tips, and other UI-local toggles.

Commands requiring explicit behavior/exclusion decisions include:

| Command | Required Decision |
|---|---|
| `app.debug` | Implement debug behavior or exclude with scope rationale. Placeholder is failure. |
| `app.console` | Implement console behavior or exclude with scope rationale. Placeholder is failure. |
| `app.heap_snapshot` | Implement heap snapshot behavior or exclude with scope rationale. Placeholder is failure. |
| `terminal.title.toggle` | Implement terminal title toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `app.toggle.animations` | Implement animation toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `app.toggle.session_directory_filter` | Implement session directory filter toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |
| `tips.toggle` | Implement tips toggle and dynamic title/availability or exclude with scope rationale. Placeholder is failure. |

Acceptance:

- Local toggles mutate the same durable or in-memory state Harness uses for equivalent UI behavior.
- Dynamic titles update immediately after dispatch.
- Logs record command ID, old/new state where safe, and success/failure.
- Commands not meaningful in Harness are excluded explicitly and tested as absent.

### Harness-Only Handling

Harness currently has 13 `harness_only: true` entries.

Requirement:

- Exact OpenCode-copy Ctrl+P mode must not show Harness-only commands.
- If Harness retains Harness-specific commands, they must move to one of:
  - A secondary palette mode.
  - A Harness-specific category only visible behind an explicit non-parity setting.
  - Another existing Harness UI surface.
- Tests must prove Harness-only commands do not appear in the parity palette.
- Documentation must explain how to reach retained Harness-only commands if they move.

## UI/UX Parity Requirements

### Dialog Lifecycle

Harness must match OpenCode command palette lifecycle:

- Ctrl+P opens command palette dialog.
- Dialog title is `Commands`.
- Escape closes the dialog.
- Selecting a command clears/closes the dialog before dispatch.
- Dispatch failure does not leave a stale placeholder banner as success.
- The input is focused when dialog opens.
- Filtering resets selection to the first filtered item as OpenCode does.
- Mouse movement after filter changes must not incorrectly steal keyboard input mode.

### Filtering And Ranking

Harness must empirically match OpenCode’s fuzzysort behavior.

Requirements:

- Search keys: `title` and `category`.
- Score function: title score weighted by 2 plus category score weighted by 1, matching `r[0].score * 2 + r[1].score`.
- Typo tolerance and ranking must match OpenCode, not merely subsequence presence.
- Either use a compatible Rust fuzzysort implementation or generate golden ranking tests from OpenCode.
- Filtering must exclude disabled options.
- Filtering must preserve category grouping because `CommandPaletteDialog` does not pass `flat`.
- Filtering must not duplicate suggested rows when filter is non-empty.
- Empty filter must prepend suggested duplicates in a `Suggested` category before the full option list.
- Filtered results must not use category-as-footer due to flat mode.

Acceptance tests:

- Golden ranking tests for representative queries with typos, category matches, title matches, mixed title/category matches, empty filter, and no-result cases.
- Tests must include queries where handmade subsequence matching would rank differently from fuzzysort.
- Tests must prove suggested duplicates appear only with empty filter.
- Tests must prove grouped filtered output remains grouped.

### Grouping And Rows

Harness must match OpenCode row semantics:

- Group by category.
- Render category section headers.
- Preserve category order according to OpenCode output order.
- Suggested duplicates use value prefix semantics equivalent to `suggested:${command}` or a Harness equivalent that prevents selection identity collisions.
- Normal and suggested duplicate selection dispatch the same underlying command.
- Details rows must render if command options provide details.
- Command palette must not render current-dot/gutter unless OpenCode `CommandPaletteDialog` starts passing `current` or `gutter`.

### Footer Semantics

Harness currently uses footer as action keybinding else category label. This must be corrected.

Requirements:

- Footer must be formatted registered keybindings where OpenCode provides bindings.
- Footer must not fall back to category label in command palette.
- Footer actions must match `DialogSelect` behavior where applicable.
- If no keybinding exists, footer should be blank/absent, not category-as-footer.
- Registered bindings override entry bindings as OpenCode does.

### Keyboard Navigation

Harness must match `DialogSelect` behavior:

| Input | Expected Behavior |
|---|---|
| Up | Previous item; wraps from first to last. |
| Down | Next item; wraps from last to first. |
| PageUp | Move by -10. |
| PageDown | Move by +10. |
| Home | First item. |
| End | Last item. |
| Enter | Select current item or focused footer action. |
| Tab | Next footer action when footer actions exist. |
| Shift+Tab | Previous footer action when footer actions exist. |
| Esc | Close dialog. |

Acceptance tests must cover wraparound, page step, home/end, enter dispatch, and no-op behavior when locked/empty where applicable.

### Mouse Semantics

Harness must match OpenCode `DialogSelect` mouse behavior:

- Mouse move switches input mode to mouse unless locked.
- Mouse over moves selection only when input mode is mouse.
- Mouse down moves selection.
- Mouse up selects the option.
- Footer action mouse up triggers the action.
- Filtering resets input mode to keyboard to avoid synthetic mouse movement changing selection.

Acceptance requires PTY/manual evidence or deterministic event simulation if the TUI testkit can simulate mouse events.

### Rendering

Harness must match visible structure:

- Dim backdrop.
- `Commands` title.
- `esc` header affordance.
- Search input with cursor behavior consistent with Harness rendering primitives and OpenCode intent.
- Grouped rows with active highlight.
- Category headers styled distinctly.
- Footer keybinding text, not category fallback.
- No command-palette current dot.
- No placeholder banners for included commands.
- No Harness-only rows in parity mode.

Render tests must include:

- Empty filter with suggested and non-suggested groups.
- Non-empty filter with no suggested duplicates.
- No-result state.
- Active selection.
- Long title/description truncation behavior.
- Footer present and footer absent rows.
- Harness-only exclusion.

## Implementation Milestones With Dependencies And Stop Gates

### Milestone 1: Freeze The Reference Inventory

Depends on: none.

Work:

- Build an OpenCode command inventory from the listed OpenCode anchors.
- Build a Harness command inventory from `palette_model.rs`.
- Include slash names and aliases from OpenCode command sources.
- Record exact status for every command: included, excluded, Harness-only secondary, missing, or placeholder.
- Add tests that assert current known Harness facts before changing behavior:
  - 59 total entries.
  - 46 OpenCode-parity entries.
  - 13 Harness-only entries.
  - 26 placeholders.

Stop gate:

- No implementation changes until the inventory test/tool exposes every command and placeholder explicitly.
- Stop if OpenCode source reveals command IDs or aliases not represented in the matrix.

### Milestone 2: Decide Scope For Every Placeholder And Harness-Only Entry

Depends on: Milestone 1.

Work:

- For each `PaletteDispatch::Placeholder`, choose:
  - Implement as OpenCode parity.
  - Exclude with source-backed rationale.
- For each Harness-only command, choose:
  - Move to secondary surface.
  - Hide from parity palette behind explicit non-parity mode.
  - Remove from palette if redundant.
- Document every decision in the inventory model and tests.

Stop gate:

- No placeholder may remain for an included OpenCode-parity command.
- No Harness-only command may remain visible in exact OpenCode-copy Ctrl+P.

### Milestone 3: Replace Filtering With Fuzzysort-Compatible Ranking

Depends on: Milestone 1.

Work:

- Replace handmade weighted fuzzy subsequence matching with fuzzysort-compatible ranking or golden-generated equivalent.
- Preserve OpenCode score weighting: title weight 2, category weight 1.
- Preserve grouping while filtered.
- Preserve suggested duplication only for empty filter.
- Remove category-as-footer behavior from command palette.

Stop gate:

- Golden ranking tests must fail under the old handmade matcher and pass under the new matcher.
- Tests must prove filtered results remain grouped because `flat` is not enabled.

### Milestone 4: Correct DialogSelect UX Semantics

Depends on: Milestone 3.

Work:

- Implement keyboard wraparound, page +/-10, home/end, enter, tab/shift-tab action focus.
- Implement mouse move/down/up/over semantics or document a TUI-platform limitation with test-backed fallback.
- Remove current-dot/gutter from command palette if present.
- Correct footer behavior.
- Ensure dialog clears before dispatch.

Stop gate:

- Deterministic render tests and palette owner tests must cover navigation, mouse semantics where possible, footer, current/gutter absence, and close-before-dispatch.

### Milestone 5: Implement Or Exclude Per-Command Behavior

Depends on: Milestone 2 and Milestone 4.

Work:

- Implement real dispatch behavior for each included command.
- Keep runtime invariants in coordinator/core; TUI dispatch may request intents but must not own core invariants.
- Add availability rules matching OpenCode.
- Add failure paths with structured logs.
- Add per-command behavior tests.

Stop gate:

- Zero included commands dispatch to `Placeholder`.
- Every excluded command has a source-backed test proving absence.
- Every included command has at least one behavior test beyond registry presence.

### Milestone 6: Slash Alias Parity

Depends on: Milestone 1 and Milestone 5.

Work:

- Inventory slash names and aliases from OpenCode command sources.
- Map aliases to Harness commands or documented exclusions.
- Add tests for alias visibility, dispatch target, and hidden/availability rules.

Stop gate:

- Missing alias inventory is failure.
- Alias tests must fail if any OpenCode alias silently disappears.

### Milestone 7: Logging, Events, And Support Evidence

Depends on: Milestone 5.

Work:

- Improve palette logs with command ID, overlay/dialog transition, dispatch target, availability rejection reason, redacted session/provider/model IDs, and failure status.
- Add automated redaction and secret scans for palette logs/support bundles.
- Verify events/logs for local toggles, dialog commands, and live provider/session-affecting commands.

Stop gate:

- No live parity claim without logs/events.
- No support bundle evidence without redaction/secret scan.

### Milestone 8: Full Evidence Signoff

Depends on: Milestones 1-7.

Work:

- Run deterministic tests.
- Run render tests.
- Run fast and quality gates.
- Run PTY/signoff/live lanes only when claiming those evidence types.
- Complete live-provider dogfood action list.
- Update docs/evidence.

Stop gate:

- Done definition must be satisfied in full.
- If any evidence lane cannot run, the final status must explicitly say which parity claim remains unproven.

## Per-Command Acceptance Checklist

For every command in the inventory, complete this checklist:

| Check | Required Result |
|---|---|
| OpenCode source anchor | File and line/function/source section recorded. |
| Harness source anchor | Registry entry and dispatch implementation recorded. |
| Command ID | Exact OpenCode ID or documented Harness-only status. |
| Title | Static/dynamic title matches OpenCode. |
| Description | Matches OpenCode or documented copy decision. |
| Category | Matches OpenCode. |
| Footer | Registered keybinding footer; no category fallback. |
| Suggested | Matches OpenCode boolean/function behavior. |
| Visibility | Hidden and `command.palette.show` rules match OpenCode. |
| Availability | Route/session/provider/workspace/prompt state matches OpenCode. |
| Dispatch | Real behavior implemented or command excluded. |
| Placeholder | Not allowed for included commands. |
| Slash aliases | Inventory and tests complete where applicable. |
| Tests | Behavior test plus inventory/render coverage where relevant. |
| Logs | Success/failure/rejection logs with redaction. |
| Evidence | Unit/render/PTY/live evidence as appropriate for command type. |

### High-Risk Command Decision Matrix

These commands must not be left as ambiguous placeholders:

| Group | Command | Required Acceptance |
|---|---|---|
| Session | `session.rename` | Rename works on current session or command is excluded with OpenCode scope decision and absence test. |
| Session | `session.timeline` | Jump-to-message UI works or command is excluded with OpenCode scope decision and absence test. |
| Session | `session.fork` | Fork behavior works or command is excluded with OpenCode scope decision and absence test. |
| Session | `session.unshare` | Visible only with share URL; removes share URL; logs redacted result; or excluded. |
| Session | `session.undo` | Visible/enabled only with revert state; reverts expected message; event/log evidence; or excluded. |
| Session | `session.redo` | Visible/enabled only with redo state; restores expected message; event/log evidence; or excluded. |
| Session | `session.toggle.conceal` | Toggle works, title updates, render/evidence proves effect; or excluded. |
| Session | `session.copy` | Copy behavior works with observable clipboard/status/log evidence; or excluded. |
| Session | `session.move` | Move behavior works with workspace/session state evidence; or excluded. |
| Workspace | `workspace.set` | Workspace selector/set behavior works and state persists as OpenCode-equivalent; or excluded. |
| Workspace | `workspace.copy_path` | Copy path works with safe redaction/log evidence; or excluded. |
| Workspace | `workspace.list` | Workspace list dialog works; or excluded. |
| Provider | `variant.list` | Variant/model picker works with provider/model state; or excluded. |
| Provider | `console.org.switch` | Visible only when switchable org count permits; switches org; or excluded. |
| Prompt | `prompt.editor_context.clear` | Visible only with editor context; clears context; or excluded. |
| Prompt | `prompt.skills` | Skills dialog works and slash aliases are tested; or excluded. |
| System | `app.debug` | Debug command works or excluded. |
| System | `app.console` | Console command works or excluded. |
| System | `app.heap_snapshot` | Heap snapshot works safely or excluded. |
| System | `terminal.title.toggle` | Toggle works, title updates, logs result; or excluded. |
| System | `app.toggle.animations` | Toggle works, title updates; or excluded. |
| Prompt/System | `app.toggle.file_context` | Toggle works, title updates, prompt context behavior tested; or excluded. |
| Prompt/System | `app.toggle.diffwrap` | Toggle works, title updates, render effect tested; or excluded. |
| Prompt/System | `app.toggle.paste_summary` | Toggle works, title updates, prompt behavior tested; or excluded. |
| System | `app.toggle.session_directory_filter` | Toggle works, title updates, session list behavior tested; or excluded. |
| System | `tips.toggle` | Toggle works, title updates, tips visibility tested; or excluded. |

## TDD-Oriented Execution Plan

Every behavior change must follow red/green/refactor:

1. Write an inventory, unit, render, or integration test that fails for the current gap.
2. Implement the smallest behavior change that passes the test.
3. Refactor only with tests green.
4. Run the targeted owner tests before moving to the next command/group.
5. Keep changes atomic by command group or UI subsystem.

Required test-first examples:

- Before replacing filter ranking, add golden tests that demonstrate current subsequence ranking differs from OpenCode fuzzysort.
- Before changing suggested duplication, add tests for empty filter and non-empty filter behavior.
- Before replacing placeholder dispatch, add a per-command test that fails because placeholder is reached.
- Before removing category fallback footer, add render tests proving footer is blank or keybinding-only.
- Before hiding Harness-only commands, add tests proving they currently appear in parity palette, then make them pass by moving/hiding them.
- Before adding logs, add log-capture tests for command ID, dialog transition, dispatch target, rejection reason, redaction, and failure status.

## Testing And Evidence Strategy

### Deterministic Unit And Owner Tests

Required commands:

- `cargo test -p harness-tui --test deterministic_render_test`
- Palette owner tests covering `crates/harness-tui/src/app/tests/palette_parity_tests.rs`
- Targeted tests for inventory, filtering, availability, dispatch, slash aliases, logging, and exclusion decisions.

Palette owner tests must cover:

- Open/close.
- Filter ranking and typo tolerance.
- Suggested duplicates only when filter empty.
- No suggested duplicates when filtering.
- Category grouping while filtered.
- Registered binding footer.
- No category-as-footer fallback.
- Hidden/excluded commands absent.
- Harness-only commands absent from parity palette.
- Dynamic titles.
- Availability rejection reasons.
- Zero placeholders for included commands.
- Dialog clear before dispatch.
- Navigation wraparound/page/home/end.
- Current/gutter absence.
- Rendered rows and no-result state.

### Lane Verification

Run the following as appropriate:

- `scripts/test-lanes.sh fast`
- `scripts/test-lanes.sh quality-gates`
- `cargo test -p harness-tui --test deterministic_render_test`
- Palette owner test target(s)
- Signoff/PTY/live lanes only when claiming those evidence types:
  - `scripts/test-lanes.sh signoff-pty`
  - `scripts/test-lanes.sh signoff-binary`
  - live provider lane if available in the project’s lane runner
  - `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e` when PTY evidence is claimed

If a lane is unavailable or intentionally skipped, the final implementation report must state which claim remains unverified.

### PTY And Manual TUI Evidence

Required when claiming visual/interactive parity:

- Capture Ctrl+P opening.
- Capture empty filter with suggested duplicates.
- Capture non-empty filter with no suggested duplicates.
- Capture grouped filtered output.
- Capture footer keybindings.
- Capture no current dot.
- Capture keyboard wraparound/page/home/end.
- Capture mouse selection if supported by the evidence harness.
- Capture command dispatch closing dialog.

Evidence must include artifact paths, command used to capture, timestamp, git revision, and environment details.

### Live Provider Dogfood

Required concrete live-provider action list:

1. Start Harness TUI with a real configured provider.
2. Run a live prompt to establish baseline provider/session behavior.
3. Open Ctrl+P.
4. Filter for a known local toggle command.
5. Select the local toggle command.
6. Verify title/state/render/log update.
7. Open Ctrl+P again.
8. Filter for a dialog command such as model/variant/session/workspace selector.
9. Select the dialog command.
10. Verify the dialog opens, selection works, and logs record transition/dispatch.
11. Open Ctrl+P again.
12. Filter for a live provider/session-affecting command.
13. Select the live command.
14. Run a live prompt after the command.
15. Inspect events and logs for expected session/provider/model effects.
16. Export support bundle.
17. Run automated redaction/secret scan on logs and support bundle.
18. Record artifact paths and command transcript.

Live dogfood must include at least:

- One local UI toggle.
- One dialog-opening command.
- One provider/session-affecting command.
- One prompt before and after the provider/session-affecting command.
- Event/log inspection.
- Support bundle export.
- Secret scan.

### Log And Event Inspection

Required checks:

- Palette command ID appears in dispatch logs.
- Overlay/dialog transition appears in logs.
- Dispatch target appears in logs.
- Availability rejection reason appears when a command is unavailable.
- Session/provider/model IDs are present only in redacted or safe form.
- Failure status is logged for failed dispatch.
- No raw auth headers, cookies, API keys, PEM blocks, provider raw requests, provider raw responses, or hidden reasoning text appear.
- Replay/session inspection remains replay-derived and side-effect free.

## Logging And Debug Requirements

Add structured logs for palette lifecycle and dispatch.

Required fields:

| Field | Requirement |
|---|---|
| `palette.command_id` | Stable command ID for every dispatch attempt. |
| `palette.dialog_state` | Opened, filtered, selected, closed, dispatch_started, dispatch_succeeded, dispatch_failed, rejected. |
| `palette.filter_length` | Length only; do not log sensitive query content unless explicitly redacted and approved. |
| `palette.dispatch_target` | Action, UI intent, dialog, local toggle, provider/session operation, excluded, or failure. |
| `palette.availability_reason` | Required when hidden/disabled/rejected. |
| `session.id` | Redacted or stable safe identifier only. |
| `provider.id` | Redacted or stable safe identifier only. |
| `model.id` | Redacted or stable safe identifier only. |
| `workspace.id/path` | Redacted according to project policy. |
| `status` | Success, failure, rejected, skipped. |
| `error.kind` | Typed failure kind where applicable; no raw secret-bearing messages. |

Required log tests:

- Success dispatch log.
- Failure dispatch log.
- Availability rejection log.
- Dialog open/close transition log.
- Redaction test for session/provider/model/workspace identifiers.
- Secret scan over generated log/support-bundle artifacts.

## Slash Alias Requirements

OpenCode slash names and aliases are parity surface because OpenCode derives slash entries from palette commands.

Requirements:

- Inventory every slash name and alias from OpenCode command sources.
- Map each slash alias to the corresponding palette command.
- Include hidden/availability behavior for slash aliases.
- Add tests proving:
  - Alias exists when OpenCode exposes it.
  - Alias dispatches the same command target.
  - Alias is absent when the underlying command is excluded.
  - No extra compatibility aliases exist unless OpenCode has them.

The implementation loop must not declare parity complete until slash alias inventory and tests are complete.

## Guardrails For Loop Agents

- Treat `inspirations/opencode` as read-only reference.
- Start every command/group by adding a failing test.
- Do not leave `PaletteDispatch::Placeholder` for included commands.
- Do not hide failures behind placeholder banners.
- Do not add compatibility aliases unless OpenCode has them.
- Do not let Harness-only commands appear in the exact OpenCode-copy Ctrl+P palette.
- Do not flatten filtered command palette results; `CommandPaletteDialog` does not pass `flat`.
- Do not use category-as-footer in command palette.
- Do not show current-dot/gutter in command palette.
- Do not claim OpenCode command palette uses current markers.
- Do not remove or weaken tests.
- Do not move coordinator/core invariants into TUI code.
- Do not execute providers, hooks, MCP, network, or CLI from replay-derived session inspection.
- Do not claim visual evidence without PTY/snapshot provenance.
- Do not claim live evidence without logs/events/support-bundle artifacts.
- Do not store raw provider requests/responses, auth headers, cookies, API keys, PEM blocks, or hidden reasoning text.
- Keep changes surgical and atomic.

## Atomic Commit Strategy

Use small commits that can be reviewed and reverted independently:

1. Inventory and baseline tests.
2. Scope decisions and exclusion tests.
3. Fuzzysort-compatible filtering and golden ranking tests.
4. DialogSelect navigation/mouse/footer/render parity.
5. Harness-only palette separation.
6. Session command implementations/exclusions.
7. Workspace command implementations/exclusions.
8. Provider command implementations/exclusions.
9. Prompt and slash alias parity.
10. System/local toggle command implementations/exclusions.
11. Logging/redaction/support-bundle evidence.
12. Final docs/evidence updates and lane signoff.

Each commit must include:

- The failing test that motivated the change.
- The minimal implementation to pass.
- Updated inventory status if command status changed.
- Targeted test output in the implementation report.

Do not combine unrelated command groups with UI infrastructure changes unless one directly depends on the other.

## Done Definition

Parity is done only when all of the following are true:

- Inventory is complete and source-anchored to OpenCode and Harness.
- All 59 current Harness entries have an explicit final status.
- All 46 current OpenCode-parity entries are implemented or explicitly excluded.
- All 13 current Harness-only entries are absent from exact OpenCode-copy Ctrl+P or moved to a deliberate secondary surface.
- Included command placeholder count is zero.
- Every command listed in the high-risk decision matrix is implemented or explicitly excluded with tests.
- Availability mirrors OpenCode for route/session, worktree workspace, model variants, switchable org count, editor context, prompt input, stash, share URL, revert state, provider connected, and replay/session state.
- Filtering empirically matches OpenCode fuzzysort behavior.
- Suggested duplicates appear only when filter is empty.
- Filtered results remain grouped.
- Footer semantics match OpenCode registered keybindings.
- Command palette does not use category-as-footer.
- Command palette does not show current-dot/gutter.
- Keyboard navigation matches OpenCode.
- Mouse semantics are implemented or platform limitation is documented with tests and evidence.
- Slash alias inventory and alias tests are complete.
- Logs include command ID, overlay/dialog transition, dispatch target, availability rejection reason, redacted IDs, and failure status.
- Automated redaction/secret scans pass.
- Deterministic unit/render tests pass.
- Palette owner tests pass.
- `scripts/test-lanes.sh fast` passes.
- `scripts/test-lanes.sh quality-gates` passes.
- PTY/manual TUI evidence exists for visual/interactive claims.
- Live-provider dogfood evidence exists for live claims.
- Support bundle export and secret scan evidence exists.
- Docs/evidence are updated with artifact paths and verification commands.
- No OpenCode reference files were modified.
- Runtime invariants remain in coordinator/core.
