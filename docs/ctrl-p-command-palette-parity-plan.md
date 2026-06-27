# Ctrl+P Command Palette Parity Plan

**Generated:** 2026-06-27  
**Status:** PRD / implementation plan; not implementation evidence  
**Provenance:** Produced from `/hyperplan` adversarial planning for Harness `Ctrl+P` parity with Opencode. The source of truth is the Opencode source under `inspirations/opencode/`, not memory or screenshots.

## Objective

Bring the Harness TUI `Ctrl+P` command palette to 1:1 parity with Opencode for every visible/reachable palette command except the explicit user exclusions below.

The implementation must preserve Harness runtime boundaries:

- Palette inventory, rendering, filtering, and local toggles belong in `crates/harness-tui`.
- Runtime/session side effects must go through existing or new safe `UiIntent` / coordinator / live-runtime pathways.
- Palette dispatch must not directly call providers, perform network I/O, write files, append events, run tools, or bypass permissions.

## Explicit Non-Goals and Exclusions

| User exclusion | Command IDs | Required behavior |
|---|---|---|
| Share session | `session.share` | Must not appear, including the dynamic `Copy share link` title when already shared. |
| Open editor | `prompt.editor` | Must not appear. Do not exclude `prompt.editor_context.clear`, `prompt.skills`, or prompt stash commands. |
| Theme settings | `theme.switch`, `theme.switch_mode`, `theme.mode.lock` | Conservatively exclude all first-party theme commands unless the user later narrows this mapping. |
| Help | `help.show` | Must not appear. |
| Open docs | `docs.open` | Must not appear. |
| Open diff viewer | `diff.open` | Must not appear. |

Hidden Opencode commands are also non-targets. Do not count them as missing parity:

- `command.palette.show`
- quick-switch slots
- model cycle / recent / favorite-cycle commands
- agent cycle / reverse-cycle commands
- `terminal.suspend`
- `prompt.clear`, `prompt.submit`, `prompt.paste`, `session.interrupt`
- session scroll/page/line/first/last/jump-last-user/message next/previous/background/child-parent navigation commands
- which-key commands from `inspirations/opencode/packages/tui/src/feature-plugins/system/which-key.tsx:537`, because the read evidence shows no `namespace: "palette"`

`session.unshare` is not excluded. It is distinct from `session.share` and must be included unless the user later excludes all sharing controls.

## Source-of-Truth References

| Area | Source |
|---|---|
| Opencode palette semantics | `inspirations/opencode/packages/tui/src/component/command-palette.tsx:15` |
| Opencode select dialog filtering, grouping, navigation | `inspirations/opencode/packages/tui/src/ui/dialog-select.tsx:149` |
| Opencode app/global commands | `inspirations/opencode/packages/tui/src/app.tsx:549` |
| Opencode prompt/stash commands | `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:330` |
| Opencode session commands | `inspirations/opencode/packages/tui/src/routes/session/index.tsx:458` |
| Opencode plugin commands | `inspirations/opencode/packages/tui/src/feature-plugins/system/plugins.tsx:238`, `inspirations/opencode/packages/tui/src/feature-plugins/home/tips.tsx:10` |
| Explicitly excluded diff command | `inspirations/opencode/packages/tui/src/feature-plugins/system/diff-viewer.tsx:1053` |
| Non-target which-key commands | `inspirations/opencode/packages/tui/src/feature-plugins/system/which-key.tsx:537` |
| Current Harness command registry | `crates/harness-tui/src/keybindings/command_registry.rs` |
| Current Harness palette filtering, availability, dispatch | `crates/harness-tui/src/app/session_navigation.rs:531` |
| Current Harness key/action surface | `crates/harness-tui/src/keybindings.rs:17` |
| Current Harness dispatch | `crates/harness-tui/src/app/key_interaction.rs:701` |

Implementation agents must read the root `AGENTS.md`, `crates/harness-tui/AGENTS.md`, and `crates/harness-core/AGENTS.md` before editing the relevant crates.

## Opencode Palette Semantics to Match

Harness must match these behaviors from Opencode:

1. `Ctrl+P` opens the command palette dialog.
2. The palette queries commands with `namespace: "palette"` and `visibility: "reachable"`.
3. Hidden commands and `command.palette.show` are excluded.
4. Commands are identified by stable command ID. Labels are not the contract.
5. Keybinding footers come from registered keybindings, not static strings.
6. Empty filter duplicates suggested commands into a synthetic `Suggested` group with values prefixed by `suggested:<id>`.
7. Non-empty filter has no synthetic suggested duplicates.
8. Fuzzy filtering uses title and category only, with title weighted higher than category.
9. Internal command IDs are not filter keys.
10. Categories remain grouped in the command palette, including filtered results.
11. No-result text is exactly `No results found`.

## Parity Matrix Requirements

Before broad implementation work, create an executable parity matrix. It may live in Rust test data, a fixture module, or a small checked-in support document, but tests must consume it.

Each matrix entry must include:

| Field | Requirement |
|---|---|
| `id` | Stable command ID. All parity assertions use this field. |
| `origin` | Opencode source file and line reference. |
| `status` | `included`, `excluded`, `hidden_non_target`, or `harness_only`. |
| `category` | Opencode category. |
| `title_rule` | Static title or dynamic title conditions. |
| `description_rule` | Description rule if present. |
| `suggested_rule` | Exact suggested condition. |
| `availability_rule` | State-based visibility/enabled rule. |
| `footer_bindings_rule` | How keybinding footer is derived. |
| `harness_equivalent` | Existing action/dialog/intent or `missing`. |
| `dispatch_path` | Safe TUI/AppState/UiIntent/coordinator path. |
| `tests` | Contract, render, filter, navigation, dispatch, and absence tests. |

## Included Command Matrix Seed

The implementation matrix must include at least these non-excluded visible/reachable Opencode palette commands.

### App / Global Origin

Source: `inspirations/opencode/packages/tui/src/app.tsx:549`

| ID | Category | Title / dynamic rule | Notes |
|---|---|---|---|
| `session.list` | Session | Switch session | Suggested when sessions exist; slash sessions/resume/continue. |
| `session.new` | Session | New session | Suggested on session route; slash new/clear. |
| `workspace.copy_path` | Workspace | Copy worktree path | Enabled when current worktree workspace has directory. |
| `workspace.list` | Workspace | Manage workspaces | Hidden unless workspace feature is enabled; slash workspaces. |
| `model.list` | Agent | Switch model | Suggested true; slash models/mo. |
| `agent.list` | Agent | Switch agent | Slash agents. |
| `mcp.list` | Agent | Toggle MCPs | Slash mcps. |
| `variant.cycle` | Agent | Variant cycle | Visible/reachable palette command. |
| `variant.list` | Agent | Switch model variant | Hidden when no variants; slash variants. |
| `provider.connect` | Provider | Connect provider | Suggested when disconnected; slash connect. |
| `console.org.switch` | Provider | Switch org | Present only when multiple orgs are switchable. |
| `opencode.status` | System | View status | Slash status. |
| `app.exit` | System | Exit the app | Slash exit/quit/q. |
| `app.debug` | System | Toggle debug panel | Local TUI state. |
| `app.console` | System | Toggle console | Local TUI state. |
| `app.heap_snapshot` | System | Write heap snapshot | Must route safely; no direct palette file write if Harness architecture forbids it. |
| `terminal.title.toggle` | System | Enable/Disable terminal title | Dynamic title. |
| `app.toggle.animations` | System | Enable/Disable animations | Dynamic title. |
| `app.toggle.file_context` | System | Enable/Disable file context | Dynamic title. |
| `app.toggle.diffwrap` | System | Enable/Disable diff wrapping | Dynamic title. |
| `app.toggle.paste_summary` | System | Enable/Disable paste summary | Dynamic title. |
| `app.toggle.session_directory_filter` | System | Enable/Disable session directory filtering | Dynamic title. |

Excluded from this origin: `command.palette.show`, `theme.switch`, `theme.switch_mode`, `theme.mode.lock`, `help.show`, `docs.open`, plus hidden-only non-targets.

### Prompt / Stash Origin

Source: `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:330`

| ID | Category | Title / dynamic rule | Notes |
|---|---|---|---|
| `prompt.editor_context.clear` | Prompt | Remove editor context | Enabled when editor context exists. |
| `prompt.skills` | Prompt | Skills | Opens skill picker and inserts slash skill. |
| `workspace.set` | Session | Warp | Enabled behind workspace flag; slash warp. |
| `session.move` | Session | Move session | Slash move. |
| `prompt.stash` | Prompt | Stash prompt | Enabled when prompt input exists. |
| `prompt.stash.pop` | Prompt | Stash pop | Enabled when stash is non-empty. |
| `prompt.stash.list` | Prompt | Stash list | Enabled when stash is non-empty. |

Excluded from this origin: `prompt.editor`. Hidden non-targets: `prompt.clear`, `prompt.submit`, `prompt.paste`, `session.interrupt`.

### Session Origin

Source: `inspirations/opencode/packages/tui/src/routes/session/index.tsx:458`

| ID | Category | Title / dynamic rule | Notes |
|---|---|---|---|
| `session.rename` | Session | Rename session | Slash rename. |
| `session.timeline` | Session | Jump to message | Slash timeline. |
| `session.fork` | Session | Fork session | Slash fork. |
| `session.compact` | Session | Compact session | Slash compact/summarize. |
| `session.unshare` | Session | Unshare session | Enabled when share URL exists; slash unshare. |
| `session.undo` | Session | Undo previous message | Slash undo. |
| `session.redo` | Session | Redo | Enabled when revert message exists; slash redo. |
| `session.sidebar.toggle` | Session | Show/Hide sidebar | Dynamic title. |
| `session.toggle.conceal` | Session | Enable/Disable code concealment | Dynamic title. |
| `session.toggle.timestamps` | Session | Show/Hide timestamps | Dynamic title; slash timestamps/toggle-timestamps. |
| `session.toggle.thinking` | Session | Expand/Collapse thinking | Dynamic title; slash thinking/toggle-thinking. |
| `session.toggle.actions` | Session | Show/Hide tool details | Dynamic title. |
| `session.toggle.scrollbar` | Session | Toggle session scrollbar | Visible/reachable palette command. |
| `session.toggle.generic_tool_output` | Session | Show/Hide generic tool output | Dynamic title. |
| `messages.copy` | Session | Copy last assistant message | Clipboard path must be safe and testable. |
| `session.copy` | Session | Copy session transcript | Slash copy. |
| `session.export` | Session | Export session transcript | Slash export. |

Excluded from this origin: `session.share`. Hidden scroll/navigation/background/child-parent commands are non-targets.

### First-Party Plugin Origin

| ID | Category | Title / dynamic rule | Source |
|---|---|---|---|
| `plugins.list` | System | Plugins | `inspirations/opencode/packages/tui/src/feature-plugins/system/plugins.tsx:238` |
| `plugins.install` | System | Install plugin | `inspirations/opencode/packages/tui/src/feature-plugins/system/plugins.tsx:238` |
| `tips.toggle` | System | Show tips / Hide tips | `inspirations/opencode/packages/tui/src/feature-plugins/home/tips.tsx:10` |

Excluded plugin command: `diff.open` from `inspirations/opencode/packages/tui/src/feature-plugins/system/diff-viewer.tsx:1053`.

## Current Harness Divergences to Resolve

Current Harness palette behavior is in `crates/harness-tui/src/keybindings/command_registry.rs` and `crates/harness-tui/src/app/session_navigation.rs:531`.

Known divergences:

- Static registry with incomplete Opencode coverage.
- Filtering can match internal IDs; Opencode filters title/category only.
- Filtered results are flattened; Opencode keeps categories grouped.
- Shortcut/footer text is static in places; Opencode derives from registered keybindings.
- Toggle rows are split into show/hide entries where Opencode uses one dynamic command ID.
- Some Harness-only or hidden-equivalent rows may be visible and must not count as Opencode parity.
- Prompt, plugin, workspace, provider, and some session commands are missing or need safe Harness equivalents.

## Architecture Constraints

1. Keep palette inventory, filtering, rendering, and local presentation in TUI/AppState.
2. Dispatch only to TUI dialogs, local AppState mutation, or safe `UiIntent` / coordinator / live-runtime paths.
3. Do not direct-call providers, network, filesystem writes, event appends, tools, or permission-sensitive operations from palette dispatch.
4. Preserve event-sourced runtime invariants. Coordinator remains the authority for event append, scheduling, permissions, hooks, compaction, and lifecycle.
5. Avoid public config changes. If unavoidable, update docs/configs/tests as required by root `AGENTS.md`.
6. Harness-only commands may remain only when explicitly marked Harness-only and excluded from Opencode parity accounting.
7. Dynamic Opencode rows must be one command ID with dynamic title/availability, not separate show/hide IDs.
8. Exclusions must be exact command-ID rules, not substring matching.

## Milestones and Checklists

### Wave 1: Ground Truth and Test Foundation

- [ ] **1. Source Audit and Parity Matrix**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: none
  - Blocks: tasks 2 and 3
  - Proof required:
    - [ ] Matrix includes every seed included command ID.
    - [ ] Matrix includes every explicit exclusion.
    - [ ] Matrix includes hidden-only non-targets.
    - [ ] Matrix distinguishes Opencode parity entries from Harness-only entries.
    - [ ] At least one contract test consumes the matrix.

- [ ] **2. Palette Contract Test Harness**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: task 1
  - Blocks: tasks 3 and 4
  - Proof required:
    - [ ] Tests drive `Ctrl+P`, typed filters, arrows/page/home/end, Enter, Esc/Ctrl+C.
    - [ ] Tests observe rendered rows or resulting AppState/UiIntent.
    - [ ] Tests fail against current divergences.
    - [ ] Tests do not mutate `palette_filtered` or equivalent internal result state directly.

### Wave 2: Core Palette Semantics

- [ ] **3. Inventory Model and Registry Refactor**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: tasks 1 and 2
  - Blocks: tasks 4, 5, 6, and 8
  - Proof required:
    - [ ] Registry uses stable command IDs as contract keys.
    - [ ] Dynamic rows use one command ID.
    - [ ] Registry can mark Harness-only commands explicitly.
    - [ ] Existing Harness commands either map to Opencode IDs or are marked Harness-only.

- [ ] **4. Opencode Filtering, Grouping, Navigation**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: tasks 2 and 3
  - Blocks: tasks 5 and 9
  - Proof required:
    - [ ] Fuzzy search keys are title and category only.
    - [ ] Title weighting is higher than category weighting.
    - [ ] Command IDs do not match filter text.
    - [ ] Filtered results preserve categories.
    - [ ] Empty state is exactly `No results found`.
    - [ ] Keyboard navigation is bounded and works across grouped rows.

### Wave 3: Dynamic Presentation and Dispatch

- [ ] **5. Dynamic Availability, Labels, Suggested Rows, Footers**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: tasks 3 and 4
  - Blocks: tasks 6, 8, and 9
  - Proof required:
    - [ ] Empty filter duplicates suggested commands into synthetic `Suggested` rows with value prefix `suggested:<id>`.
    - [ ] Non-empty filter has no suggested duplicates.
    - [ ] Footer bindings come from registered keybindings.
    - [ ] Dynamic labels match state for all toggle commands.
    - [ ] Availability rules are covered for required representative states.

- [ ] **6. Safe Dispatch Path Completion**
  - Category: `ultrabrain`
  - Skills: `karpathy-guidelines`, `rust-best-practices`, `rust-async-patterns`
  - Depends on: tasks 3 and 5
  - Blocks: tasks 7, 8, and 9
  - Proof required:
    - [ ] Every included command has a dispatch path or explicit milestone-linked missing path.
    - [ ] Dispatch tests verify dialog/AppState/UiIntent outcomes through Enter selection.
    - [ ] Disabled/unavailable commands cannot execute.
    - [ ] No direct provider/network/file/event/tool side effects occur from palette dispatch.

### Wave 4: Missing Bridges and Cleanup

- [ ] **7. Missing Dialogs and UiIntent Bridges**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`, `rust-async-patterns`
  - Depends on: task 6
  - Blocks: task 9
  - Proof required:
    - [ ] Missing commands open appropriate Harness dialogs or emit safe intents.
    - [ ] Side effects remain coordinator/runtime-routed.
    - [ ] Commands that cannot yet be functionally completed are escalated or represented as explicit safe placeholder dialogs with tests; they must not fake success.

- [ ] **8. Harness-Only and Exclusion Cleanup**
  - Category: `quick` for simple registry cleanup, `deep` if cleanup spans dispatch/render tests
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: tasks 3, 5, and 6
  - Blocks: task 9
  - Proof required:
    - [ ] Explicit exclusions are absent in all representative states.
    - [ ] Hidden Opencode commands are absent.
    - [ ] Harness-only commands are labeled and excluded from parity totals.
    - [ ] Split show/hide rows are collapsed where Opencode uses one dynamic ID.
    - [ ] Absence tests assert exact IDs, not labels or substrings.

### Wave 5: Acceptance and Dogfood

- [ ] **9. State Matrix Acceptance Coverage**
  - Category: `deep`
  - Skills: `karpathy-guidelines`, `rust-best-practices`
  - Depends on: tasks 4, 5, 6, 7, and 8
  - Blocks: task 10
  - Proof required for each state:
    - [ ] home with no sessions
    - [ ] home with sessions and disconnected provider
    - [ ] live session idle
    - [ ] live session with prompt input, stash, and editor context
    - [ ] live shared session
    - [ ] live session with revert
    - [ ] provider connected/disconnected
    - [ ] variants present/absent
    - [ ] workspace feature flag on/off
    - [ ] review surface open
    - [ ] startup shell
    - [ ] each state asserts included IDs, excluded IDs, categories, dynamic labels, and suggested duplicates where applicable

- [ ] **10. Dogfood, PTY, Render, Fast Lane Verification**
  - Category: `unspecified-high`
  - Skills: `karpathy-guidelines`, `rust-best-practices`, `visual-qa`
  - Depends on: task 9
  - Blocks: task 11
  - Proof required:
    - [ ] Run targeted `harness-tui` tests.
    - [ ] Run deterministic render tests.
    - [ ] Drive TUI/PTY or equivalent harness-testkit surface through `Ctrl+P` happy path.
    - [ ] Drive one bad/no-result query and verify `No results found`.
    - [ ] Run `scripts/test-lanes.sh fast` when the milestone touches cross-crate behavior.
    - [ ] Record command output and artifact paths in PR notes.

Recommended commands, adjusted to actual test names after implementation:

```bash
cargo test -p harness-tui --test deterministic_render_test
cargo test -p harness-tui palette
cargo test -p harness-tui command_palette
scripts/test-lanes.sh fast
```

### Wave 6: Final Packaging

- [ ] **11. Documentation, PRD Notes, Commit Split Review**
  - Category: `writing`; use `quick` for commit prep only if requested
  - Skills: `karpathy-guidelines`, `git-master` for commits
  - Depends on: tasks 1 through 10
  - Proof required:
    - [ ] PR notes cite Opencode source refs and Harness files changed.
    - [ ] Public docs/configs/tests are updated if any public contract changed.
    - [ ] Atomic commit strategy is followed if the user requests commits.
    - [ ] Final verification evidence is attached or summarized.
    - [ ] No unchecked milestone remains.

## Acceptance Test Groups

1. **Contract matrix tests**
   - Exact included command IDs by representative state.
   - Exact excluded IDs absent.
   - Categories and order.
   - Dynamic labels.
   - Footer bindings from registered keybindings.

2. **Palette interaction tests**
   - Drive `Ctrl+P`.
   - Type filters.
   - Use arrow/page/home/end.
   - Press Enter.
   - Press Esc/Ctrl+C.
   - Observe rendered rows and resulting AppState/UiIntent.

3. **Filtering tests**
   - Title search matches.
   - Category search matches.
   - ID-only search does not match.
   - Title weighting affects order.
   - Filtered results preserve categories.
   - No-result query renders `No results found`.

4. **Suggested tests**
   - Empty filter duplicates suggested commands into `Suggested`.
   - Synthetic values use `suggested:<id>`.
   - Non-empty filter removes suggested duplicates.
   - Selecting a suggested duplicate dispatches the same command ID.

5. **Dispatch tests**
   - Local toggles mutate AppState only.
   - Dialog commands open expected dialogs.
   - Runtime/session commands emit safe intents or coordinator-routed actions.
   - Disabled/unavailable commands do not dispatch.
   - Explicitly excluded commands cannot dispatch from palette.

6. **Representative state tests**
   - Cover every state listed in task 9.

## Anti-Gaming Rules

1. Do not prove palette behavior by mutating `palette_filtered` or equivalent internal state.
2. Do not assert labels only; parity must be by command ID.
3. Do not exclude commands by substring heuristics.
4. Do not count Harness-only commands as Opencode parity.
5. Do not count hidden Opencode commands as missing parity.
6. Do not implement direct side effects in palette dispatch.
7. Do not fake dispatch by setting final state without driving `Ctrl+P` and Enter in at least one acceptance path per command class.
8. Do not mark a milestone complete unless source refs are linked.
9. Do not mark a milestone complete unless matrix entries are updated.
10. Do not mark a milestone complete unless absence tests pass.
11. Do not mark a milestone complete unless dispatch path is exercised through `Ctrl+P`.
12. Do not mark a milestone complete unless Harness remains functional after the milestone.
13. Do not weaken tests to match implementation divergence; update implementation to match Opencode semantics unless the user approves an explicit Harness deviation.
14. Do not introduce public config changes without updating docs/configs/tests per `AGENTS.md`.

## Rollout Strategy

1. Land matrix and failing tests first.
2. Land registry/model refactor with minimal behavior change.
3. Land filtering/grouping/navigation parity.
4. Land dynamic presentation parity.
5. Land dispatch parity in safe command groups.
6. Land missing dialogs/intents and cleanup.
7. Land full state acceptance and dogfood evidence.
8. Prepare final PR notes and optional atomic commits.

## Key Risks and Mitigations

| Risk | Mitigation |
|---|---|
| Dynamic Opencode labels drift | Assert command IDs and dynamic title rules. |
| Harness lacks equivalent for some commands | Add safe dialog/intent/coordinator path; do not direct-call side effects. |
| Tests couple to implementation internals | Drive keyboard/render/AppState paths. |
| Suggested duplicate rows dispatch incorrectly | Map `suggested:<id>` back to the same command ID. |
| Hidden/excluded commands leak through | Maintain explicit negative matrix and absence tests. |
| Footer shortcuts drift | Resolve footers from registered keybindings dynamically. |
| Harness-only commands create scope creep | Mark Harness-only and exclude from parity accounting. |
| Public config changes broaden API | Avoid unless necessary; update docs/configs/tests if unavoidable. |

## Optional Commit Split

Only commit if the user explicitly requests it. Before committing, inspect `git status`, `git diff`, and recent log; stage only intended files.

Recommended split:

1. `test(tui): add command palette parity matrix`
2. `test(tui): cover opencode palette interaction semantics`
3. `refactor(tui): model command palette entries by stable id`
4. `fix(tui): match opencode palette filtering and grouping`
5. `fix(tui): add dynamic command availability and suggested rows`
6. `feat(tui): route palette commands through safe intents`
7. `fix(tui): align exclusions and harness-only commands`
8. `test(tui): add state matrix and dogfood coverage`
9. `docs(tui): record command palette parity evidence`

## Success Criteria

The implementation is complete only when:

- [ ] Harness `Ctrl+P` includes every non-excluded visible/reachable Opencode palette command ID from the matrix.
- [ ] Harness `Ctrl+P` excludes every explicit exclusion and hidden-only non-target.
- [ ] Filtering, grouping, navigation, empty state, suggested duplication, and keybinding footers match Opencode semantics.
- [ ] Dynamic labels and availability match required representative states.
- [ ] Palette dispatch uses safe Harness architecture paths only.
- [ ] Tests drive real palette input/render/dispatch behavior.
- [ ] Dogfood gates pass after large changes and final implementation.
- [ ] PR notes include source refs, test evidence, and any approved Harness deviations.
