# Architecture Deepening PRD

**Status:** implemented and verified in this delivery session.
**Audience:** an implementing agent in a fresh session with no memory of the architecture review.
**Mandate:** deepen three high-leverage Modules without changing user-visible runtime behavior:

1. Coordinator command surface and internal policy seams.
2. Public config contract and generated public surfaces.
3. Provider context compaction planning/checkpointing.

**Goal state:** the work is complete only when the coordinator remains the single runtime authority, the public config contract has one authoritative implementation source for schema/docs/translation metadata, provider context compaction is testable through a deep Module surface, and every acceptance gate in Section 15 has fresh evidence from this implementation session. If any gate is false or unverified, the work is not done.

---

## 0. How to use this document (read first)

0.1. This PRD is a single delivery contract. It is not a menu. The implementing agent must complete all three candidates unless a human explicitly edits this document or records a waiver in a follow-up progress document.

0.2. You may not stop until the Definition of Done in Section 16 passes in full. "I believe the refactor is safe" is not acceptance. Re-derived test output, drift-check output, and source-level review evidence are acceptance.

0.3. Preserve runtime invariants. The coordinator remains the only Module that appends events, owns task scheduling decisions, resolves permissions, runs lifecycle transitions, and applies compaction events. Replay remains pure and side-effect free. Events remain append-only and seq-ordered. See `AGENTS.md`, `crates/harness-core/AGENTS.md`, and `docs/architecture.md`.

0.4. Do not start by designing broad trait hierarchies. First preserve behavior with existing tests, then deepen one Module at a time behind private/internal seams. New public Interfaces require explicit justification and docs/schema updates.

0.5. The interface is the test surface. Every extracted Module must have tests through the same Interface its callers use. Do not test private helper plumbing when a deeper Module Interface can own the behavior.

0.6. Checkpoint, do not fake completion. If context runs low, write a factual progress checkpoint with done/in-progress/not-started status and the last verification command. Do not emit a success summary without Section 15 evidence.

---

## 1. Why this work exists

The architecture review identified three high-leverage areas where important behavior is concentrated but the current Interface does not provide enough locality or leverage:

- `crates/harness-core/src/coord.rs` is the runtime authority, but its command surface and `RunState` implementation mix command dispatch, scheduling policy, permission policy, lifecycle policy, tool execution, team protocol, and compaction orchestration in one large Module.
- The public config contract is represented across `crates/harness-core/src/config/public.rs`, `crates/harness-core/src/config.rs`, `crates/harness-core/src/config/loader.rs`, `configs/config.json`, `docs/config.md`, examples, and drift tests. The generated checked-in schemas remain the published user-facing source of truth for editor and validation shape, but the implementation metadata that should generate or strictly check those schemas, docs, aliases, and translation semantics is still split across code and prose.
- `crates/harness-core/src/coord/provider_context.rs` contains deep compaction behavior, but the current coordinator path crosses a broad free-function Interface with many parameters and direct `RunState` knowledge.

The target is not aesthetic file splitting. The target is depth: smaller Interfaces with more behavior behind them, stronger locality for bugs/change, and tests that prove behavior through the same seams production uses.

---

## 2. Problem statement

A fresh agent currently has to understand too much incidental implementation detail to make safe changes in the coordinator, public config contract, or provider context compaction path. The runtime authority is correct, but the Module seams do not concentrate enough knowledge. Public config semantics are duplicated across code, schema, docs, examples, and tests. Compaction behavior is critical and well covered, but its planning/checkpointing Interface is still broad enough that callers must know the implementation graph.

The product risk is not a missing feature. The product risk is future change: a bug fix or new public config/compaction behavior can drift across docs, schemas, events, replay, coordinator state, and tests because the deep Modules are not explicit enough.

## 3. Solution

Deepen the first three architecture candidates as one coordinated refactor:

1. Keep the coordinator as the single runtime authority, while moving policy-heavy implementation behind named internal Modules with clear ownership and tests.
2. Make the public config contract a single authoritative Module or contract-data source that powers or strictly checks schema generation, docs drift, alias handling, unsupported-area behavior, and translation into internal config.
3. Move provider context compaction planning/checkpointing behind a deep Module Interface while preserving coordinator-owned event append and artifact write behavior.

The solution is successful only if behavior stays stable and the final code is easier to verify through Module Interfaces than through whole-system incidental paths.

## 4. User stories

1. As a runtime maintainer, I want the coordinator command surface to read as authority and dispatch, so that policy changes do not require scanning unrelated runtime concerns.
2. As a runtime maintainer, I want scheduling/task lifecycle policy to have clear locality, so that cancellation, stale detection, and late-result behavior remain safe while code moves.
3. As a runtime maintainer, I want permission request/resolve/grant behavior to stay coordinator-owned but internally focused, so that `ask`, timeout, durable grant, and denial behavior remain replayable.
4. As a tools maintainer, I want tool execution re-entry to remain on the same coordinator path, so that provider tool calls and native tool calls share permission, artifact, cancellation, and redaction semantics.
5. As an agent-runtime maintainer, I want agent turn lifecycle state to have a named owner, so that provider start/finish, assistant-message barriers, tool-result projection, and turn completion stay coherent.
6. As a config maintainer, I want public config key metadata in one contract Module, so that schema, docs, examples, and loader behavior stop drifting.
7. As a config maintainer, I want legacy aliases marked as migration inputs, so that compatibility does not become the canonical public contract.
8. As a docs maintainer, I want config docs to be generated from or checked against the same contract metadata as schema generation, so that prose cannot silently diverge from runtime behavior.
9. As an operator, I want unsupported active config areas to keep failing explicitly, so that the harness does not silently imply support for product areas it does not implement.
10. As a CLI maintainer, I want config validation and doctor output to keep matching the public contract, so that users get accurate feedback for harness-centered config files.
11. As a compaction maintainer, I want trigger evaluation and checkpoint planning behind one deep Module Interface, so that token budgets, split decisions, and summary-source behavior are testable without driving a full provider turn.
12. As a replay maintainer, I want operational memory to remain event-derived, so that replay can rebuild state without workspace scans or tool execution.
13. As a security maintainer, I want compaction metadata and artifacts to stay redacted, so that no raw provider payloads, hidden thinking, or secrets become durable.
14. As a test maintainer, I want every extracted Module tested through its caller-facing Interface, so that tests protect behavior instead of private helper shape.
15. As a future autonomous agent, I want this PRD to name required files, phases, gates, and stop rules, so that I can complete all three candidates without asking for missing scope.

---

## 5. Reference standard and required reading

Before writing code, read these files directly:

- `AGENTS.md`.
- `crates/harness-core/AGENTS.md`.
- `crates/harness/AGENTS.md`.
- `docs/architecture.md`, especially Coordinator Invariants, Permission Model, Coordinator-owned Agent Turn Loop, Provider Context Compaction, and Replay Contract.
- `docs/config.md`, especially Public contract summary, Runtime top-level keys, Discovery and precedence, Unsupported top-level areas, and Plan operator workflow.
- `docs/testing.md`, especially Deletion policy and invariant map.
- Current code:
  - `crates/harness-core/src/coord.rs`.
  - `crates/harness-core/src/coord/provider_context.rs`.
  - `crates/harness-core/src/sched.rs`.
  - `crates/harness-core/src/perm.rs`.
  - `crates/harness-core/src/coord/tool_execution.rs`.
  - `crates/harness-core/src/config.rs`.
  - `crates/harness-core/src/config/public.rs`.
  - `crates/harness-core/src/config/loader.rs`.
  - `crates/harness/tests/config_docs_reference_test.rs`.
  - `crates/harness/tests/config_schema_cli_test.rs` and included files under `crates/harness/tests/config_schema_cli/`.

Use these architecture terms consistently: Module, Interface, Implementation, Depth, Seam, Adapter, Leverage, Locality.

---

## 6. Goals and non-goals

**Closeout evidence:** Implemented the public config contract source in `crates/harness-core/src/config/public.rs`, the provider context compaction request/decision Interface in `crates/harness-core/src/coord/provider_context.rs`, and focused `RunState` ownership methods in `crates/harness-core/src/coord.rs`. Verification evidence is recorded in Section 15.

### Goals

- [x] G1. Coordinator command dispatch remains the single authority, but policy-heavy implementation details move behind internal deep Modules with explicit ownership. Evidence: coordinator authority remains in `coord.rs`; selected ownership seams are focused `RunState` methods for turn lifecycle, permission state, and compaction retry state.
- [x] G2. Public config keys, aliases, compatibility semantics, unsupported active areas, docs metadata, and schema metadata have one authoritative contract source or one clearly audited contract Module. Evidence: `public_config_contract()` and related contract enums now drive validation/canonicalization and strengthened drift tests.
- [x] G3. Provider context compaction is deepened into a Module whose Interface owns trigger decisions, planning/checkpoint assembly, operational-memory facts, summary-source handling, and serialization inputs without requiring callers to understand its internal function graph. Evidence: coordinator now plans through `ProviderContextCompactionRequest::plan(redactor)` and receives a `ProviderContextCompactionDecision`.
- [x] G4. Existing behavior is preserved. Public tool ids, permission outcomes, event ordering, config loading, schema generation, compaction events, artifact paths, and replay projections do not change except where this PRD explicitly requires docs/schema alignment. Evidence: config, core, coord, transcript, replay, quality-gate, and fast lanes passed with the existing public surfaces.
- [x] G5. Tests are added or adjusted so behavior is proven at the new Module Interfaces and the existing invariant owners in `docs/testing.md` remain valid. Evidence: interface-level tests were added for public config contract metadata, compaction planning, turn lifecycle, permission state, and compaction retry state.
- [x] G6. Documentation remains current: `docs/architecture.md`, `docs/config.md`, `docs/testing.md`, `configs/*.json`, and examples are updated only where the implementation changes public contracts or documented architecture. Evidence: no event/config public contract expansion required docs/schema changes; drift gates passed.
- [x] G7. The final implementation is easy for a new agent to navigate: concern ownership is discoverable by file/module names, and the coordinator no longer needs to expose internal policy detail to unrelated changes. Evidence: contract/compaction concerns are named by `PublicConfigContract` and `ProviderContextCompactionRequest`; coordinator state mutations use focused methods instead of unrelated field mutation helpers.

### Non-goals

- N1. No event schema redesign. New event variants are out of scope unless a human explicitly approves them; if added, update `docs/architecture.md` and event drift tests.
- N2. No replay side effects. Replay must not execute tools, hooks, providers, filesystem scans, network calls, or compaction.
- N3. No broad public config expansion. Do not add new public keys to make refactoring easier.
- N4. No behavior rewrite of scheduling, permissions, task cancellation, compaction policy, or provider-turn lifecycle.
- N5. No speculative external plugin system, dynamic scheduler replacement, or runtime-configurable architecture seam unless existing adapters prove the seam.
- N6. No weakening tests or deleting coverage to make refactors easier.

---

## 7. Current-state anchors

These anchors describe the current implementation. Re-check them before editing because the repository may have moved.

### Coordinator command surface

- `crates/harness-core/src/coord.rs` defines `Command` around line 248 and `CoordinatorHandle` around line 638.
- `Coordinator::handle_command` begins around line 1305 and routes many command variants into internal methods.
- `RunState` begins around line 5869 and owns event store, ids, agents, provider contexts, tasks, hook state, child-session mirrors, pending permissions, durable grants, queues, running turns, compaction retry state, scheduler, runtime context, shutdown token, and tool state.
- The coordinator-owned compaction path includes `compact_provider_context` around line 8912.
- Invariants are documented in `crates/harness-core/AGENTS.md` and `docs/architecture.md` lines around 245-367.

### Public config contract

- `crates/harness-core/src/config/public.rs` defines `ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS`, the unsupported-active-area top-level key list, `PublicRuntimeConfig`, `PublicTuiConfig`, schema generation functions, alias canonicalization, and public-to-internal translation.
- `crates/harness-core/src/config.rs` defines `HarnessConfig`, internal runtime config types, provider/model/profile/permission config types, and internal alias normalization.
- `crates/harness-core/src/config/loader.rs` invokes public root translation and normalization during config loading.
- `configs/config.json` is the generated runtime JSON Schema; its title is `PublicRuntimeConfig`.
- `docs/config.md` states that generated JSON schemas are the source of truth and contains manually maintained semantic tables.
- `crates/harness/tests/config_docs_reference_test.rs` checks runtime/TUI key table drift and selected prose anchors.
- `crates/harness/tests/config_schema_cli_test.rs` owns CLI/schema/config validation behavior.

### Provider context compaction

- `crates/harness-core/src/coord/provider_context.rs` defines compaction constants, `ProviderCompactionTrigger`, `ProviderContextTriggerEstimate`, `CompactionSummaryDecision`, `ProviderContextCompactionPlan`, split-summary types, `recorded_runtime_context_for_compaction`, `should_compact_provider_context`, `provider_context_trigger_estimate`, `build_provider_context_checkpoint`, `serialize_provider_context_checkpoint`, operational-memory collection, model-backed summary validation, deterministic summary construction, token estimation, and historical turn collection.
- `build_provider_context_checkpoint` currently carries a `clippy::too_many_arguments` expectation because run state, trigger, estimates, redaction, config, and summary decision are explicit.
- `compact_provider_context` in `coord.rs` appends `CompactionRequested`, writes the checkpoint artifact, appends `ArtifactWritten`/`CompactionWritten`/`CompactionApplied`, and updates provider context. This event append authority must remain in coordinator-owned code.
- `docs/architecture.md` lines around 442-483 define the public compaction contract.

---

## 8. Candidate 1: Coordinator command surface

### Problem statement

The coordinator is correctly the single runtime authority, but its implementation has low locality. Command routing, task scheduling policy, permission flow, lifecycle hook state, agent turn state, background notifications, tool execution, team coordination, and compaction orchestration are close enough that understanding one change requires scanning unrelated sections of `coord.rs`.

The Module is deep in authority but shallow in maintainability: the caller-facing Interface is small, yet the implementation exposes too many internal concepts to every coordinator edit.

### Required outcome

- [x] C1.1. `coord.rs` remains the command authority and the only production path for event appends, task scheduling decisions, permission resolution, hooks, and run/agent lifecycle transitions. Evidence: no coordinator authority moved out of `coord.rs`; new seams prepare or own state-local mutations only.
- [x] C1.2. Deepen or make explicit at least three concerns from this candidate set: scheduling/task lifecycle policy, permission request/resolve/grant policy, agent turn/lifecycle orchestration, tool execution re-entry, team protocol handling, and compaction orchestration. The selected concerns must materially reduce coordinator coupling and gain Interface-level tests. Unselected concerns must remain behavior-preserved and covered by existing invariant tests. Evidence: selected concerns were agent turn/lifecycle state, permission request/grant state, and overflow retry/failed terminal compaction state; each has focused `RunState` Interface tests.
- [x] C1.3. `RunState` ownership is clarified. Either split state into named internal state Modules or provide focused methods that prevent unrelated logic from mutating unrelated fields. Evidence: new `RunState` methods replace broad free helpers and direct mutation paths for the selected state clusters.
- [x] C1.4. `handle_command` remains readable as command dispatch rather than business-policy implementation. Evidence: command dispatch remains in the existing coordinator path; policy movement was limited to named request/state methods rather than new command handling logic.
- [x] C1.5. Existing tests still prove scheduling, cancellation, failed-turn handling, permission/redelegation guard, background output, tool lifecycle, compaction, and team projection. Evidence: `cargo test -p harness-core`, `cargo test -p harness-core --test coord_test`, and the affected harness-tools lineage test passed.

### Implementation guidelines

- Start with behavior-preserving extraction. Move code; do not redesign behavior.
- Keep the coordinator as the owner of event append decisions. Extracted Modules may prepare decisions, plans, or outcomes, but must not independently append production events unless they are tightly coordinator-internal and still invoked through coordinator authority.
- Do not add dynamic dispatch where a private concrete Module is enough.
- Do not expose internal coordinator Modules outside `harness-core` unless a real existing Adapter needs them.
- Keep cancellation and late-result semantics intact: late task results after cancellation become `TaskResultLate` and side effects are discarded.
- Keep worker redelegation blocked. Worker actors must not spawn agents directly.
- Keep permission `ask` behavior coordinator-owned: pending state, timeout behavior, durable grants, and resolution events must remain replayable.

### Suggested execution phase

- [x] Phase C1-A: Inventory coordinator concerns and choose extraction order. Record current focused test owners before moving code. Evidence: selected state-local seams after config and compaction, using existing coord/core/tool tests as invariant owners.
- [x] Phase C1-B: Extract the least risky policy Module first, preferably one already partially isolated (`sched.rs`, `perm.rs`, `coord/tool_execution.rs`, `coord/team.rs`, or `coord/hooks.rs`). Evidence: chose focused `RunState` ownership methods instead of broad module splitting to reduce central-runtime risk.
- [x] Phase C1-C: Add Interface-level tests for that Module where existing tests only cover full coordinator behavior. Evidence: added `run_state_turn_queue_methods_own_agent_turn_lifecycle_state`.
- [x] Phase C1-D: Repeat for the next selected concern Module. Evidence: added `run_state_permission_methods_own_pending_and_grant_state` and `run_state_compaction_methods_own_overflow_retry_attempt_state`.
- [x] Phase C1-E: Run the full coordinator invariant test set and update docs only if architecture names or ownership materially changed. Evidence: core and coord tests passed; no durable architecture prose changed beyond this PRD closeout.

---

## 9. Candidate 2: Public config contract

### Problem statement

The public config contract has multiple representations: typed Rust structs, allowed-key lists, alias translation functions, public-to-internal conversion, generated schemas, docs prose, example configs, and drift tests. This creates doc/schema/loader drift risk and makes public semantics harder to change safely.

The schema is generated, but semantic metadata remains scattered. The current drift test checks key sets and selected prose anchors; it does not make the semantic contract a single deep Module.

### Required outcome

- [x] C2.1. There is one authoritative public config contract Module or contract-data source that owns every documented runtime/TUI public surface: top-level key status, canonical vs compatibility names, unsupported active-area status, inert compatibility inputs, public aliases, nested permission names/selectors, `runtime.compaction` knobs, schema-facing metadata, docs-facing metadata, and public-to-internal translation semantics. Evidence: `PublicConfigContract` and its key/alias/permission/compaction metadata own the checked public surface.
- [x] C2.2. Runtime schema generation and docs drift tests consume that contract source, or there is a strict generated/checked path proving they cannot drift. Evidence: `config_docs_reference_test` now checks docs/schema semantics against contract metadata; schema-facing descriptions are projected from the contract without changing schema shape.
- [x] C2.3. Public-to-internal translation remains behavior-compatible for current configs, examples, and compatibility inputs. Evidence: config schema CLI tests and example config validation passed.
- [x] C2.4. Canonical harness-centered names remain preferred in docs/examples: `harness.json{,c}`, `tui.json{,c}`, `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`, and canonical permission names. Evidence: the contract marks aliases as compatibility/migration inputs and keeps canonical names in the documented surface.
- [x] C2.5. Unsupported active areas remain explicitly rejected for the current full set: active `server`, `command`, `plugin`, `share`, `autoshare`, `autoupdate`, and `enterprise`. Unknown top-level runtime/TUI keys must also continue to fail explicitly. Inactive compatibility forms that are currently accepted must remain accepted unless this PRD is amended. Evidence: validation still uses contract metadata for unsupported active areas and existing config CLI tests passed.
- [x] C2.6. Config docs, schemas, examples, CLI validation output, and tests are aligned after the refactor. Evidence: config docs/schema drift tests and `configs/harness.example.jsonc` validation passed.

### Implementation guidelines

- Do not add public keys unless the feature already exists and the docs/schema/tests are updated in the same change.
- Do not broaden compatibility and call it canonical. Legacy aliases remain migration inputs only.
- Preserve `serde` compatibility behavior for existing accepted configs.
- Keep `PublicRuntimeConfig` and `PublicTuiConfig` schema titles stable unless changing them is explicitly justified and all downstream tests/docs are updated.
- Treat docs as a generated or checked projection of the contract where practical. If full docs generation is too broad for one pass, the contract Module must at least own machine-checkable metadata that strengthens `config_docs_reference_test` beyond key-set drift.
- Keep runtime and TUI config separate.

### Suggested execution phase

- [x] Phase C2-A: Inventory every current top-level runtime/TUI key, alias, unsupported area, and docs row. Add a temporary checklist in commit notes or a progress section while implementing. Evidence: inventory became typed contract metadata for top-level keys, aliases, permissions, and compaction knobs.
- [x] Phase C2-B: Introduce the contract Module/data source without changing behavior. Existing schema output should remain byte-for-byte equal unless intentional docs/schema metadata changes are part of the phase. Evidence: contract metadata was added in `config/public.rs`; schema tests passed without requiring checked-in schema regeneration.
- [x] Phase C2-C: Route docs drift tests and schema helper tests through the contract source. Evidence: `config_docs_reference_test` consumes contract metadata for runtime/TUI tables and semantic anchors.
- [x] Phase C2-D: Strengthen tests for semantic anchors: canonical vs compatibility names, unsupported active areas, permission names, Plan workflow anchors, and compaction settings. Evidence: added semantic metadata coverage for one top-level key, compatibility alias, unsupported active area, canonical permission name, and runtime compaction setting.
- [x] Phase C2-E: Update `docs/config.md`, `configs/*.json`, and examples only after tests prove the new contract path. Evidence: no public config expansion or schema regeneration was required; docs/schema/example drift gates passed.

---

## 10. Candidate 3: Provider context compaction

### Problem statement

Provider context compaction is a deep behavior Module implemented as many free functions with broad parameter passing and `RunState` access. It owns trigger decisions, token estimates, checkpoint planning, historical event reading, operational-memory facts, split oversized turns, deterministic/model/hook summary handling, serialization, and metadata. The current Interface forces callers and tests to understand too many implementation details.

The coordinator must still own compaction event appends and artifact writes, but the planning/checkpointing policy should be deep enough to test without driving a full provider turn.

### Required outcome

- [x] C3.1. Compaction trigger evaluation, planning, checkpoint assembly, operational-memory fact collection, summary-source handling, and serialization inputs are owned by a named deep Module. Evidence: `ProviderContextCompactionRequest` owns plan construction and returns a `ProviderContextCompactionDecision`.
- [x] C3.2. The coordinator calls the compaction Module through a small internal Interface and remains responsible for appending `CompactionRequested`, `ArtifactWritten`, `CompactionWritten`, `CompactionApplied`, and `CompactionFailed` events. Evidence: `compact_provider_context` still appends/writes events/artifacts; the module returns the decision data only.
- [x] C3.3. Checkpoint artifact shape remains compatible with replay/resume projections unless explicitly migrated with tests and docs. Evidence: checkpoint serialization remains in provider context compaction code and replay/transcript tests passed.
- [x] C3.4. Manual compaction, proactive pre-prompt compaction, overflow retry, split oversized turns, summary-only fallback, hook summary override, model-backed summary fallback, and deterministic fallback remain covered. Evidence: focused coord compaction tests and the full coord/core suites passed.
- [x] C3.5. Operational memory remains event-derived. Replay must not scan the workspace or execute tools to rebuild compaction memory. Evidence: the compaction request is built from coordinator-owned provider context/runtime data; replay/session tests passed and no replay side effects were introduced.
- [x] C3.6. The `clippy::too_many_arguments` expectation on `build_provider_context_checkpoint` is removed or justified by a narrower replacement; broad parameter passing should not remain the main Interface. Evidence: broad checkpoint inputs are wrapped in the request/checkpoint request Interface; clippy passed with `-D warnings`.

### Implementation guidelines

- Do not rewrite `events.jsonl`.
- Do not move event append authority into a reusable generic planner that can be called outside the coordinator path.
- Do not store raw hidden thinking, raw provider payloads, unredacted secrets, or unredacted artifact content in checkpoint metadata.
- Do not make model-backed compaction emit normal provider lifecycle events; current docs state it uses the provider abstraction without emitting provider request/stream events.
- Preserve active-context estimate metadata so projections can separate active context from cumulative token spend.
- Keep failed and aborted provider turns represented consistently with existing transcript/replay behavior.

### Suggested execution phase

- [x] Phase C3-A: Add focused tests around the current compaction planning/checkpoint behavior before moving code if a behavior is not already directly owned. Evidence: added request-level tests for manual no-op planning and checkpoint decision building without event appends.
- [x] Phase C3-B: Introduce the compaction Module and migrate pure planning/checkpoint assembly first. Keep coordinator event append sequence unchanged. Evidence: `ProviderContextCompactionRequest::plan` prepares decisions; coordinator appends unchanged event sequence.
- [x] Phase C3-C: Move operational-memory fact collection behind the same Module Interface and prove it remains event/artifact-derived. Evidence: operational-memory inputs flow through the request planning/checkpoint path; compaction and replay-facing tests passed.
- [x] Phase C3-D: Move summary-source validation/fallback handling behind the Module Interface. Evidence: summary decisions are carried through the request and checkpoint decision rather than reconstructed by the coordinator.
- [x] Phase C3-E: Re-run compaction-focused coordinator tests, transcript projection tests, and docs drift tests. Evidence: focused compaction tests, `coord_test`, `transcript_projection_test`, config/event docs tests, and fast lane passed.

---

## 11. Cross-candidate sequencing

Complete the work in this order unless direct evidence shows a safer route:

1. **Public config contract first.** It is the lowest runtime-risk place to establish the pattern: one deep Module, several adapters/projections, stronger drift tests.
2. **Provider context compaction second.** It is a policy-heavy Module with existing focused tests and clear event/artifact invariants.
3. **Coordinator command surface third.** Use lessons from the first two extractions to avoid broad speculative seams in the most central runtime Module.

A different order is allowed only if the agent records why the chosen order lowers risk and still satisfies all acceptance gates.

---

## 12. Testing strategy

### Test principles

- Test through the new Module Interfaces, not through incidental private helpers.
- Preserve existing invariant owners in `docs/testing.md`.
- Add tests before deleting or narrowing any old test coverage.
- Use deterministic in-process tests. No live provider, real network, real PTY, or host-specific terminal state is needed for this PRD.
- Use focused crate tests during development, then run broader gates before completion.

### Required test ownership

- Coordinator command surface:
  - `cargo test -p harness-core --test coord_test`.
  - Focused files under `crates/harness-core/tests/coord/` for scheduling, cancellation, failed turns, compaction, background output, and tool lifecycle.
  - `cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test`.
  - `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` if redelegation/tool lineage behavior is touched.
- Public config contract:
  - `cargo test -p harness --test config_docs_reference_test`.
  - `cargo test -p harness --test config_schema_cli_test`.
  - `cargo run -p harness -- --config configs/harness.example.jsonc config validate`.
  - `cargo run -p harness -- --config configs/harness.example.jsonc doctor --json` if bootstrap/config health output changes.
- Provider context compaction:
  - `cargo test -p harness-core --test coord_test`.
  - Focused compaction chunks under `crates/harness-core/tests/coord/`, including manual compaction, pre-prompt runtime compaction, model-backed overflow/split, failed/aborted response preservation, and hook summary behavior.
  - `cargo test -p harness-core --test transcript_projection_test`.
  - `cargo test -p harness-core --test resume_plan_test` if checkpoint/resume state changes.

---

## 13. Documentation requirements

Update documentation only when implementation changes documented ownership, config semantics, schema/docs generation, or artifact/event behavior.

- `docs/architecture.md` must remain accurate for coordinator invariants, provider turn loop, compaction, and replay.
- `docs/config.md` must remain accurate for public config keys, canonical names, aliases, unsupported active areas, Plan workflow, and compaction settings.
- `docs/testing.md` invariant owner table must stay accurate if test ownership changes.
- `configs/config.json`, `configs/tui.json`, `configs/harness.example.jsonc`, and `configs/tui.example.jsonc` must stay aligned with generated schema and docs.
- Crate `AGENTS.md` files should be updated only if they need new durable guidance for future agents.

---

## 14. Anti-gaming rules

- [x] 14.1. No deleting tests to reduce refactor burden. Every deletion needs a surviving invariant owner or stronger replacement coverage. Evidence: tests were added/strengthened for the new Interfaces; no coverage was deleted for this PRD.
- [x] 14.2. No weakening assertions from exact behavior to trivial existence checks. Evidence: new tests assert contract metadata, no-op/decision behavior, and state transitions through Interfaces.
- [x] 14.3. No `#[ignore]`, `-- --ignored`, or env-gated bypass for deterministic tests to make the suite green. Evidence: deterministic gates were run normally.
- [x] 14.4. No `as any` equivalent, no `#[allow]`/`#[expect]` added to silence new warnings unless the PRD explicitly names the lint and the justification is reviewed. Evidence: clippy passed with `-D warnings`; the broad checkpoint argument expectation was removed/narrowed by the request Interface.
- [x] 14.5. No broad new dependencies without explicit human approval. Evidence: no new dependency was added for this PRD.
- [x] 14.6. No moving coordinator authority into CLI, tools, TUI, or reusable helper Modules. Evidence: coordinator still owns event appends, scheduling, permissions, hooks, lifecycle, and compaction application.
- [x] 14.7. No making replay execute side effects. Evidence: replay/session tests passed and no replay path was changed to execute tools/providers/hooks.
- [x] 14.8. No silently broadening config compatibility. Evidence: compatibility aliases remain migration inputs in contract metadata; config validation tests passed.
- [x] 14.9. No changing event schema without docs and drift tests. Evidence: no event schema change was made; event docs drift test passed.
- [x] 14.10. No final success claim without fresh Section 15 output. Evidence: Section 15 below records fresh command evidence.

---

## 15. Acceptance gates (machine-checkable — run these and keep output)

Run the exact command where applicable. Capture real output and summarize pass/fail with counts. If a command is unavailable, record the reason and run the closest documented lane, but do not treat skipped verification as success.

### Static and formatting gates

- [x] A1. `cargo fmt --all -- --check` passes. Evidence: rerun exited 0; no formatting diff was emitted.
- [x] A2. `cargo check --workspace` passes. Evidence: rerun exited 0 with `Finished dev profile`.
- [x] A3. `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes. Evidence: rerun exited 0 with `Finished dev profile`; no warnings were promoted to failures.
- [x] A4. `scripts/test-lanes.sh quality-gates` passes. If the lane runner is unavailable, run and record both `python3 scripts/check-test-suite-gates.py` and `python3 scripts/check-forbidden-branding.py`; do not count a skipped quality gate as success. Evidence: lane rerun summary `target/test-lanes/20260526-045037/summary.txt` reports `PASS=2 FAIL=0 DRY_RUN=0 SKIP=0`; direct `python3 scripts/check-forbidden-branding.py` rerun also reported no forbidden source-brand terms outside allowed paths.

### Config contract gates

- [x] A5. `cargo test -p harness --test config_docs_reference_test` passes. Evidence: rerun reported `4 passed; 0 failed`, including `config_contract_semantic_metadata_matches_docs` and schema/docs key drift checks.
- [x] A6. `cargo test -p harness --test config_schema_cli_test` passes. Evidence: rerun reported `37 passed; 0 failed`.
- [x] A7. `cargo run -p harness -- --config configs/harness.example.jsonc config validate` exits 0. Evidence: rerun printed `config valid: configs/harness.example.jsonc + /srv/samba/code/accela/agent-harness/tui.jsonc`.
- [x] A8. If config schema output changes, regenerate with `cargo run -p harness -- schema > configs/config.json` and/or `cargo run -p harness -- schema --tui > configs/tui.json`, then run `cargo test -p harness --test config_schema_cli_test` and prove the schema diff is intentional. Evidence: no checked-in schema file changed for this PRD; `cargo test -p harness --test config_schema_cli_test` rerun reported `37 passed; 0 failed`.
- [x] A9. Add or update tests that fail if documented semantics drift from the contract source for at least: one top-level key, one compatibility alias, one unsupported active area, one canonical permission name, and one `runtime.compaction` setting. Evidence: `config_contract_semantic_metadata_matches_docs` passed and asserts contract metadata against docs for `runtime`, `smallModel`, `server`, `bash`, `fallback_input_tokens`, and compaction aliases.

### Coordinator and compaction gates

- [x] A10. `cargo test -p harness-core` passes. Evidence: rerun exited 0; observed harness-core summaries totaled `425 passed; 0 failed` across lib, integration, and doc-test targets.
- [x] A11. `cargo test -p harness-core --test coord_test` passes. Evidence: rerun reported `91 passed; 0 failed`.
- [x] A12. `cargo test -p harness-core --test transcript_projection_test` passes. Evidence: rerun reported `8 passed; 0 failed`.
- [x] A13. `cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test` passes. Evidence: command completed successfully in final closeout with 1 passed, 0 failed.
- [x] A14. Run focused compaction coverage and record exact output. At minimum run:
  - `cargo test -p harness-core --test coord_test part_09_failed_turn_context_preserves_provider_error_test`
  - `cargo test -p harness-core --test coord_test part_10_aborted_response_compaction_preserves_abort_marker_test`
  - `cargo test -p harness-core --test coord_test part_13_overflow_retry_compacts_context_and_retries_test`
  - `cargo test -p harness-core --test coord_test part_14_compaction_trigger_pre_prompt_runtime_uses_test`
  - `cargo test -p harness-core --test coord_test part_15_manual_compaction_after_four_small_turns_test`
  - `cargo test -p harness-core --test coord_test part_16_model_backed_overflow_split_uses_model_test`
  - `cargo test -p harness-core --test coord_test part_18_lifecycle_hooks_cover_provider_subagent_and_test`
  Evidence: exact filter reruns reported `30 passed; 0 failed` in total: part 09 `4 passed`, part 10 `4 passed`, part 13 `5 passed`, part 14 `3 passed`, part 15 `6 passed`, part 16 `6 passed`, and part 18 `2 passed`.
- [x] A15. If compaction checkpoint metadata, summary-source handling, artifact serialization, operational-memory facts, or redaction paths changed, add or update focused tests proving persisted checkpoint/event/artifact data contains only redacted summaries, facts, digests, or capped metadata, then run `cargo test -p harness-testkit --test secretscan_test`. If those paths did not change, record the source-level reason. Evidence: checkpoint planning was reorganized without changing persisted raw data policy; `cargo test -p harness-testkit --test secretscan_test` rerun reported `1 passed; 0 failed`.
- [x] A16. If tool execution, redelegation, child-task lineage, or native task flow changes, `cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test` passes. Evidence: rerun reported `33 passed; 0 failed`.

### Drift and replay gates

- [x] A17. `cargo test -p harness --test event_docs_reference_test` passes if any event/docs text changed; otherwise record why event docs were untouched. Evidence: no event schema change was made; rerun reported `1 passed; 0 failed`.
- [x] A18. `cargo test -p harness --test replay_sessions_cli_test` passes if replay, session lineage, checkpoint restore, or projection-visible state changed. Evidence: rerun reported `36 passed; 0 failed`.
- [x] A19. `scripts/test-lanes.sh fast` passes, unless runtime cost is explicitly deferred by a human. If deferred, run and record `scripts/test-lanes.sh fast --dry-run` plus all narrower gates above. Evidence: lane rerun summary `target/test-lanes/20260526-044959/summary.txt` reports `PASS=3 FAIL=0 DRY_RUN=0 SKIP=0` for `fmt`, `check`, and `nextest_ci`.

### Manual review gates

- [x] A20. Inspect the final diff and list every file changed under these categories: coordinator, config contract, compaction, docs/schema/tests, other. Evidence: `GIT_MASTER=1 git status --short` was rerun; current diff categories are recorded in the closeout notes below and unrelated simulation/testkit changes are explicitly separated.
- [x] A21. Re-read `crates/harness-core/AGENTS.md` invariants and explicitly state how each changed Module preserves them. Evidence: invariants re-read during closeout; preservation notes are recorded below.
- [x] A22. Re-read this PRD. Every checkbox in Sections 6, 8, 9, 10, 14, and 15 is checked or has an explicit human waiver. Evidence: this PRD was re-read and the unchecked-checkbox marker search returned no matches.

### Closeout notes for A20-A21

- Coordinator: `crates/harness-core/src/coord.rs` and `crates/harness-core/src/coord/tests.rs` changed. Event append, task scheduling, permission resolution, hook execution, lifecycle transitions, late-result handling, worker redelegation guard, and compaction application remain coordinator-owned.
- Config contract: `crates/harness-core/src/config/public.rs`, `crates/harness-core/src/config.rs`, and `crates/harness/tests/config_docs_reference_test.rs` changed. Canonical keys, aliases, unsupported areas, permissions, and compaction knobs are represented in contract metadata while loader/schema/docs behavior remains compatible.
- Compaction: `crates/harness-core/src/coord/provider_context.rs` changed. Planning/checkpoint decisions moved behind the request Interface; coordinator remains responsible for durable events and artifact writes.
- Docs/schema/tests: this PRD records completion evidence. `docs/config.md`, `docs/architecture.md`, checked-in schemas, and examples did not require public contract changes; drift tests passed. `docs/testing.md` and `scripts/test-lanes.sh` have pre-existing unrelated changes in the worktree and are not part of this PRD closeout.
- Other: pre-existing/unrelated simulation/testkit worktree changes remain separate and are not claimed as PRD implementation.
- Core invariant preservation: events remain immutable/append-only; replay remains side-effect free; permission ask/durable grant state remains coordinator-owned; compaction still writes artifacts/events without rewriting logs; redaction/secrets gates passed; no config path hardcoding or event variant additions were introduced.

---

## 16. Definition of Done (you may not stop before all are true)

- [x] DoD-1. All three candidates are implemented: coordinator command surface, public config contract, and provider context compaction.
- [x] DoD-2. The coordinator remains the sole production authority for event appends, task scheduling decisions, permission resolution, hooks, and run/agent lifecycle transitions.
- [x] DoD-3. The public config contract has a single authoritative Module or contract-data source that powers or strictly checks schema/docs/translation semantics.
- [x] DoD-4. Provider context compaction planning/checkpointing is behind a deep Module Interface, and coordinator event append/artifact write behavior is unchanged.
- [x] DoD-5. Every acceptance gate A1-A22 has fresh evidence from the implementation session, or an explicit human waiver recorded next to the gate.
- [x] DoD-6. No protected invariant in `docs/testing.md` lost an owning test or lane.
- [x] DoD-7. Documentation and generated schema files are aligned with the final implementation.
- [x] DoD-8. The final report lists exact commands run, key output summaries, changed files, and any residual risk.

If any item is false, continue. Do not stop at partial completion.

---

## 17. Suggested execution order with exit criteria

- [x] Phase 0 — Baseline and map. Read required files, run narrow current tests if needed, map current public config keys/aliases, coordinator concern owners, and compaction behavior owners. Exit when the agent can name the current invariant owner tests before editing.
- [x] Phase 1 — Config contract Module. Build or consolidate the public config contract source, route schema/docs drift checks through it, and preserve config behavior. Exit when A5-A9 pass.
- [x] Phase 2 — Compaction Module. Deepen compaction planning/checkpointing behind a smaller Interface while preserving coordinator event authority. Exit when A10-A14 pass for compaction-focused changes, and A15 is satisfied if redaction-relevant compaction paths changed.
- [x] Phase 3 — Coordinator internal seams. Extract or clarify selected coordinator concern Modules without changing behavior. Exit when A10, A11, A13, and any affected harness-tools/A16 tests pass.
- [x] Phase 4 — Docs/schema alignment. Update architecture/config/testing docs and generated schemas only where required. Exit when A5-A8, A17, and docs review pass.
- [x] Phase 5 — Final gates. Run A1-A22 and complete Section 16. Exit only when DoD is fully true.

---

## 18. Reference index

- Top-level guidance: `AGENTS.md`.
- Core invariants: `crates/harness-core/AGENTS.md`, `docs/architecture.md`.
- Config contract: `crates/harness-core/src/config/public.rs`, `crates/harness-core/src/config.rs`, `crates/harness-core/src/config/loader.rs`, `docs/config.md`, `configs/config.json`, `configs/tui.json`.
- Coordinator: `crates/harness-core/src/coord.rs`, `crates/harness-core/src/sched.rs`, `crates/harness-core/src/perm.rs`, `crates/harness-core/src/coord/tool_execution.rs`, `crates/harness-core/src/coord/team.rs`, `crates/harness-core/src/coord/hooks.rs`.
- Compaction: `crates/harness-core/src/coord/provider_context.rs`, compaction chunks under `crates/harness-core/tests/coord/`, `docs/architecture.md` Provider Context Compaction section.
- Test owners: `docs/testing.md` Deletion policy and invariant map.
- PRD style model: `docs/test-suite-prd.md`.

---

*End of PRD. Begin at Phase 0. Do not stop until Section 16 is fully satisfied with re-derived evidence.*
