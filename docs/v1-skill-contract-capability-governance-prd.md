# V1 Skill Contract + Capability Governance PRD

**Status:** Completed on 2026-05-28; evidence recorded in Section 14 and final Oracle review emitted `<promise>VERIFIED</promise>` after roadmap evidence wording fixes.  
**Audience:** autonomous implementation agents starting fresh in this workspace.  
**Mandate:** implement this slice end-to-end only after reading the required context below. Keep working until every required checkbox is satisfied with fresh evidence, or write a checkpoint that lets the next agent continue without rediscovery. Do not mark this PRD, any roadmap checkbox, or any implementation checklist item complete without the evidence required by Sections 13 and 14.

This PRD is strict. Belief is not acceptance. Only source-grounded implementation, tests, docs, manual QA, and cited evidence count.

## 0. Read First Rules

0.1. Read these files before implementing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `crates/harness/AGENTS.md`
- `docs/AGENTS.md`
- `docs/roadmap-v1.md`
- `docs/v1-agent-catalog-workspace-intelligence-prd.md`
- `README.md`
- `docs/config.md`
- `docs/architecture.md`
- `docs/testing.md`
- `crates/harness-tools/src/control_plane.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-core/src/config.rs`
- `crates/harness-core/src/agent_catalog.rs`
- `crates/harness/src/doctor.rs`
- Existing skill assets under `.agent-harness/skills/` and `.agents/skills/`
- Relevant skill references under `inspirations/`, especially Codex skill metadata/rendering and OMO built-in skill references

0.2. Use `inspirations/` only as reference material. Copy user-observable behavior only when it fits Harness's Rust-native, event-sourced runtime and coordinator-owned permission model.

0.3. Preserve runtime invariants:

- The coordinator remains the only authority for event append, task scheduling, permission resolution, hooks, and run lifecycle.
- Replay remains side-effect free and derives from append-only JSONL events.
- Tool execution goes through coordinator permission checks before execution.
- Skill metadata must never grant tools or bypass profile/coordinator permissions.
- Doctor and support export report readiness without starting providers, MCP servers, hooks, or remote fetches.
- Native tools own strict argument schemas, workspace path safety, stable ids, and capped/redacted outputs.

0.4. Use TDD. For every public skill-contract behavior, metadata seam, doctor/support field, or docs/schema contract, add or update failing tests first, then implement, then refactor.

0.5. Use atomic commits. Each commit should have one coherent behavior change plus tests and docs where required. Do not commit unless explicitly asked by the operator, but structure the work so commits are obvious.

0.6. Completion claims are evidence-gated:

- Do not mark a checkbox complete because code was written, tests were planned, or a command is expected to pass.
- A checkbox is complete only when the behavior is implemented, verified through the matching public surface, and cited in the final evidence table.
- If a verification command is not run, stale, env-gated, or fails for any reason, record it as `NOT RUN`, `STALE`, `ENV-GATED`, or `FAIL`; do not count it as acceptance.
- If any required behavior is deferred, blocked, or partially implemented, leave the PRD status incomplete and record the blocker. Do not call the slice done.
- Final status can change to completed only after Sections 13 and 14 are fully satisfied.

## 1. Problem Statement

The previous V1 slice delivered a working catalog and workspace-intelligence spine: agent/category metadata, native tool catalog, model-visible session tools, background cancellation, primitive team discovery, AST-grep search, doctor/support metadata, and TUI/docs consumers. Harness can now expose much of its runtime surface consistently.

The next V1 risk is skill and capability drift. Skills already work, but their public contract is still split across config, `control_plane.rs`, `agent_ops.rs`, README prose, doctor status, tests, roadmap checkboxes, and small checked-in starter assets. An operator or implementation agent cannot yet rely on one deterministic answer for:

- Which skill roots and scopes are active?
- Which skill wins when multiple scopes define the same skill?
- Which fields are valid V1 skill frontmatter?
- Which metadata is catalog-visible before loading the full `SKILL.md` body?
- Which skills are denied, disabled, malformed, shadowed, or loadable?
- Whether a skill can alter tool permissions or only describe/restrict expected tools?
- How skill content combines with agent prompts, category appends, `AGENTS.md`, and task delegation context?
- Which doctor/support/export fields prove skill readiness without leaking full prompt bodies?

This ambiguity blocks safe built-in skill expansion. Adding full `git-master`, `review-work`, or visual skill packs before the substrate is stable would freeze accidental parser behavior and permission assumptions into public V1 behavior.

## 2. Solution

Implement **V1 Skill Contract + Capability Governance**.

The slice formalizes skills as a stable Harness-native capability surface:

- A typed V1 skill metadata/frontmatter contract.
- Deterministic discovery and precedence across local roots.
- Compact catalog-time metadata separated from activation-time `SKILL.md` body loading.
- Progressive disclosure for skill metadata, skill body, and bundled references.
- Disabled, denied, malformed, missing, duplicate, and shadowed states with stable behavior.
- Permission invariants proving skill metadata cannot grant tools or bypass coordinator/profile restrictions.
- Task delegation proof for `load_skills`, category prompt interaction, lineage, sync/background behavior, and summary capping.
- Doctor/support/readiness evidence for local skill availability without full body leakage.
- Docs/config/schema drift coverage that keeps public skill claims honest.

This is a contract foundation, not a full built-in skill pack.

## 3. User Stories

1. As a local Harness operator, I want skill behavior to be deterministic, so that the same skill name loads the same content regardless of which surface invokes it.
2. As a local Harness operator, I want doctor output to show skill readiness, so that I can diagnose missing, denied, malformed, or disabled skills before a prompt fails.
3. As a local Harness operator, I want disabled skills to fail clearly, so that I can turn off built-in or local capability surfaces without guessing whether they still affect prompts.
4. As a local Harness operator, I want skill permission posture to be visible, so that I know whether a skill can be loaded automatically, requires approval, or is denied.
5. As a local Harness operator, I want skill metadata to be compact, so that support bundles and status surfaces do not leak full instruction bodies unnecessarily.
6. As a parent agent, I want `task(load_skills = [...])` to load skills in a documented order, so that child prompts are predictable.
7. As a parent agent, I want missing or denied skills to fail before child spawn, so that delegation does not proceed with partial or misleading context.
8. As a parent agent, I want child task results to report loaded skill metadata, so that I can interpret what context shaped the child answer without reading the whole child prompt.
9. As a child agent, I want loaded skill content to appear in a stable position relative to agent prompt, category prompt append, `AGENTS.md`, and task body, so that instruction priority is understandable.
10. As a skill author, I want a documented V1 frontmatter schema, so that I can write skills that pass validation and behave consistently.
11. As a skill author, I want invalid or unknown public fields to fail with actionable errors, so that mistakes do not silently change runtime behavior.
12. As a skill author, I want a quality template, so that skills include purpose, use-when, do-not-use-when, execution policy, steps, tool usage, stop conditions, and final checklist.
13. As a maintainer, I want skill discovery to report source scope and root, so that project/global/built-in precedence can be debugged from evidence.
14. As a maintainer, I want duplicate and shadowed skills represented explicitly, so that root ordering changes do not silently alter behavior.
15. As a maintainer, I want malformed skills represented without crashing discovery, so that one bad skill file does not hide the rest of the catalog.
16. As a maintainer, I want full `SKILL.md` bodies loaded only on activation, so that catalog and doctor surfaces remain cheap and safe.
17. As a maintainer, I want `allowed_tools`-style metadata to be restrictive or descriptive only, so that prompt assets cannot grant runtime tools.
18. As a security reviewer, I want tests proving skill metadata cannot bypass coordinator permissions, so that skills cannot become a permission escalation path.
19. As a support maintainer, I want support export to include compact skill readiness, so that skill-related failures can be debugged from redacted bundles.
20. As a release maintainer, I want docs and schema tests to fail on drift, so that V1 skill claims remain honest.
21. As a future built-in skill implementer, I want stable ids and disablement behavior before adding more built-ins, so that shipped skills can be governed after release.
22. As a future extension implementer, I want skill metadata and capability governance separated from hooks and plugins, so that extension work starts from a clear local contract.
23. As a model using session tools, I want skill-related task summaries capped, so that parent context stays lean across long delegations.
24. As a Plan or Explore restricted profile, I want skill loading to preserve profile restrictions, so that read-only lanes remain read-only even when a skill mentions edit or bash workflows.

## 4. Required Scope

### 4.1 Phase 1: Test Inventory And Red Baseline

- [x] Add or update a focused implementation checklist in PR/checkpoint notes, not in generated artifacts.
- [x] Add failing tests for V1 skill frontmatter parsing and validation.
- [x] Add failing tests for project/global/custom-root precedence after V1 terms were finalized: `project` covers project/workspace roots, `global` covers user/XDG roots, and separate built-in skill scope remains a follow-on before adding more built-in skills.
- [x] Add failing tests for duplicate, shadowed, missing, denied, disabled, and malformed skills.
- [x] Add failing tests proving skill metadata cannot grant tools beyond profile/coordinator permissions.
- [x] Add failing tests for `skill` activation loading full body only after explicit load.
- [x] Add failing tests for `task(load_skills = [...])` injection ordering and pre-spawn failure behavior.
- [x] Add failing tests for child lineage, sync/background behavior, continuation metadata, and summary capping when skills are loaded.
- [x] Add failing tests for doctor/support compact skill metadata and no full-body leakage.
- [x] Add docs/config/schema drift tests for any public contract introduced by this slice.

### 4.2 Phase 2: V1 Skill Metadata Contract

- [x] Define the V1 local skill metadata model in the narrowest shared owner that can serve tools, doctor, support export, and docs tests.
- [x] Required metadata includes stable skill id, name, description, source scope, root path, loadability, permission mode, and status.
- [x] Optional metadata includes argument hint, expected/restrictive tools, target agent/category, and deferred MCP/resource metadata.
- [x] The contract distinguishes catalog-visible metadata from activation-only `SKILL.md` body content.
- [x] Unknown or unsupported public behavior is rejected or clearly ignored according to tests and docs.
- [x] Deferred MCP and remote fields are metadata only; they do not execute, fetch, register tools, or start MCP servers.

### 4.3 Phase 3: Discovery, Precedence, And Status

- [x] Discovery produces deterministic ordering across configured roots.
- [x] Project/workspace roots are consistently reported as `project`, user/XDG roots as `global`, and built-in skill scope is explicitly deferred in docs/roadmap before additional built-ins ship.
- [x] Duplicate and shadowed skills are represented with actionable reasons.
- [x] Denied, disabled, malformed, and missing skills cannot load through either `skill` or `task(load_skills = [...])`.
- [x] Workspace path safety and symlink rejection behavior remain intact.
- [x] Discovery remains local and does not fetch remote skill URLs.

### 4.4 Phase 4: Capability Governance And Permission Invariants

- [x] Skill metadata cannot grant tools.
- [x] Expected or allowed tool metadata is treated as descriptive and/or restrictive only.
- [x] Tests prove a skill declaring edit/bash-like expectations does not grant those tools to read-only or restricted profiles.
- [x] Runtime profile toolsets and coordinator permission checks remain authoritative.
- [x] Local/starter skill disablement uses stable ids and has default behavior, tests, and doctor visibility; separate built-in skill-pack disablement remains a follow-on before more built-ins ship.
- [x] New capability governance does not add an event schema change unless implementation proves it unavoidable.

### 4.5 Phase 5: Progressive Disclosure And Prompt Injection

- [x] Catalog, doctor, and support surfaces expose compact metadata only.
- [x] `skill` loads full body only on explicit activation.
- [x] `task(load_skills = [...])` resolves and loads requested skills before child spawn.
- [x] Injection order among agent prompt, category append, skill content, `AGENTS.md` context, command context, and task body is documented and tested.
- [x] Multiple loaded skills preserve requested or documented order.
- [x] Duplicate `load_skills` requests have deterministic behavior.
- [x] Child summaries report skill metadata without leaking full bodies unnecessarily.

### 4.6 Phase 6: Doctor, Support Export, And Minimal Operator Surfaces

- [x] Doctor reports available, denied, disabled, malformed, and shadowed skill states without network calls.
- [x] Doctor separates skill catalog readiness from provider health and MCP registration.
- [x] Support export includes compact skill catalog/readiness metadata with redaction status.
- [x] Any TUI/status/toggles update consumes the same metadata and does not become runtime authority.
- [x] Operator-visible output makes disabled or unavailable skills understandable enough for manual QA.

### 4.7 Phase 7: Docs, Examples, And Roadmap Evidence

- [x] Update `docs/config.md` only after schema/examples/tests exist for any public config shape.
- [x] Add or update a skill authoring guide that covers frontmatter schema, content quality template, precedence, disablement, progressive disclosure, and permission invariants.
- [x] Update README task/skill prose to match runtime behavior.
- [x] Update `docs/roadmap-v1.md` checkboxes only with fresh test/manual evidence.
- [x] Keep parity and inspiration references marked as migration evidence, not release claims.

## 5. Implementation Decisions And Owning Seams

| Decision | Owner Seam | Rule |
|---|---|---|
| Skill metadata model | `harness-tools` / shared core helper only if needed | Keep close to skill tool behavior unless doctor/support need shared types. |
| Skill discovery | Existing control-plane skill discovery seam | Preserve local path safety and deterministic roots. |
| Task skill injection | `crates/harness-tools/src/agent_ops.rs` | Resolve all skills before child spawn; fail before spawning on missing/denied/disabled/malformed. |
| Permission authority | `harness-core` coordinator/profile policy | Skill metadata cannot grant tools or bypass permission checks. |
| Doctor readiness | `crates/harness/src/doctor.rs` consuming shared metadata | Doctor remains local readiness only. |
| Support export | Existing replay/export helpers | Export compact, redacted readiness metadata, not full skill bodies. |
| Docs/schema parity | `docs/config.md`, checked-in schemas/examples/tests | Do not document public keys before schema/examples/tests exist. |
| Built-in skills | Existing `.agent-harness/skills/` assets | Use minimal fixtures only; defer full skill pack polish. |

## 6. Testing Decisions

- Tests must exercise external behavior: tool outputs, task spawn behavior, doctor/support JSON, config validation, and docs/schema drift.
- Parser tests should cover valid frontmatter, missing required fields, malformed fields, deferred metadata, and unsupported fields.
- Discovery tests should cover root order, project/global/custom roots, git-root walking, symlink safety, duplicate names, shadowing, missing skills, malformed skills, denied skills, and disabled skills.
- Task tests should cover `load_skills` pre-spawn failure, injection ordering, multiple skills, duplicate skill requests, category interaction, lineage preservation, sync/background behavior, and summary capping.
- Permission tests should prove skill metadata cannot grant tools to Plan, Explore, category routes, or any restricted profile.
- Doctor/support tests should prove compact metadata is present and full body content is absent.
- Docs tests should fail when documented config/schema/tool/skill behavior drifts from checked-in examples or CLI validation output.

## 7. Verification Commands

Targeted gates:

```bash
cargo test -p harness-tools --test skill_load_discovery_test
cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test
cargo test -p harness-tools --test native_control_plane_tools_test
cargo test -p harness-tools --test native_tool_parity_matrix_test
cargo test -p harness --test config_docs_reference_test
cargo test -p harness --test config_schema_cli_test
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
```

Workspace and quality gates:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates --dry-run
```

## 8. Manual QA And Signoff

- Run `skill` against an allowed representative skill and confirm compact metadata is visible before full body content is loaded.
- Run `task(load_skills = [...], run_in_background = false)` with a representative skill and confirm the child prompt receives skill content before the task body.
- Run a background child task with `load_skills` and confirm result metadata, `background_output`, and continuation instructions do not leak full skill bodies unnecessarily.
- Run a denied or disabled skill through both `skill` and `task(load_skills = [...])` and confirm no child spawn occurs.
- Run a malformed skill discovery scenario and confirm other valid skills remain discoverable.
- Inspect `doctor --json` and a support export to confirm skill catalog readiness is compact, redacted, and source-scoped.
- Confirm any TUI/status/toggle surface shows skill availability or disablement without adding independent runtime truth.

## 9. Out Of Scope

- Full `git-master`, `review-work`, or `frontend-ui-ux` skill implementation.
- Full built-in skill pack polish.
- Remote skill URL fetching, signing, caching, registry, marketplace, or trust policy.
- Hooks, slash-command execution, browser tools, skill-embedded MCP lifecycle, or arbitrary plugin runtime.
- Team Mode expansion, worktrees, tmux visualization, mailbox artifacts, file claims, or declared team registry.
- `ast_grep_replace`.
- Broad Operator Cockpit hardening such as prompt history, diff hunk navigation, provider fallback UX, session search polish, or background/subagent keyboard navigation outside skill readiness needs.
- Event schema changes unless proven unavoidable during implementation.

## 10. Risks

- `skills.urls` may be misread as V1 remote skill support; keep it inert/deferred or explicitly rejected in docs/tests.
- `allowed_tools` may be misread as a permission grant; tests and docs must state it is restrictive/descriptive only.
- Precedence can become confusing if project, workspace/config, user/global, and built-in scopes are named inconsistently.
- Doctor/support may accidentally expose full skill bodies; progressive disclosure tests must guard against leakage.
- Heavy built-in skill work can swamp this slice; keep representative fixtures minimal.
- Adding public config prose before schema/examples/tests would violate docs conventions.

## 11. Suggested Atomic Commit Strategy

1. Add red tests and fixtures for V1 skill contract behavior.
2. Implement typed skill metadata/frontmatter validation.
3. Implement deterministic discovery, precedence, disabled/shadowed/malformed status.
4. Implement governance restrictions and permission invariant tests.
5. Implement progressive disclosure in skill/task outputs.
6. Wire doctor/support and any minimal TUI/status compact metadata consumers.
7. Update docs, config examples/schema references, and roadmap evidence.
8. Run final workspace verification and record evidence.

## 12. Follow-On Slices

- Built-in skill pack: `git-master`, `review-work`, and `frontend-ui-ux` after this contract stabilizes.
- Operator Cockpit hardening focused on skill/catalog visibility and diff/session navigation.
- Strict skill-scoped runtime enforcement if metadata-only governance proves insufficient.
- Hook seam and command contract after capability governance is stable.
- Skill resources/assets/scripts and skill-embedded MCP metadata activation.
- Remote skill distribution, signing, marketplace, and trust policy.
- `ast_grep_replace` as a separate edit-safety slice.

## 13. Final Definition Of Done

This slice is done only when every item below is true and cited in Section 14:

- [x] Every required checkbox in Section 4 is complete with fresh evidence.
- [x] Every user story in Section 3 is either satisfied by implemented behavior or explicitly mapped to an out-of-scope/follow-up item.
- [x] The V1 skill metadata/frontmatter contract is implemented, documented, and tested.
- [x] Skill discovery, precedence, duplicate/shadowed behavior, denied/disabled/malformed handling, and missing-skill errors are implemented, documented, and tested.
- [x] Progressive disclosure is implemented and tested: catalog/doctor/support surfaces expose compact metadata, and full `SKILL.md` bodies load only on activation.
- [x] `skill` and `task(load_skills = [...])` both enforce disabled, denied, missing, and malformed states before activation or child spawn.
- [x] Task delegation evidence proves injection ordering, category interaction, lineage, sync/background behavior, continuation metadata, and summary capping.
- [x] Permission tests prove skill metadata cannot grant tools or bypass coordinator/profile permissions.
- [x] Doctor and support export show compact skill readiness without full body leakage.
- [x] Docs, schemas, examples, README, and roadmap claims match the shipped behavior.
- [x] Every verification command in Section 7 has a fresh outcome recorded as `PASS`, `FAIL`, `NOT RUN`, or `ENV-GATED`; required commands must be `PASS` unless the slice is marked blocked.
- [x] Manual QA in Section 8 has been performed through CLI/tool surfaces, with the TUI compact-catalog surface covered by deterministic runtime-toggle evidence unless a manual TUI transcript is cited in Section 14.
- [x] No out-of-scope work from Section 9 was smuggled into the slice.
- [x] No event schema change happened; `docs/architecture.md` and event drift tests did not require updates.
- [x] The final evidence report cites artifact paths, command logs, or transcripts rather than only prose.

Non-acceptance examples:

- `cargo check` passes but skill loading was not manually exercised.
- Parser tests pass but `task(load_skills = [...])` was not verified through child spawn behavior.
- Doctor output changed but support export and no-body-leak behavior were not checked.
- Roadmap checkboxes were edited without command logs or manual QA artifacts.
- A live or env-gated lane was skipped and then described as passing.
- A follow-up issue exists for a required checkbox but the PRD status is marked complete.

## 14. Final Evidence Report

Checkpoint reference: uncommitted working-tree checkpoint on 2026-05-28; evidence root `/tmp/opencode/v1-skill-contract-qa/`.

### Verification command evidence

| Command | Outcome | Evidence |
|---|---:|---|
| `cargo test -p harness-tools --test skill_load_discovery_test` | PASS, 15 tests | `/tmp/opencode/v1-skill-contract-qa/skill_load_discovery_test.log` |
| `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` | PASS, 37 tests | `/tmp/opencode/v1-skill-contract-qa/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test.log` |
| `cargo test -p harness-tools --test native_control_plane_tools_test` | PASS, 2 tests | `/tmp/opencode/v1-skill-contract-qa/native_control_plane_tools_test.log` |
| `cargo test -p harness-tools --test native_tool_parity_matrix_test` | PASS, 1 test | `/tmp/opencode/v1-skill-contract-qa/native_tool_parity_matrix_test.log` |
| `cargo test -p harness --test config_docs_reference_test` | PASS, 5 tests | `/tmp/opencode/v1-skill-contract-qa/config_docs_reference_test.log` |
| `cargo test -p harness --test config_schema_cli_test` | PASS, 40 tests | `/tmp/opencode/v1-skill-contract-qa/config_schema_cli_test.log` |
| `cargo run -p harness -- --config configs/harness.example.jsonc config validate` | PASS | `/tmp/opencode/v1-skill-contract-qa/config_validate_cli.log` |
| `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json` | PASS | `/tmp/opencode/v1-skill-contract-qa/doctor_cli.json` |
| `cargo run -p harness -- schema` | PASS | `/tmp/opencode/v1-skill-contract-qa/schema_cli.log` |
| `cargo test -p harness --test replay_sessions_cli_test sessions_export_cli_support_includes_readiness_and_config_summaries` | PASS, 1 focused test | `/tmp/opencode/v1-skill-contract-qa/replay_sessions_export_test.log` |
| `cargo test -p harness runtime_toggles_report_compact_skill_catalog_states` | PASS | `/tmp/opencode/v1-skill-contract-qa/runtime_toggles_skill_catalog_test.log` |
| `cargo test -p harness prompt --lib` | PASS, 27 filtered lib tests | `/tmp/opencode/v1-skill-contract-qa/dynamic_prompt_lib_test.log` |
| `cargo fmt --all -- --check` | PASS | `/tmp/opencode/v1-skill-contract-qa/cargo_fmt_check.log` |
| `cargo check --workspace` | PASS | `/tmp/opencode/v1-skill-contract-qa/cargo_check_workspace.log` |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS | `/tmp/opencode/v1-skill-contract-qa/cargo_clippy_workspace_all_targets_all_features.log` |
| `cargo test --workspace --all-features` | PASS | `/tmp/opencode/v1-skill-contract-qa/cargo_test_workspace_all_features.log` |
| `scripts/test-lanes.sh fast` | PASS, fmt/check/nextest_ci | `/tmp/opencode/v1-skill-contract-qa/test_lanes_fast.log` |
| `scripts/test-lanes.sh quality-gates --dry-run` | PASS as dry-run | `/tmp/opencode/v1-skill-contract-qa/test_lanes_quality_gates_dry_run.log` |

### Manual QA evidence

| Surface | Outcome | Evidence |
|---|---|---|
| CLI discovery/help | `harness --help` and `harness run --help` render available surfaces used for QA. | `/tmp/opencode/v1-skill-contract-qa/harness_help.log`, `/tmp/opencode/v1-skill-contract-qa/harness_run_help.log` |
| Schema CLI | Runtime schema includes `SkillsConfig.disabled` and public skill config shape. | `/tmp/opencode/v1-skill-contract-qa/schema_cli.log` |
| Config validate CLI | Shipped config validates successfully. | `/tmp/opencode/v1-skill-contract-qa/config_validate_cli.log` |
| Doctor CLI | `doctor --json` reports skill catalog source `harness_tools::skill_catalog`, project source scope, `body_loaded: false`, counts, and `no_network_probes: true`. | `/tmp/opencode/v1-skill-contract-qa/doctor_cli.json` |
| Support export CLI | Deterministic `golden_path` run exported with compact `skill_catalog_summary`; manual disabled skill appears as `skill:project:manual-disabled`, `source_scope: project`, `status: disabled`, `body_loaded: false`, `disabled_count: 1`, and the body sentinel is absent. | `/tmp/opencode/v1-skill-contract-qa/manual-stable-id/run_golden_path.log`, `/tmp/opencode/v1-skill-contract-qa/manual-stable-id/support-bundle.json`, `/tmp/opencode/v1-skill-contract-qa/manual-stable-id/sessions_export.log` |
| Native `skill` activation | Allowed skill activation returns body only through explicit activation; stable-id disabled skills remain catalog-visible and activation-denied. | `v1_skill_activation_reports_metadata_then_loads_body`, `v1_skill_catalog_reports_shadowed_disabled_denied_and_malformed_states` in `/tmp/opencode/v1-skill-contract-qa/skill_load_discovery_test.log` |
| Native `task(load_skills)` foreground/background and failure surfaces | Foreground loaded-skill injection, background output route metadata with `load_skills`, missing/stable-id-disabled/denied/malformed pre-spawn failure, order/deduplication, and no body leakage are verified. | `/tmp/opencode/v1-skill-contract-qa/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test.log` |
| TUI toggles/status surface | Runtime toggles consume compact shared catalog states without becoming authority; this is deterministic runtime-toggle coverage, not a manual TUI transcript. | `/tmp/opencode/v1-skill-contract-qa/runtime_toggles_skill_catalog_test.log` |

### Feature evidence by phase

| Phase | Status | Evidence |
|---|---|---|
| 4.1 Test inventory/red baseline | Complete | New V1 discovery/activation tests, task governance tests, doctor/support docs-schema tests; logs listed above. |
| 4.2 Metadata contract | Complete | `crates/harness-tools/src/skill_catalog.rs`; `docs/config.md`; `docs/starter-skills.md`; `skill_load_discovery_test.log`. |
| 4.3 Discovery/precedence/status | Complete | Project/global/custom root, shadowed, denied, stable-id-disabled, malformed, missing, symlink-safety coverage in `skill_load_discovery_test.log` and task pre-spawn failure coverage. |
| 4.4 Capability governance | Complete | `allowed_tools` remains metadata-only; restricted child/profile behavior remains coordinator/profile-owned in `native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test.log`. |
| 4.5 Progressive disclosure/prompt injection | Complete | `skill` activation loads body only explicitly; task output reports compact metadata and injected prompts include requested skill content in tested order. |
| 4.6 Doctor/support/TUI surfaces | Complete | `doctor_cli.json`, stable-id disabled doctor/support tests in `config_schema_cli_test.log` and `replay_sessions_export_test.log`, `/tmp/opencode/v1-skill-contract-qa/manual-stable-id/support-bundle.json`, and deterministic TUI metadata coverage in `runtime_toggles_skill_catalog_test.log`. |
| 4.7 Docs/examples/roadmap | Complete | `README.md`, `docs/config.md`, `docs/starter-skills.md`, `docs/roadmap-v1.md`, `configs/config.json`, and drift tests. |

### User-story evidence

| Story | Outcome |
|---:|---|
| 1 | Deterministic catalog and activation path implemented through `discover_skill_catalog` and shared `skill`/`task` resolution tests. |
| 2 | Doctor reports concrete compact skill readiness in `doctor_cli.json`. |
| 3 | Stable-id disabled skills fail in `skill` and `task(load_skills)` tests; local/starter stable ids are documented. |
| 4 | Permission posture and status are part of `SkillCatalogEntry` and doctor/support metadata. |
| 5 | Catalog, doctor, task result metadata, and support export keep `body_loaded: false` and omit body sentinels. |
| 6 | `task(load_skills)` preserves request order and deduplicates deterministically. |
| 7 | Missing, denied, disabled, malformed, and symlink-unsafe skills fail before child spawn. |
| 8 | Child task outputs include compact loaded-skill metadata. |
| 9 | Prompt injection order is documented in `docs/config.md` and tested in task provider prompt assertions. |
| 10 | V1 frontmatter schema is documented in `docs/config.md` and `docs/starter-skills.md`, guarded by tests. |
| 11 | Unsupported public fields make entries malformed instead of silently changing behavior. |
| 12 | Skill authoring template is documented in `docs/starter-skills.md`. |
| 13 | Source scope/root are reported for implemented V1 scopes: `project` and `global`; separate built-in skill scope remains follow-up before adding more built-ins. |
| 14 | Duplicate and shadowed entries are represented explicitly with reasons. |
| 15 | Malformed skills do not hide valid siblings. |
| 16 | Full bodies load only via explicit `skill` activation or resolved task prompt injection. |
| 17 | `allowed_tools` metadata is descriptive/restrictive only and cannot grant tools. |
| 18 | Coordinator/profile permission authority is preserved by task/profile boundary tests. |
| 19 | Support export includes compact skill readiness in `skill_catalog_summary`. |
| 20 | Config/docs/schema drift tests guard public claims. |
| 21 | Stable ids exist for local/starter project/global skills; full built-in skill pack remains a follow-on. |
| 22 | Skill metadata/governance is separate from hooks/plugins/MCP execution; deferred fields do not execute. |
| 23 | Task summaries/metadata are capped and compact; full skill bodies are not echoed in parent outputs. |
| 24 | Restricted profile/toolset behavior remains authoritative even when skill metadata names tools. |

### Changed public contracts and guarding tests

- Added `skills.disabled` to `SkillsConfig`, schema, and docs; a `disabledIds` serde alias remains compatibility input but is not a generated-schema/docs contract.
- Added V1 local skill frontmatter fields: `name`, `description`, `argument_hint`, expected/allowed tools, target agent/category, deferred MCP/resource metadata, and metadata map handling; guarded by `skill_load_discovery_test`.
- Added compact `SkillCatalogEntry`/`SkillCatalogStatus` shared catalog seam; consumed by `skill`, `task(load_skills)`, doctor, support export, and TUI toggles.
- Clarified that `allowed_tools` never grants runtime permissions and `skills.urls` is inert/deferred metadata only.
- Clarified V1 scope terms: `project` and `global` are implemented; a separate built-in skill scope/pack is intentionally follow-up.

### Rejected alternatives and non-acceptance notes

- A manual `prompt --mock` with arbitrary text was rejected as a QA path because the mock provider requires scripted request digests; deterministic `run --scenario golden_path` plus `sessions export` was used for support-export CLI evidence instead.
- No event schema changes were made.
- No remote skill fetching, MCP startup, hook execution, arbitrary plugin behavior, or full built-in skill pack was added.

### Remaining follow-up work

- Add V1-quality built-in skill packs such as `git-master`, `review-work`, and `frontend-ui-ux` after this contract stabilizes.
- If Harness later adds a separate built-in skill source scope, extend the stable-id/disablement, doctor/support, precedence, and docs tests before checking the roadmap built-in-scope boxes.
- Implement skill resources/assets/scripts and skill-embedded MCP activation only in a later slice.
