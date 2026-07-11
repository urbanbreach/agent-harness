# OpenCode TUI Parity Gaps — Implementation Plan

**Generated:** 2026-07-05
**Method:** Hyperplan adversarial multi-agent analysis (5 category members, 3 rounds: independent analysis → cross-attack → convergence)
**Status:** Planning-complete, implementation-ready

---

## Methodology

Five adversarial analysts (structural, visual, interaction, feature-surface, state-model) independently compared Harness TUI (`crates/harness-tui/`) against Opencode TUI (`inspirations/opencode/packages/tui/`) to find UI/UX parity gaps. Each produced 3-5 candidate gaps with file:line evidence. In Round 2, each member ruthlessly attacked the other four's findings, verdicting every gap as STRONG, WEAK, DUPLICATE, or FALSE. 24 candidate gaps were narrowed to 5 that survived with STRONG consensus from 4/4 attacking members.

**Eliminated findings (FALSE):**
- Startup branding (Harness is a different product — branding is intentional)
- Always-allow permission scope (factually wrong — `PermissionGrantScope` has `Run`/`Session`/`Workspace`, UI offers `AllowAlways`)
- Sub-agent hierarchy (Harness has `LineageBrowserState` with depth/parent_index/child_indices)
- Message type taxonomy (Harness uses distinct event variants, TUI builds typed turns from events)
- Timeline pagination (cited model is in web UI, not TUI)

---

## The Five Core Parity Gaps

### Gap 1: Workspace Management System (4 dialogs completely missing)

**Severity:** High — entire feature category absent

**Opencode source:**
- `inspirations/opencode/packages/tui/src/component/dialog-workspace-create.tsx` (308 lines) — workspace selection/warp dialog for switching sessions between workspaces, handles VCS file-change confirmation
- `inspirations/opencode/packages/tui/src/component/dialog-workspace-list.tsx` (112 lines) — lists all connected workspaces with status indicators, allows deletion
- `inspirations/opencode/packages/tui/src/component/dialog-workspace-file-changes.tsx` (144 lines) — shows uncommitted VCS file changes during workspace warp, asks "move these changes with the session?"
- `inspirations/opencode/packages/tui/src/component/dialog-workspace-unavailable.tsx` (69 lines) — error recovery when session's workspace becomes unavailable
- Wired in `app.tsx:606-614` (`workspace.list` → `DialogWorkspaceList`) and `prompt/index.tsx:533-539` (`workspace.set` → warp)

**Harness current state:**
- `crates/harness-tui/src/app/workspace_display.rs` (89 lines) — only generates directory/branch label strings
- `crates/harness-tui/src/keybindings/parity_matrix.rs:170-190` — confirms `workspace.list`, `workspace.set`, `workspace.copy_path`, `session.move` all `harness_equivalent: "missing"`
- `crates/harness-tui/src/app/palette_controller.rs:150-155` — returns `false` for all workspace commands

**User impact:** Users cannot create, list, switch between, or delete workspaces from the TUI. Cannot warp sessions to different workspace contexts.

#### Implementation Steps

1. **Define workspace data model** — Create `crates/harness-tui/src/app/workspace_manager.rs` with:
   - `WorkspaceEntry` struct (id, path, branch, status, is_active)
   - `WorkspaceManagerState` struct (entries, selected_index, filter, loading)
   - Methods: `list_workspaces()`, `create_workspace()`, `warp_session()`, `delete_workspace()`

2. **Add overlay kinds** — Modify `crates/harness-tui/src/overlay.rs`:
   - Add `WorkspaceList`, `WorkspaceCreate`, `WorkspaceFileChanges`, `WorkspaceUnavailable` variants to `OverlayKind`
   - Add corresponding boolean fields to `OverlayState`

3. **Create workspace list dialog** — Create `crates/harness-tui/src/ui_overlays/workspace_list_dialog.rs`:
   - Render searchable list of workspaces with status indicators
   - Support pin/delete actions via keyboard (Ctrl-D for delete, Enter to warp)
   - Mirror `dialog-workspace-list.tsx` layout (status letters, truncated paths)

4. **Create workspace create/warp dialog** — Create `crates/harness-tui/src/ui_overlays/workspace_create_dialog.rs`:
   - Render workspace selection list for warping
   - Show VCS file-change confirmation sub-dialog when uncommitted changes detected
   - Mirror `dialog-workspace-create.tsx` flow

5. **Create file-changes confirmation dialog** — Create `crates/harness-tui/src/ui_overlays/workspace_file_changes_dialog.rs`:
   - Show list of uncommitted VCS files
   - "Move these changes with the session?" prompt with Yes/No

6. **Create workspace unavailable dialog** — Create `crates/harness-tui/src/ui_overlays/workspace_unavailable_dialog.rs`:
   - Error recovery UI with restore option

7. **Register slash commands** — Modify `crates/harness-tui/src/keybindings/command_registry.rs`:
   - Add `/warp` (alias for `workspace.set`), `/workspaces` (alias for `workspace.list`)
   - Add `/move` (alias for `session.move`)

8. **Update palette controller** — Modify `crates/harness-tui/src/app/palette_controller.rs`:
   - Return `true` for `workspace.list`, `workspace.set`, `workspace.copy_path`, `session.move`

9. **Update parity matrix** — Modify `crates/harness-tui/src/keybindings/parity_matrix.rs`:
   - Change `harness_equivalent` from `"missing"` to the new implementation paths
   - Change `status` from `Excluded` to `Parity`

#### Verification Gates

- **Test:** Write `crates/harness-tui/src/app/tests/workspace_dialog_tests.rs`:
  - Assert `WorkspaceList` overlay opens via `/workspaces` command
  - Assert workspace list renders with correct status indicators
  - Assert warp flow opens file-changes confirmation when VCS changes detected
  - Assert delete action removes workspace from list
- **Test:** Update `palette_parity_tests.rs` to assert workspace commands appear in palette
- **Manual QA:** Run `cargo run -p harness -- tui`, type `/workspaces`, verify dialog opens with workspace list
- **Build:** `cargo build -p harness-tui` exits 0
- **Lint:** `cargo clippy -p harness-tui -- -D warnings` exits 0

---

### Gap 2: Plugin System (runtime slots/routes/status missing)

**Severity:** High — blocks entire class of TUI extension

**Opencode source:**
- `inspirations/opencode/packages/tui/src/plugin/runtime.tsx:12-34` — creates plugin runtime with `Slot`, `routes`, `commands`, `status`, `update`, `clear`, `setupSlots`
- `inspirations/opencode/packages/tui/src/plugin/slots.tsx:25-65` — creates Solid slot registry with `HostSlots.register(...)`
- `inspirations/opencode/packages/tui/src/feature-plugins/system/plugins.tsx:238-261` — registers visible palette commands `plugins.list` and `plugins.install`

**Harness current state:**
- `crates/harness-tui/src/overlay.rs:1-17` — closed `OverlayKind` enum with only first-party overlays
- `crates/harness-tui/src/ui_overlays/status_dialog.rs:226-228` — hard-renders `No Plugins`
- `crates/harness-tui/src/keybindings/parity_matrix.rs:1494-1502` — explicitly says `plugins.list`/`plugins.install` missing

**User impact:** Opencode can add TUI panels, commands, status rows, and routes at runtime. Harness users see fixed first-party overlays and static `No Plugins` status. Plugin-driven UI extension is structurally absent.

#### Implementation Steps

1. **Define plugin trait** — Create `crates/harness-tui/src/plugin/mod.rs`:
   - `trait TuiPlugin`: `fn id(&self) -> &str`, `fn name(&self) -> &str`, `fn slots(&self) -> Vec<PluginSlot>`, `fn commands(&self) -> Vec<PluginCommand>`, `fn status_rows(&self) -> Vec<StatusRow>`
   - `struct PluginSlot { id: String, kind: SlotKind, render: fn(&PluginContext) -> PluginRender }`
   - `struct PluginManager { plugins: Vec<Box<dyn TuiPlugin>>, slots: HashMap<String, PluginSlot> }`

2. **Create plugin registry** — Create `crates/harness-tui/src/plugin/registry.rs`:
   - `PluginRegistry::register(plugin)` — adds plugin, registers slots/commands/status
   - `PluginRegistry::unregister(id)` — removes plugin and its contributions
   - `PluginRegistry::list()` — returns plugin metadata for UI display

3. **Add plugin overlay kinds** — Modify `crates/harness-tui/src/overlay.rs`:
   - Add `PluginList` variant to `OverlayKind`
   - Add `plugin_list_open: bool` to `OverlayState`

4. **Create plugin list dialog** — Create `crates/harness-tui/src/ui_overlays/plugin_list_dialog.rs`:
   - Render list of registered plugins with name, description, status
   - Support install/uninstall actions (if applicable)
   - Replace hard-coded `No Plugins` in status dialog with dynamic plugin list

5. **Wire plugin slots into rendering** — Modify `crates/harness-tui/src/ui_overlays.rs`:
   - Add plugin slot rendering pass after first-party overlays
   - Route plugin-contributed commands through palette controller

6. **Register slash commands** — Modify `crates/harness-tui/src/keybindings/command_registry.rs`:
   - Add `/plugins` (alias for `plugins.list`)
   - Add `/install-plugin` (alias for `plugins.install`)

7. **Update palette controller** — Modify `crates/harness-tui/src/app/palette_controller.rs`:
   - Return `true` for `plugins.list`, `plugins.install`
   - Dynamically include plugin-contributed commands

8. **Update status dialog** — Modify `crates/harness-tui/src/ui_overlays/status_dialog.rs`:
   - Replace `No Plugins` hard-code with dynamic plugin list from registry
   - Show plugin-contributed status rows

9. **Update parity matrix** — Modify `crates/harness-tui/src/keybindings/parity_matrix.rs`:
   - Change `plugins.list`/`plugins.install` from `missing` to implemented
   - Change `status` from `Excluded` to `Parity`

#### Verification Gates

- **Test:** Write `crates/harness-tui/src/app/tests/plugin_system_tests.rs`:
  - Assert `PluginRegistry::register()` adds plugin to list
  - Assert plugin list dialog renders registered plugins
  - Assert `/plugins` command opens plugin list overlay
  - Assert status dialog shows registered plugins instead of `No Plugins`
- **Test:** Update `palette_parity_tests.rs` to assert plugin commands appear in palette
- **Manual QA:** Run TUI, type `/plugins`, verify dialog opens with plugin list
- **Build:** `cargo build -p harness-tui` exits 0
- **Lint:** `cargo clippy -p harness-tui -- -D warnings` exits 0

---

### Gap 3: Theme Catalog (2 vs 34+ themes, no custom/system themes)

**Severity:** High — immediately visible visual divergence

**Opencode source:**
- `inspirations/opencode/packages/tui/src/theme/index.ts:36-90` — token-rich Theme model with primary/secondary/accent/status/text/background/diff/markdown/syntax tokens
- `inspirations/opencode/packages/tui/src/theme/index.ts:130-164` — ~34 defaults in `DEFAULT_THEMES` (Catppuccin variants, Gruvbox, Tokyonight, GitHub, etc.)
- `inspirations/opencode/packages/tui/src/context/theme.tsx:37-60` — discovers custom `.opencode/themes/*.json` and global config themes
- `inspirations/opencode/packages/tui/src/theme/index.ts:360-460` — generates `system` theme from terminal palette
- `inspirations/opencode/packages/tui/src/component/dialog-theme-list.tsx:6-49` — searchable `DialogThemeList` with all themes

**Harness current state:**
- `crates/harness-tui/src/theme.rs:493-502` — fixed Rust `Theme` palette
- `crates/harness-tui/src/theme.rs:801-918` — only `harness_dark()` and `harness_high_contrast()` constructors
- `crates/harness-tui/src/theme.rs:921-923` — `available_theme_names()` returns `["default", "high-contrast"]`
- `crates/harness-tui/src/ui_overlays/theme_dialog.rs:9-24,39-62` — hard-coded 44x8 list with labels `Harness Dark`/`High Contrast`, no search

**User impact:** Opencode users can select Catppuccin/Gruvbox/Tokyonight/etc., custom themes, system-adaptive colors. Harness has only 2 themes. Colors and syntax/diff palettes diverge immediately.

#### Implementation Steps

1. **Expand theme token model** — Modify `crates/harness-tui/src/theme.rs`:
   - Add missing token categories to `Theme` struct: `accent`, `markdown`, `syntax` (if not already present)
   - Ensure all Opencode token categories have Harness equivalents

2. **Port default themes** — Add to `crates/harness-tui/src/theme.rs`:
   - Create constructor functions for each Opencode default theme: `catppuccin_mocha()`, `catppuccin_latte()`, `catppuccin_frappe()`, `catppuccin_macchiato()`, `gruvbox_dark()`, `gruvbox_light()`, `tokyonight_storm()`, `tokyonight_night()`, `tokyonight_day()`, `github_dark()`, `github_light()`, `github_dark_dimmed()`, `github_dark_high_contrast()`, `github_light_high_contrast()`, and remaining themes from `DEFAULT_THEMES`
   - Update `available_theme_names()` to return all theme names
   - Update `load_theme_by_name()` to resolve any theme by name

3. **Add custom theme discovery** — Create `crates/harness-tui/src/theme/discovery.rs`:
   - Scan `.harness/themes/*.json` for custom theme files (mirror Opencode's `.opencode/themes/*.json`)
   - Parse JSON theme definitions into `Theme` structs
   - Merge with built-in defaults

4. **Add system theme generation** — Create `crates/harness-tui/src/theme/system.rs`:
   - Query terminal palette (via ANSI escape sequences or terminal queries)
   - Generate `system` theme from detected colors
   - Mirror Opencode's `systemThemeFromTerminal()` logic

5. **Upgrade theme dialog** — Modify `crates/harness-tui/src/ui_overlays/theme_dialog.rs`:
   - Replace hard-coded 44x8 list with dynamic list from `available_theme_names()` + custom themes
   - Add search/filter input (mirror Opencode's `DialogSelect` search)
   - Add backdrop dim (call `render_overlay_dim_backdrop` before rendering)
   - Expand dialog to show all available themes with scrolling

6. **Wire theme switching** — Modify `crates/harness-tui/src/app/`:
   - Ensure theme selection persists to config
   - Ensure theme switch applies immediately without restart

7. **Update parity matrix** — Modify `crates/harness-tui/src/keybindings/parity_matrix.rs`:
   - Update theme-related entries to reflect expanded catalog

#### Verification Gates

- **Test:** Write `crates/harness-tui/src/theme/tests.rs` (or extend existing):
  - Assert `available_theme_names()` returns 30+ theme names
  - Assert each theme name resolves to a valid `Theme` struct
  - Assert custom theme files in `.harness/themes/` are discovered and loaded
  - Assert `system` theme generates from terminal palette
- **Test:** Update theme dialog tests:
  - Assert theme dialog renders all available themes
  - Assert search filters themes by name
  - Assert backdrop dim is rendered
- **Manual QA:** Run TUI, open theme dialog (`/themes`), verify 30+ themes appear, search works, switching to `catppuccin_mocha` changes colors immediately
- **Build:** `cargo build -p harness-tui` exits 0
- **Lint:** `cargo clippy -p harness-tui -- -D warnings` exits 0

---

### Gap 4: Rich Paste Handling (no image/PDF/file-path/large-paste)

**Severity:** Medium — daily composer workflow gap

**Opencode source:**
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:366-384` — supports image clipboard content
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:1391-1415` — normalizes bracketed paste, suppresses terminal default insertion
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:1178-1199` — converts local text/binary attachments from pasted paths
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:1201-1207` — summarizes large pastes as `[Pasted ~N lines]` when enabled
- `inspirations/opencode/packages/tui/src/component/prompt/index.tsx:1219-1264` — inserts image/PDF virtual parts

**Harness current state:**
- `crates/harness-tui/src/app/prompt_input.rs:214-229` — only normalizes CRLF/CR and inserts every pasted char into the prompt
- No attachment detection, no local file-path paste conversion, no large-paste summary interaction

**User impact:** Pasting a screenshot/PDF/file path or a large multi-line snippet in Opencode creates compact structured prompt parts. In Harness it becomes plain text or an unbounded raw paste.

#### Implementation Steps

1. **Add paste classification** — Modify `crates/harness-tui/src/app/prompt_input.rs`:
   - Add `PasteKind` enum: `Text`, `FilePath`, `LargeText`, `ImageData`, `PdfData`
   - Add `classify_paste(content: &str) -> PasteKind` function:
     - If content looks like a file path (starts with `/` or `./` or `~` and path exists) → `FilePath`
     - If content has > 150 chars or >= 3 lines → `LargeText`
     - Otherwise → `Text`

2. **Add file-path paste conversion** — Create `crates/harness-tui/src/app/paste_attachments.rs`:
   - When `FilePath` detected, check if file is image (png/jpg/gif/webp/svg) or PDF
   - For images: insert `@<filepath>` file mention entry (leverage existing `file_mentions` system)
   - For PDFs: insert `@<filepath>` file mention entry
   - For text files: read content and insert as `[Pasted file: <filename>]` + content
   - For non-existent paths: insert as plain text

3. **Add large-paste summary** — Modify `crates/harness-tui/src/app/prompt_input.rs`:
   - When `LargeText` detected and paste summary is enabled (check config toggle):
     - Count lines and chars
     - Insert `[Pasted ~N lines]` summary token instead of raw content
     - Store full content in a paste buffer for expansion if needed
   - When paste summary disabled: insert raw content as before

4. **Add paste summary toggle** — Modify `crates/harness-tui/src/app/toggles.rs`:
   - Add `PasteSummary` toggle (on/off, default on)
   - Wire to config: `tui.jsonc` → `paste_summary: bool`

5. **Add bracketed paste normalization** — Modify `crates/harness-tui/src/app/prompt_input.rs`:
   - Detect bracketed paste escape sequences (`\x1b[200~` ... `\x1b[201~`)
   - Strip escape sequences, normalize content
   - Suppress terminal default insertion during bracketed paste

6. **Update composer tests** — Modify `crates/harness-tui/src/app/tests/composer_editing_tests.rs`:
   - Add test cases for each `PasteKind`
   - Add test for large-paste summary
   - Add test for file-path detection and attachment conversion

#### Verification Gates

- **Test:** Write tests in `crates/harness-tui/src/app/tests/paste_handling_tests.rs`:
  - Assert `classify_paste()` correctly identifies file paths, large text, plain text
  - Assert file-path paste creates file mention entry (not raw text)
  - Assert large paste (>150 chars) creates `[Pasted ~N lines]` summary when toggle is on
  - Assert large paste inserts raw text when toggle is off
  - Assert bracketed paste sequences are stripped
- **Manual QA:** Run TUI, paste a 200-line block, verify `[Pasted ~200 lines]` summary appears. Paste a file path to an image, verify `@<path>` mention appears.
- **Build:** `cargo build -p harness-tui` exits 0
- **Lint:** `cargo clippy -p harness-tui -- -D warnings` exits 0

---

### Gap 5: Skill Selection Dialog (no skill picker)

**Severity:** Medium — discoverability gap for skill system

**Opencode source:**
- `inspirations/opencode/packages/tui/src/component/dialog-skill.tsx:13-70` — skill picker dialog. Fetches available skills from SDK (`sdk.client.app.skills()`), displays them in a searchable select with name, description, and search filter. Shows error state if skills cannot be loaded.
- Wired in `prompt/index.tsx:510-528` (`prompt.skills` command, slash name `/skills`). Selecting a skill inserts `/<skill> ` into the prompt buffer.

**Harness current state:**
- No implementation. Grep for `skill.*dialog`, `DialogSkill`, `skill.*picker` in `crates/harness-tui/src/` returns only `parity_matrix.rs` and `palette_parity_tests.rs` (which document the gap).
- `crates/harness-tui/src/keybindings/parity_matrix.rs:1420-1456` — confirms `prompt.skills` has `harness_equivalent: "missing"` and `status: ParityStatus::Excluded`
- `crates/harness-tui/src/overlay.rs:1-17` — no skill dialog overlay kind

**User impact:** Users cannot browse and select skills from the TUI. While Harness has a skill system (skills are loaded via `task` tool's `load_skills` parameter), there is no interactive skill picker dialog. Users must know skill names in advance.

#### Implementation Steps

1. **Add skill dialog overlay kind** — Modify `crates/harness-tui/src/overlay.rs`:
   - Add `SkillDialog` variant to `OverlayKind`
   - Add `skill_dialog_open: bool` to `OverlayState`

2. **Create skill dialog state** — Create `crates/harness-tui/src/app/skill_dialog.rs`:
   - `SkillDialogState` struct: `skills: Vec<SkillEntry>`, `filter: String`, `selected_index: usize`, `loading: bool`, `error: Option<String>`
   - `SkillEntry` struct: `name: String`, `description: String`, `path: String`
   - Methods: `load_skills()` (scan `.agent-harness/skills/` directory), `filter_skills()`, `select_skill()`

3. **Create skill dialog renderer** — Create `crates/harness-tui/src/ui_overlays/skill_dialog.rs`:
   - Render searchable list of skills with name + description
   - Add search/filter input at top
   - Show loading state while scanning
   - Show error state if skills cannot be loaded
   - Mirror `dialog-skill.tsx` layout (searchable `DialogSelect` titled `Skills`)

4. **Wire skill selection to composer** — Modify `crates/harness-tui/src/app/composer.rs`:
   - When skill selected from dialog, insert `/<skill_name> ` into prompt buffer
   - Close skill dialog overlay
   - Focus composer input

5. **Register slash command** — Modify `crates/harness-tui/src/keybindings/command_registry.rs`:
   - Add `/skills` command (maps to `prompt.skills`)
   - Command opens `SkillDialog` overlay

6. **Update palette controller** — Modify `crates/harness-tui/src/app/palette_controller.rs`:
   - Return `true` for `prompt.skills`

7. **Update parity matrix** — Modify `crates/harness-tui/src/keybindings/parity_matrix.rs`:
   - Change `prompt.skills` from `harness_equivalent: "missing"` to implementation path
   - Change `status` from `Excluded` to `Parity`

8. **Add backdrop dim** — Modify `crates/harness-tui/src/ui_overlays.rs`:
   - Call `render_overlay_dim_backdrop` before rendering skill dialog (parity with Opencode's full-screen modal)

#### Verification Gates

- **Test:** Write `crates/harness-tui/src/app/tests/skill_dialog_tests.rs`:
  - Assert `SkillDialog` overlay opens via `/skills` command
  - Assert skill list renders with name + description from `.agent-harness/skills/`
  - Assert search filters skills by name
  - Assert selecting a skill inserts `/<skill_name> ` into prompt buffer
  - Assert error state renders when skills directory is empty/missing
- **Test:** Update `palette_parity_tests.rs` to assert `/skills` appears in palette
- **Test:** Update `slash_menu_tests.rs` to assert `/skills` resolves
- **Manual QA:** Run TUI, type `/skills`, verify dialog opens with skill list from `.agent-harness/skills/`. Search for a skill, select it, verify `/<skill> ` appears in composer.
- **Build:** `cargo build -p harness-tui` exits 0
- **Lint:** `cargo clippy -p harness-tui -- -D warnings` exits 0

---

## Prioritization and Sequencing

### Implementation Waves

**Wave 1 (parallel, no dependencies):**
- **Gap 3: Theme Catalog** — Self-contained, no cross-crate dependencies. Highest user-visible impact. Start here.
- **Gap 4: Rich Paste Handling** — Self-contained to `prompt_input.rs` and `paste_attachments.rs`. No cross-gap dependencies.
- **Gap 5: Skill Selection Dialog** — Self-contained. Depends only on existing `.agent-harness/skills/` directory structure.

**Wave 2 (after Wave 1):**
- **Gap 2: Plugin System** — Architectural change to overlay system. Should be done after theme catalog (which also modifies overlays) to avoid merge conflicts. The plugin system's `PluginRegistry` will be used by future workspace management features.

**Wave 3 (after Wave 2):**
- **Gap 1: Workspace Management** — Most complex (4 dialogs). Depends on plugin system architecture for overlay extensibility. Should be done last to leverage the dialog stack improvements from Gap 2.

### Dependency Graph

```
Gap 3 (Themes) ──────────────────────────────→ done
Gap 4 (Paste) ───────────────────────────────→ done
Gap 5 (Skills) ──────────────────────────────→ done
Gap 2 (Plugins) ──── depends on overlay ─────→ done
                     architecture from Wave 1
Gap 1 (Workspace) ── depends on dialog ─────→ done
                      stack from Gap 2
```

### Atomic Commit Strategy

Each gap should be implemented as a series of atomic commits:

1. **Test commit** (RED): Write failing tests that prove the gap exists
2. **Implementation commit** (GREEN): Implement the feature to make tests pass
3. **Parity matrix commit**: Update `parity_matrix.rs` to reflect the new implementation
4. **Documentation commit**: Update relevant docs if needed

Commit message format: `tui(parity): <gap-name> - <what changed>`

Example:
```
tui(parity): theme-catalog - add 30+ default themes and searchable dialog
tui(parity): theme-catalog - update parity matrix to Parity status
tui(parity): paste-handling - add paste classification and large-paste summary
tui(parity): paste-handling - add file-path attachment conversion
tui(parity): skill-dialog - add skill picker overlay with search
tui(parity): skill-dialog - register /skills slash command
tui(parity): plugin-system - add TuiPlugin trait and PluginRegistry
tui(parity): plugin-system - replace No Plugins hardcode with dynamic list
tui(parity): workspace-mgmt - add workspace list/create/warp dialogs
tui(parity): workspace-mgmt - register /warp and /workspaces commands
```

---

## Verification Checklist

### Per-Gap Verification

- [ ] **Gap 1 (Workspace):** `/workspaces` command opens dialog with workspace list; warp flow shows file-change confirmation; `/warp` and `/move` commands resolve in palette
- [ ] **Gap 2 (Plugins):** `/plugins` command opens dialog with plugin list; status dialog shows registered plugins instead of `No Plugins`; plugin-contributed commands appear in palette
- [ ] **Gap 3 (Themes):** Theme dialog shows 30+ themes; search filters by name; switching to `catppuccin_mocha` changes colors immediately; custom themes from `.harness/themes/` are discovered
- [ ] **Gap 4 (Paste):** Pasting 200-line block shows `[Pasted ~200 lines]` summary; pasting image file path creates `@<path>` mention; bracketed paste sequences are stripped
- [ ] **Gap 5 (Skills):** `/skills` command opens dialog with skill list from `.agent-harness/skills/`; search filters by name; selecting skill inserts `/<skill> ` into composer

### Global Verification

- [ ] `cargo build -p harness-tui` exits 0
- [ ] `cargo clippy -p harness-tui -- -D warnings` exits 0
- [ ] `cargo nextest run -p harness-tui` exits 0
- [ ] `cargo nextest run -p harness-tui --test palette_parity_tests` exits 0
- [ ] `cargo nextest run -p harness-tui --test slash_menu_tests` exits 0
- [ ] All parity matrix entries for implemented gaps changed from `Excluded` to `Parity`
- [ ] No `as any`, `@ts-ignore`, `unwrap()`, or `panic!` in new code
- [ ] No unrelated files changed

### Manual QA

- [ ] Run `cargo run -p harness -- tui` and exercise each new feature
- [ ] Compare side-by-side with Opencode TUI at same terminal size
- [ ] Verify no regressions in existing TUI features

---

## Cross-Attack Evidence Summary

| Gap | Visual | Feature | Interaction | Structural | Consensus |
|-----|--------|---------|-------------|------------|-----------|
| Gap 1 (Workspace) | STRONG | STRONG | STRONG | STRONG | 4/4 STRONG |
| Gap 2 (Plugins) | STRONG | STRONG | STRONG | STRONG | 4/4 STRONG |
| Gap 3 (Themes) | STRONG | STRONG | STRONG | STRONG | 4/4 STRONG |
| Gap 4 (Paste) | STRONG | STRONG | STRONG | STRONG | 4/4 STRONG |
| Gap 5 (Skills) | STRONG | STRONG | STRONG | STRONG | 4/4 STRONG |

**Eliminated findings (FALSE — not gaps):**
- Startup branding — Harness is a different product, branding is intentional
- Always-allow permission scope — `PermissionGrantScope` has `Run`/`Session`/`Workspace`, UI offers `AllowAlways`
- Sub-agent hierarchy — Harness has `LineageBrowserState` with depth/parent_index/child_indices
- Message type taxonomy — Harness uses distinct event variants, TUI builds typed turns from events
- Timeline pagination — cited model is in web UI (`packages/app`), not TUI

**Eliminated findings (DUPLICATE — already covered):**
- Missing slash commands (`/variants`, `/editor`, `/skills`, `/warp`, `/move`) — aggregates Gaps 1, 2, 5
- Missing palette commands (workspace, variant, org, status, plugin) — aggregates Gaps 1, 2, 5
- Skills picker (structural finding) — same as Gap 5
