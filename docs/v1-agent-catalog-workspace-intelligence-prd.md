# V1 Agent Catalog + Workspace Intelligence Control Plane PRD

**Status:** completed V1 implementation slice; final evidence is recorded in Section 13.
**Audience:** autonomous overnight implementation agents starting fresh in this workspace.  
**Mandate:** implement this slice end-to-end without repeating Install-to-First-Trusted-Edit. Keep working until every required checkbox is satisfied with fresh evidence, or write a checkpoint that lets the next agent continue without rediscovery.

This PRD is strict. Belief is not acceptance. Only source-grounded implementation, tests, docs, and cited evidence count.

## 0. Read First Rules

0.1. Read these files before implementing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `crates/harness/AGENTS.md`
- `crates/harness-tui/AGENTS.md`
- `docs/AGENTS.md`
- `docs/roadmap-v1.md`
- `README.md`
- `docs/architecture.md`
- `docs/config.md`
- `docs/testing.md`
- `crates/harness-tools/src/lib.rs`
- `crates/harness-tools/src/native_tools.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/src/team_ops.rs`
- `crates/harness/src/doctor.rs`
- `crates/harness/src/sessions.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/session_lineage.rs`
- `crates/harness-tui/src/keybindings.rs`
- `crates/harness-tui/src/app.rs`
- Every repository in `inspirations/`

0.2. Use `inspirations/` only as reference material. Copy user-observable behavior only when it fits Harness's Rust-native, event-sourced runtime.

0.3 You are required to implement everything listed in this document.

0.4. Preserve runtime invariants:

- The coordinator remains the only authority for event append, scheduling, permission resolution, hooks, task lifecycle, and run lifecycle.
- Replay remains side-effect free and derives from append-only JSONL events.
- Tool execution goes through coordinator permission checks before execution.
- Redaction happens before persistence and before support artifacts expose data.
- TUI consumes projection-derived state and must not own runtime truth.
- Native tools own strict argument schemas, workspace path safety, and stable ids.

0.5. Use TDD. For every new public tool, catalog seam, doctor field, or TUI metadata surface, add or update failing tests first, then implement, then refactor.

0.6. Use atomic commits. Each commit must have one coherent behavior change plus tests and docs where required. Do not commit unless explicitly asked by the operator, but structure the work so commits are obvious.

## 1. Problem Statement

The first V1 slice proved that Harness can be installed, configured, launched, trusted for a first edit, replayed, and exported with evidence.

The next V1 risk is discoverability and control-plane drift. Harness already has agents, category routes, task delegation, background task retrieval, session CLI projections, team primitives, doctor readiness, native tools, TUI commands, permissions, and support export. These surfaces are useful, but their metadata is still split across config, doctor code, native tool registration, task output, TUI command lists, session CLI code, and roadmap prose.

That split makes V1 fragile because an operator or model cannot reliably answer:

- Which agents and category routes exist?
- Which profile, model, tools, permissions, prompt asset, fallback, and hidden/primary/subagent role apply?
- Which native tools exist, which permission bucket owns them, and which are supervisor-only?
- Which sessions can the model inspect without shelling out to the CLI?
- Which background child task can be cancelled, and what happened after cancellation?
- Which team runs exist without expanding Team Mode?
- Which AST-aware code search path is safe to use before broad replacements?
- Which doctor/status/support/TUI surface is authoritative?

V1 needs one bounded metadata and control spine so the agent and operator discover the same routes, tools, permissions, sessions, background work, team primitives, readiness, and evidence from one source of truth.

## 2. Solution

Implement **V1 Agent Catalog + Workspace Intelligence Control Plane**.

The slice delivers a bounded metadata spine and model-visible workspace intelligence tools:

- A thin `AgentCatalog`-style seam that resolves profiles, category routes, hidden profiles, prompt assets, model targets, fallbacks, toolsets, permission posture, skill readiness, display ordering, and doctor/support/TUI metadata.
- A native tool catalog seam that exposes stable tool ids, aliases, capabilities, permission kind, actor availability, replay behavior, schema status, and docs status.
- Model-visible session tools: `session_list`, `session_read`, `session_search`, and `session_info`, implemented over existing replay/session projections with capped, redacted, side-effect-free outputs.
- Dedicated `background_cancel` as a canonical wrapper over existing coordinator-owned background cancellation, while preserving `background_output(cancel=true)` compatibility.
- `team_list` as a mandatory narrow primitive over existing event-sourced team projection, with no Team Mode expansion.
- `ast_grep_search` as a required native tool after the native tool catalog and parity tests exist.
- `ast_grep_replace` as conditional/stretch only if edit safety, dry-run, path-safety, diff/artifact, permission, and parity gates pass.
- Doctor, support export, TUI status/help/command metadata, docs, and tests consuming the same catalog metadata where possible.

This is not full Team Mode, not a plugin host, not a slash-command system, not built-in skill catalog expansion, and not specialist-agent parity.

## 3. User Stories

1. As a local coding operator, I want one resolved agent catalog, so that Build, Plan, Discipline, subagents, hidden profiles, and category routes agree across doctor, task output, TUI, and support export.
2. As a parent agent, I want task results to expose the resolved child profile, category route, model, permissions, tools, fallback, and prompt asset status, so that delegation can be interpreted without guessing.
3. As a model, I want `session_list`, `session_read`, `session_search`, and `session_info`, so that I can inspect prior Harness sessions without shelling out to `harness sessions` or executing replay side effects.
4. As an operator, I want background cancellation to be a dedicated `background_cancel` tool, so that cancellation is explicit and discoverable while old `background_output(cancel=true)` calls still work.
5. As an operator, I want `team_list` to show current event-sourced team runs, so that primitive team state is discoverable without pulling in Team Mode worktrees, tmux, mailbox, or declared team registries.
6. As a coding agent, I want `ast_grep_search`, so that structural code search is available through a safe native tool with strict schema, path safety, and capped/artifacted output.
7. As a maintainer, I want `ast_grep_replace` only if it is as safe as the rest of the edit surface, so that structural replacement cannot bypass hashline/edit permissions or workspace safety.
8. As a support maintainer, I want doctor JSON and support export to include catalog/tool/session/team readiness, so that failures can be debugged from redacted evidence.
9. As a TUI operator, I want command/help/status/keybinding metadata to agree with runtime metadata, so that the UI is not a second source of truth.
10. As a release maintainer, I want docs and parity tests to fail on drift, so that V1 claims about tools, agents, permissions, sessions, and readiness stay honest.

## 4. Required Scope

### 4.1 Phase 1: Test Inventory And Red Baseline

- [x] Add or update a focused implementation checklist in the PR or checkpoint notes, not in generated artifacts.
- [x] Add failing tests for agent catalog resolution covering primary, subagent, hidden, and category routes.
- [x] Add failing tests for native tool catalog parity covering all currently registered native tool ids.
- [x] Add failing tests for model-visible session tools using deterministic fixture sessions.
- [x] Add failing tests for `background_cancel` equivalence with `background_output(cancel=true)`.
- [x] Add failing tests for narrow `team_list` projection behavior.
- [x] Add failing tests for `ast_grep_search` schema and workspace safety.
- [x] Add conditional failing tests for `ast_grep_replace` only if implementation is attempted.
- [x] Add docs drift tests or assertions for any public docs modified by this slice.

### 4.2 Phase 2: Agent Catalog Seam

- [x] Create a thin catalog seam in `harness-core` or the narrowest existing config/projection owner that can be reused by CLI, tools, doctor, support export, and TUI without moving coordinator authority.
- [x] The catalog resolves shipped primary profiles: `build`, `plan`, `discipline`.
- [x] The catalog resolves shipped subagents: `explore`, `general`.
- [x] The catalog resolves shipped category routes: `visual-engineering`, `artistry`, `ultrabrain`, `deep`, `quick`, `unspecified-low`, `unspecified-high`, `writing`.
- [x] The catalog resolves hidden profiles such as title, summary, and compaction when present.
- [x] Each entry includes stable id, display name, role, mode, hidden flag, category binding, display order, prompt asset status, prompt source, model ref, resolved provider/model/variant where available, fallback chain, toolset, permission posture, skill metadata, and readiness warnings.
- [x] Category fallback policy is represented once and reused by task output and doctor.
- [x] Unknown category fallback remains visible and follows existing documented policy.
- [x] Plan-to-Explore restriction remains runtime-enforced and tested.
- [x] Read-only subagent restrictions remain runtime-enforced and tested.
- [x] The catalog does not add specialist personas or aliases beyond existing shipped profiles.
- [x] No event schema change is added unless unavoidable; if unavoidable, update `docs/architecture.md` and event drift tests in the same phase.

### 4.3 Phase 3: Native Tool Catalog Seam

- [x] Add a native tool catalog seam owned by `harness-tools` or `harness-core::tool`, not by docs or TUI.
- [x] Each tool entry includes canonical id, provider function name if different, aliases if any, description summary, capability, permission kind, actor availability, supervisor-only status, schema status, mutation/read-only status, replay behavior, artifact behavior, and docs status.
- [x] Existing canonical ids remain stable.
- [x] Worker registry filtering remains through `ActorKind::Worker`.
- [x] MCP generic wrappers remain available and are clearly separated from built-in native tools.
- [x] Doctor can report the tool catalog without network calls or MCP server startup.
- [x] Tool parity tests fail if a registered required V1 tool is missing from the catalog or if docs list an unregistered tool.
- [x] Permission docs map each tool to canonical permission names: `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, and `lsp`.

### 4.4 Phase 4: Model-Visible Session Tools

- [x] Add `session_list` as a native model-visible tool.
- [x] Add `session_read` as a native model-visible tool.
- [x] Add `session_search` as a native model-visible tool.
- [x] Add `session_info` as a native model-visible tool.
- [x] Reuse `crates/harness/src/sessions.rs`, `harness_core::proj`, `harness_core::session_lineage`, and replay projections by extracting reusable core/library helpers where needed.
- [x] Do not shell out to the CLI from native tools.
- [x] Do not execute providers, tools, hooks, MCP servers, or network calls while reading sessions.
- [x] Outputs are capped, redacted, structured JSON plus concise display text.
- [x] Tools reject traversal and out-of-session-root access.
- [x] Tools expose enough metadata for run id, title, status, profile, provider/model, mode source, resumability, artifact count, child count, parent id, timestamps, and event counts where available.
- [x] `session_read` supports bounded reading by run id/path selector, message/event window, and redaction-on by default.
- [x] `session_search` supports text search across safe replay-derived message/tool summaries and returns capped excerpts with session/run ids.
- [x] `session_info` reports metadata, lineage, status, event counts, artifact index summary, and recovery/resume notes without dumping whole logs.
- [x] Session tool output explicitly states `source: "event_replay"` or equivalent.

### 4.5 Phase 5: Dedicated Background Cancel

- [x] Add `background_cancel` as a first-class native tool.
- [x] `background_cancel` uses the existing coordinator cancellation path used by `background_output(cancel=true)`.
- [x] Required selector is `request_id` unless compatibility requires `task_id` or `session_id`; ambiguous selectors fail with actionable errors.
- [x] Optional `reason` is redacted/capped before persistence.
- [x] Authorization behavior matches current background projection rules.
- [x] Terminal tasks report no cancellation performed.
- [x] Non-terminal authorized tasks produce the same lifecycle events as `background_output(cancel=true)`.
- [x] Late result handling remains `TaskResultLate` with side effects discarded.
- [x] `background_output(cancel=true)` remains supported and documented as compatibility.
- [x] Task result `next_actions` prefer `background_cancel(request_id=...)` for cancellation while still listing `background_output` for status/result retrieval.

### 4.6 Phase 6: Mandatory Narrow `team_list`

- [x] Add `team_list` as mandatory for this slice, but only as a primitive projection-reader over existing active event-sourced team state.
- [x] `team_list` must not add declared team registries.
- [x] `team_list` must not add worktrees.
- [x] `team_list` must not add tmux visualization.
- [x] `team_list` must not add mailbox artifacts.
- [x] `team_list` must not add team file claims.
- [x] `team_list` must not spawn, resume, shut down, or mutate teams.
- [x] Output includes team run id, name, description, status, lead summary, member counts, task counts, message counts, bounds consumption, created/last monotonic timestamps where projected, and deletion/shutdown state.
- [x] `team_status` remains the detailed per-team inspection tool.
- [x] Doctor reports active team count from projections if available, without treating full Team Mode as V1 scope.

### 4.7 Phase 7: AST-Grep Search And Conditional Replace

- [x] Add `ast_grep_search` as a required native tool after the native tool catalog and parity harness are in place.
- [x] `ast_grep_search` supports explicit language, pattern, paths, include/exclude globs, context, and output cap.
- [x] `ast_grep_search` is read-only and maps to the `codesearch` permission kind unless a new permission kind is explicitly justified, documented, and tested.
- [x] `ast_grep_search` rejects traversal and out-of-workspace paths.
- [x] `ast_grep_search` returns structured matches with file path, range, language, matched text or capped snippet, and artifact refs when output is large.
- [x] `ast_grep_search` failure modes are actionable for missing binary/adapter, parse failure, unsupported language, no matches, too many matches, and invalid pattern.
- [x] `ast_grep_replace` is conditional/stretch, not required for final DoD if safety gates cannot pass.
- [x] `ast_grep_replace` was not attempted in this slice, so no mutation surface was shipped without the required search, catalog, parity, permission, path-safety, and dry-run gates.
- [x] `ast_grep_replace` dry-run, explicit apply, edit-permission, workspace path-safety, diff/artifact, overlap, and mutation test gates remain deferred with the unshipped replace tool.
- [x] If any replace safety gate cannot be implemented cleanly, do not ship mutation. Document `ast_grep_replace` as deferred, keep roadmap unchecked for replace, and still complete required `ast_grep_search`.

### 4.8 Phase 8: Doctor, Support Export, TUI, And Docs Consumers

- [x] Doctor JSON consumes the agent catalog for resolved profile/category/hidden metadata.
- [x] Doctor JSON consumes the native tool catalog for tool ids, permission kinds, schema status, actor availability, and docs status.
- [x] Doctor reports model-visible session tool readiness.
- [x] Doctor reports `background_cancel` readiness.
- [x] Doctor reports primitive `team_list` readiness and active team count if inspectable.
- [x] Doctor reports AST-grep readiness without requiring network calls.
- [x] Support export includes agent catalog summary and native tool catalog summary.
- [x] Support export includes session tool availability and redaction status.
- [x] TUI command/help/keybinding/slash metadata is centralized or adapted to consume one metadata table rather than maintaining conflicting strings.
- [x] TUI remains compose-first and transcript-first.
- [x] TUI status/help surfaces do not become runtime authority.
- [x] README, `docs/config.md`, `docs/architecture.md`, `docs/testing.md`, and roadmap references are updated to exactly match shipped behavior.
- [x] Add a concise native tool catalog doc in `docs/`.
- [x] Add or update an agent/subagent guide in `docs/`.
- [x] Add or update a sessions/replay guide in `docs/`.
- [x] Add or update a permissions guide if tool-permission mapping changes or becomes newly documented.

## 5. Implementation Decisions And Owning Seams

| Decision | Owner Seam | Rule |
|---|---|---|
| Agent/profile/category resolution | `harness-core` config/catalog seam | One resolved metadata view, reusable by CLI/tools/TUI/support. |
| Coordinator authority | `harness-core::coord` | Do not move scheduling, permission, or event append authority. |
| Session inspection | `harness-core` projections plus extracted CLI-safe helpers | Native tools must not shell out to `harness sessions`. |
| Native tool metadata | `harness-tools` registry/tool seam | Tool ids, schema, capabilities, actor availability, permission mapping stay near tools. |
| Doctor readiness | `crates/harness/src/doctor.rs` consuming catalog metadata | Doctor remains local readiness only and makes no provider/MCP network calls. |
| Support export | `crates/harness/src/sessions.rs` and replay/export helpers | Export uses redacted, replay-derived metadata. |
| TUI metadata | `crates/harness-tui` app/view-model/keybinding seams | UI consumes metadata and projections, not runtime truth. |
| Background cancellation | Existing coordinator cancellation path | `background_cancel` is a wrapper, not a second cancellation engine. |
| Team list | Existing team projection | Read-only primitive only. No Team Mode expansion. |
| AST-grep search | `harness-tools` native tool adapter | Read-only structural search with capped artifacts. |
| AST-grep replace | Existing edit/permission safety path | Conditional only if dry-run and apply safety are proven. |

## 6. Tool Contracts

### 6.1 `session_list`

Required behavior:

- Input supports optional status/profile/resumable/filter/sort/limit fields.
- Output is structured JSON and concise text.
- Source is replay/session catalog projection.
- No side effects.
- Redacted by default.
- Capped result count with truncation metadata.

Required evidence:

- Unit tests for filters and sorting.
- Integration tests over fixture session directories.
- Test proving no provider/tool execution occurs.

### 6.2 `session_read`

Required behavior:

- Input selects a session by run id or safe path selector.
- Input supports bounded message/event windows.
- Output includes replay-derived messages/events summaries, not raw secrets.
- Large output spills to artifacts.
- Redaction is on by default and cannot be disabled by model tool calls unless an explicit operator-facing policy already exists.

Required evidence:

- Tests for read bounds, corrupted session handling, redaction, and traversal rejection.

### 6.3 `session_search`

Required behavior:

- Input supports query, optional session selector, case sensitivity, result limit, and context limit.
- Search is over replay-derived safe text: user messages, assistant summaries, tool summaries, titles, and metadata.
- Output includes session id, run dir, matched field, excerpt, and event/message reference when available.

Required evidence:

- Tests for multi-session search, no matches, caps, and redaction.

### 6.4 `session_info`

Required behavior:

- Input selects one session.
- Output includes run metadata, catalog entry, lineage, status, event counts, artifact summary, resumability, parent/child links, and recovery notes.
- No full event dump unless explicitly bounded.

Required evidence:

- Tests for normal, failed, replay-only, child, corrupt, and missing sessions.

### 6.5 `background_cancel`

Required behavior:

- Input supports `request_id` and optional `reason`.
- Optional compatibility selectors may include `task_id` or `session_id` only if ambiguity is handled safely.
- Output includes request id, session id, previous status, final status, terminal flag, cancel requested, cancel performed, cancel reason, route/runtime metadata where available, and `source: "event_replay"` or equivalent.
- Uses existing coordinator cancellation.

Required evidence:

- Tests matching `background_output(cancel=true)` behavior.
- Tests for unauthorized sibling request rejection.
- Tests for terminal no-op cancellation.
- Tests for late result handling.

### 6.6 `team_list`

Required behavior:

- Read-only list of active/deleted/shutdown event-sourced team runs from existing projection.
- No declared registry.
- No spawning.
- No worktrees.
- No tmux.
- No mailbox.
- No mutation.

Required evidence:

- Tests for empty list, active team, shutdown-requested team, deleted team, counts, and output caps.

### 6.7 `ast_grep_search`

Required behavior:

- Read-only structural search.
- Strict schema with `deny_unknown_fields`.
- Language is explicit or safely inferred from file extension.
- Paths are workspace-relative or validated absolute paths inside workspace.
- Output is capped and artifacted when large.
- Missing adapter produces actionable error.

Required evidence:

- Tests for schema, language validation, path safety, match output, caps, artifact spill, missing adapter, parse error, and no matches.

### 6.8 `ast_grep_replace`

Required behavior if implemented:

- Conditional/stretch.
- Dry-run by default.
- Apply requires explicit opt-in.
- Apply requires edit permission.
- Writes diff/artifact evidence.
- Does not bypass existing edit safety.
- Rejects traversal and overlapping unsafe edits.

Required evidence if implemented:

- Tests for dry-run default, explicit apply, permission denial, path safety, diff artifact, overlap rejection, and no partial writes.

## 7. Docs, Tests, And Evidence Gates

### 7.1 Required Test Commands

Run these unless the slice changes the command itself and documents why:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
cargo test -p harness-tools --test native_tool_parity_matrix_test
cargo test -p harness-tools --test native_control_plane_tools_test
cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test
cargo test -p harness --test replay_sessions_cli_test
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test event_docs_reference_test
cargo test -p harness-tui
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
```

### 7.2 Required Evidence

- [x] Agent catalog tests pass.
- [x] Tool catalog parity tests pass.
- [x] Session tool tests pass.
- [x] Background cancellation tests pass.
- [x] Team list tests pass.
- [x] AST-grep search tests pass.
- [x] AST-grep replace tests pass only if replace is implemented.
- [x] Doctor text and JSON evidence are captured.
- [x] Docs drift tests pass.
- [x] Redaction tests pass for session/support output touched by this slice.
- [x] TUI tests pass if command/help/keybinding/status surfaces change.
- [x] Lane artifact paths are recorded for final report.

### 7.3 Documentation Deliverables

- [x] Update `docs/roadmap-v1.md` checkboxes only when behavior is implemented and verified.
- [x] Update `docs/architecture.md` for new catalog/tool/session contracts and any event/schema changes.
- [x] Update `docs/config.md` for permission/tool/profile docs affected by this slice.
- [x] Update `docs/testing.md` for new required targeted tests or lane evidence.
- [x] Add `docs/native-tool-catalog.md` or equivalent concise V1 native tool catalog.
- [x] Add or update `docs/sessions-and-replay.md` or equivalent model-visible session tool guide.
- [x] Add or update `docs/agents-and-subagents.md` or equivalent agent catalog guide.
- [x] Add or update permission docs if tool-permission mapping changes or becomes newly documented.
- [x] README mentions only shipped behavior and does not claim full Team Mode or AST replace if deferred.

## 8. Atomic Commit Strategy

Use small commits in this order if commits are requested:

1. Catalog red tests and fixture inventory.
2. Agent catalog seam and tests.
3. Native tool catalog seam and parity tests.
4. Session projection helper extraction and session tool red tests.
5. `session_list` and `session_info`.
6. `session_read` and `session_search`.
7. `background_cancel` wrapper and compatibility updates.
8. `team_list` primitive projection tool.
9. `ast_grep_search` adapter, schema, docs, and tests.
10. Conditional `ast_grep_replace` dry-run/apply safety or explicit deferral docs.
11. Doctor/support export metadata consumers.
12. TUI command/help/keybinding metadata alignment if touched.
13. Docs, roadmap, and final evidence closeout.

Each commit should preserve green targeted tests for the touched seam. Do not combine AST-grep mutation with catalog plumbing. Do not combine TUI rendering changes with core/tool authority changes.

## 9. TDD Execution Rules

- Write the failing test before implementation for every new public contract.
- Prefer deterministic fixtures over live provider or host-specific state.
- Use fake adapters for AST-grep tests when the external binary is optional.
- Test permission denial before testing happy-path mutation.
- Test replay/no-side-effect behavior for all session tools.
- Test docs drift for any public tool or catalog table.
- Refactor only after green tests.
- Do not weaken existing tests to make the slice pass.
- Do not delete tests unless a replacement invariant owner is documented in `docs/testing.md`.

## 10. Out Of Scope

- Full Team Mode.
- Declared team registries.
- Team worktrees.
- Team tmux visualization.
- Team mailbox artifacts.
- Team file claims.
- Autonomous loops, Ralph loop, ultrawork loop, idle continuation, or todo enforcer.
- Specialist persona catalog expansion such as Oracle, Librarian, Metis, Momus, Atlas, Hephaestus, Sisyphus, or multimodal roles.
- Built-in skill catalog expansion.
- Skill progressive disclosure unless only documented as future work.
- Skill-embedded MCP or MCP OAuth.
- Slash-command or hook system expansion.
- Browser, media, desktop, web, mobile, IDE, or remote collaboration surfaces.
- Plugin host or upstream plugin compatibility.
- Broad provider fallback overhaul.
- Live-provider claims not backed by live-gated evidence.
- `ast_grep_replace` mutation if safety gates cannot pass.

## 11. Anti-Gaming Rules

- [x] Do not count doctor as provider execution proof.
- [x] Do not count CLI session commands as model-visible session tools.
- [x] Do not shell out from native tools to `harness sessions`.
- [x] Do not count a docs table as a catalog seam.
- [x] Do not let TUI command lists become the source of truth.
- [x] Do not count `background_output(cancel=true)` alone as the dedicated `background_cancel` tool.
- [x] Do not implement `team_list` by expanding Team Mode.
- [x] Do not implement AST-grep replace before search, catalog, parity, permission, path-safety, and dry-run gates.
- [x] Do not ship AST-grep mutation without explicit apply mode and edit permission.
- [x] Do not add broad compatibility aliases that bypass canonical tool ids or permission names.
- [x] Do not weaken tests, delete failing tests, narrow assertions, or mark roadmap boxes complete without evidence.
- [x] Do not publish release claims for speed, provider breadth, parity, Team Mode, or AST replace unless current evidence proves them.

## 12. Checkpoint Rule

If implementation cannot finish in one agent context, write or update a checkpoint before stopping.

The checkpoint must include:

- [x] Completed and remaining PRD checkboxes.
- [x] Exact files touched.
- [x] Exact tests and commands run.
- [x] Artifact/evidence paths.
- [x] Current failures with output summaries.
- [x] Deferred conditional items, especially `ast_grep_replace`.
- [x] Whether `team_list` stayed within primitive projection-reader scope.
- [x] Open design decisions and why they block.
- [x] Next smallest action for a fresh agent.

Do not write a final success report until all required gates pass.

## 13. Final Evidence Report

### Summary

- Slice: V1 Agent Catalog + Workspace Intelligence Control Plane
- Result: PASS
- Commit range: uncommitted workspace changes
- Evidence root: `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046`
- Conditional `ast_grep_replace`: DEFERRED
- `team_list` scope: PRIMITIVE PROJECTION-READER CONFIRMED

### Required Commands

| Command | Evidence path | Status |
|---|---|---|
| `git diff --check` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/git_diff_check.log` | PASS |
| `cargo fmt --all -- --check` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_fmt_check.log` | PASS |
| `python3 scripts/check-test-suite-gates.py` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/test_suite_gates.log` | PASS |
| `cargo check --workspace` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_check_workspace.log` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_clippy_workspace.log` | PASS |
| `cargo test --workspace --all-features` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log` | PASS |
| `cargo run -p harness -- --config configs/harness.example.jsonc config validate` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/config_validate.log` | PASS |
| `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log` | PASS |
| `cargo run -p harness -- --config configs/harness.example.jsonc doctor` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_text.log` | PASS |
| `cargo test -p harness-tools --test native_tool_parity_matrix_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_tool_parity.log` | PASS |
| `cargo test -p harness-tools catalog_includes_registered_tool_ids_with_permission_metadata` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/tool_catalog_unit.log` | PASS |
| `cargo test -p harness --test config_schema_cli_test doctor_cli_json_reports_native_tool_catalog_readiness` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_native_tool_catalog.log` | PASS |
| `cargo test -p harness-tools --test native_control_plane_tools_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_control_plane.log` | PASS |
| `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_agent_spawn_batch.log` | PASS |
| `cargo test -p harness-tools --test team_list_counts_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/team_list_counts.log` | PASS |
| `cargo test -p harness --test replay_sessions_cli_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| `cargo test -p harness --test config_docs_reference_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/config_docs_reference.log` | PASS |
| `cargo test -p harness --test event_docs_reference_test` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/event_docs_reference.log` | PASS |
| `cargo test -p harness-tui` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/harness_tui.log` | PASS |
| `scripts/test-lanes.sh fast` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/test_lanes_fast.log`; lane artifact root `target/test-lanes/20260528-134333` | PASS |
| `scripts/test-lanes.sh quality-gates` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/test_lanes_quality.log`; lane artifact root `target/test-lanes/20260528-134351` | PASS |

### Feature Evidence

| Feature | Tests/evidence | Status |
|---|---|---|
| Agent catalog seam | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/config_validate.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log` | PASS |
| Native tool catalog seam | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/tool_catalog_unit.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_tool_parity.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_native_tool_catalog.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log` | PASS |
| `session_list` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| `session_read` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| `session_search` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| `session_info` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/session_info_tool.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_workspace_intelligence_tools.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| `background_cancel` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_agent_spawn_batch.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_control_plane.log` | PASS |
| `team_list` primitive | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/team_list_counts.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log` | PASS |
| `ast_grep_search` | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/cargo_test_workspace.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/native_tool_parity.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log` | PASS |
| `ast_grep_replace` conditional | Deferred because the mutation safety gates in Section 4.7 were not attempted in this slice; no replace tool was shipped. | DEFERRED |
| Doctor/support metadata | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_text.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/doctor_json.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/replay_sessions_cli.log` | PASS |
| TUI metadata alignment | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/harness_tui.log` | PASS |
| Docs drift | `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/config_docs_reference.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/event_docs_reference.log`; `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046/test_lanes_quality.log` | PASS |

### Deferred Or Env-Gated Items

- `ast_grep_replace`: deferred; the required search/catalog/readiness slice shipped, but mutation was not attempted because this delivery did not implement and prove the dry-run/apply/edit-permission/diff-artifact/overlap safety gates.
- Live provider lanes: not used for release claims; `doctor` evidence is local readiness only and reports `provider_execution_proof: false`.
- Native visual lanes: not used for release claims.
- Full Team Mode: out of scope

### Release Claim Boundary

- Implemented: resolved agent/category catalog metadata; native tool catalog metadata; model-visible replay-safe session tools; dedicated `background_cancel`; primitive read-only `team_list`; read-only `ast_grep_search`; doctor/support/TUI/docs consumers for the shipped metadata spine.
- Verified: all required commands in Section 7.1 passed with logs under `target/v1-agent-catalog-workspace-intelligence-evidence/20260528-164046`; fast lane artifacts are under `target/test-lanes/20260528-134333`; quality-gates artifacts are under `target/test-lanes/20260528-134351`.
- Not claimed: full Team Mode, declared team registries, team worktrees, tmux/mailbox/file-claim surfaces, plugin hosting, browser/media surfaces, skill-MCP expansion, live provider execution proof, native visual signoff, or `ast_grep_replace` mutation.

## 14. Final Definition Of Done

This slice is done only when:

- [x] Every required checkbox in Sections 4 through 8 is complete.
- [x] `team_list` exists as a narrow primitive projection-reader or the slice is marked blocked.
- [x] `ast_grep_search` exists with strict schema, path safety, caps, artifacts, docs, and tests.
- [x] `ast_grep_replace` is either implemented with all safety gates and evidence or explicitly deferred as conditional/stretch.
- [x] Model-visible session tools exist and are replay-safe, redacted, capped, and tested.
- [x] `background_cancel` exists and reuses coordinator-owned cancellation.
- [x] Agent catalog metadata is reused by doctor/task/support/TUI consumers where touched.
- [x] Native tool catalog metadata is parity-tested and documented.
- [x] Docs and roadmap reflect exactly what shipped.
- [x] Required commands pass or failures are documented as blockers.
- [x] Final evidence report cites artifact paths, not only prose.
- [x] No out-of-scope orchestration, plugin, browser/media, skill-MCP, or Team Mode work was smuggled into the slice.
