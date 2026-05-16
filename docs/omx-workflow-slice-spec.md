# OMX-style workflow slice specification

This document defines the next large Agent Harness slice after the first OMO parity work. It is intentionally separate from `docs/omo-parity-spec.md` because the goal is no longer only parity with `inspirations/oh-my-openagent/`. The goal is a Harness-native workflow layer inspired mainly by `inspirations/oh-my-codex/`, with selective ideas from `inspirations/senpi/` and remaining OMO gaps.

The slice should make Harness feel closer to oh-my-codex: a small set of memorable workflow entrypoints, durable workflow state, strong setup and doctor surfaces, visible runtime status, and evidence-gated completion. It must still preserve the Harness contract: the coordinator owns event append, scheduling, permissions, tool execution re-entry, continuation, and task state. Replay remains side-effect free.

## Source-backed inspiration

### oh-my-codex

Primary inspiration comes from these source areas:

- `inspirations/oh-my-codex/README.md`: OMX positions itself as a workflow layer around Codex, not a replacement runtime. The default path is `omx --madmax --high`, then `$deep-interview`, `$ralplan`, `$team`, `$ralph`, and `$ultragoal`.
- `inspirations/oh-my-codex/DEMO.md`: setup installs prompts, skills, config, AGENTS guidance, notification hooks, HUD config, and then doctor validates the install. The demo treats workflow keywords, AGENTS orchestration, MCP state, team runtime, and HUD as product surfaces.
- `inspirations/oh-my-codex/docs/STATE_MODEL.md`: workflow modes have explicit state, transition rules, allowed overlaps, denied rollbacks, and lifecycle outcomes such as `finished`, `blocked`, `failed`, `userinterlude`, and `askuserQuestion`.
- `inspirations/oh-my-codex/docs/codex-native-hooks.md`: native hooks, plugin hooks, and fallback runtime hooks are documented as separate proof boundaries. It also defines explicit stop outcomes, continuation behavior, and wiki lifecycle capture.
- `inspirations/oh-my-codex/docs/hooks-extension.md`: hook-driven runtime extension ideas that should map onto typed Harness hook policies instead of arbitrary script execution.
- `inspirations/oh-my-codex/src/mcp/memory-server.ts` and `inspirations/oh-my-codex/src/mcp/lifecycle-telemetry.ts`: memory/context and lifecycle telemetry concepts that should become event-derived Harness projections or artifacts.
- `inspirations/oh-my-codex/docs/plugin-bundle-ssot.md`: setup assets, plugin skills, MCP metadata, native agents, and prompt files have a single source of truth plus sync and verification commands.
- `inspirations/oh-my-codex/docs/wiki-feature.md` and `inspirations/oh-my-codex/skills/wiki/SKILL.md`: the wiki is repository-visible markdown project knowledge under `omx_wiki/`, search-first rather than vector-first, and can be used before broader repository search.
- `inspirations/oh-my-codex/missions/README.md` and `inspirations/oh-my-codex/skills/autoresearch/SKILL.md`: research missions have `mission.md`, `sandbox.md`, validator contracts, iteration ledgers, and artifact-gated completion.
- `inspirations/oh-my-codex/skills/deep-interview/SKILL.md`: deep interview is intent-first, asks one question per round, gathers discoverable code facts before user questions, scores ambiguity, and persists a context snapshot before downstream handoff.
- `inspirations/oh-my-codex/skills/ralplan/SKILL.md`: planning is a consensus loop with planner, architect, and critic roles, explicit decision drivers, options, ADR output, and a pre-execution gate for vague execution requests.
- `inspirations/oh-my-codex/skills/ralph/SKILL.md`: execution is persistence plus verification, not one-shot work. It requires context intake, fresh evidence, architect review, deslop, regression re-verification, and a completion audit.
- `inspirations/oh-my-codex/skills/team/SKILL.md`: team mode is a durable operator workflow with shared task state, mailbox, optional worktrees, tmux panes, startup proof, status, resume, and shutdown proof.
- `inspirations/oh-my-codex/skills/ultragoal/SKILL.md`: large work can be split into durable goals with a ledger, checkpoint evidence, and final quality gate.
- `inspirations/oh-my-codex/skills/hud/SKILL.md`: runtime status is a first-class operator surface, not buried in logs.

### Senpi

Senpi contributes the "small core, powerful extension defaults" shape:

- `inspirations/senpi/README.md`: Senpi keeps the surface light, ships opinionated builtin extensions, and tracks core modifications through `changes.md` files to keep rebases clean.
- `inspirations/senpi/packages/coding-agent/README.md`: the runtime supports interactive, print/JSON, RPC, and SDK modes. It emphasizes packages, skills, prompt templates, extensions, themes, session branching, configurable keybindings, message queue, compaction, and session export/share.
- `inspirations/senpi/packages/coding-agent/AGENTS.md`: extension API first, core modifications last. Add tools and commands through extensions where possible. Keep high-conflict session lifecycle changes small and documented.
- Senpi builtin extension examples map well to Harness seams: permission-system, todowrite, dynamic-prompt, prompt-preset, GPT apply-patch, bash-timeout, tool-pair-guard, compaction, comment checker, LSP, AST-grep, sandbox, webfetch, and websearch.

### oh-my-openagent remainder

OMO remains useful for agent/team/tool breadth:

- `inspirations/oh-my-openagent/docs/reference/features.md`: agents, category routing, background agents, Team Mode, LSP, AST-grep, session tools, MCP tiers, skill-embedded MCP, model fallback, hooks, IntentGate, and compatibility remain the broad reference surface.
- `docs/omo-parity-spec.md`: the first parity slice already identified Harness-native seams for extension registration, hooks, agent catalog, skills, MCP, terminal sessions, persistent tasks, continuation, team mode, browser/media, provider fallback, doctor, docs, and tests.
- `docs/parity-ledger.json`: current gaps remain team worktrees and tmux visualization, AST-grep non-dry-run apply semantics, terminal fallback/signoff, browser/media signoff, executable skill MCP, hook implementations, and provider/model capability diagnostics.

## Product north star

Harness should gain a workflow layer that feels like this:

1. The user can start with a vague or broad goal and use one memorable command to clarify and run it.
2. The clarified goal becomes a durable context snapshot plus a replay-derived run dossier.
3. The operator can see the current phase, lane, evidence state, blockers, and next decision without reading raw logs.
4. The workflow can simulate the coordinator-owned path deterministically before env-gated live execution.
5. Completion requires evidence mapped to acceptance criteria, not a model statement.
6. State is durable, inspectable, restart-safe, replay-safe, and coordinator-owned.
7. Optional compatibility imports are adapters, not hidden plugin execution.

This should feel more like an operating layer than a bag of tools, but the first implementation must prove one narrow loop before broader workflow surfaces expand.

## Current baseline and redo matrix

Use `docs/parity-ledger.json` as an intake baseline, not as a design oracle. Every reused surface still needs contract review against workflow requirements: coordinator ownership, replay purity, projection availability, permission ordering, artifact redaction/capping, restart behavior, and lane coverage.

| Surface | Current status | Slice action | Notes |
| --- | --- | --- | --- |
| Event-sourced coordinator and replay contract | stronger | Reuse after contract review | Preserve as the source of truth. Do not add workflow state outside events plus referenced artifacts. |
| `task`, `background_output`, child metadata | present | Reuse after contract review | Workflow lanes should add metadata/evidence classification, not a second delegation path. |
| Continuation / Ralph / ultrawork loop | present | Reuse after contract review | Build work-loop semantics on `ContinuationStarted`, `ContinuationReminderQueued`, `ContinuationStopped`, and `ContinuationLimitReached`; do not add another loop scheduler. |
| Persistent task tools | present | Reuse after contract review | Use for durable cross-run work items where appropriate; do not confuse them with scheduler tasks or team checklist tasks. |
| Session tools, replay/session repair | present | Reuse after contract review | Dossier and status inspection should consume projections and session history without executing tools. |
| Provider/model fallback and doctor JSON baseline | present | Reuse after contract review | Extend stable check ids instead of inventing a separate workflow doctor format. |
| Team Mode MVP | partial | Harden / wrap with workflow metadata | Team ritual is policy/projection over `TeamTask*` and `TeamMessage*`, not a second scheduler. Worktrees, file claims, mailbox artifact refs, and tmux diagnostics are follow-up hardening unless needed by the demonstrator. |
| Terminal / PTY / browser / media evidence | partial | Harden when touched | Deterministic simulator should not depend on these; PTY/live/browser/native lanes are required only when those surfaces change. |
| Hook middleware | partial | Harden through typed policies | Add workflow policies over the existing typed hook seam. Do not reintroduce shell hook execution. |
| Skill MCP lifecycle | partial | Harden later | Keep skill MCP scoped and visible, but defer executable/OAuth depth unless required by the demonstrator. |
| Agent catalog and category routing | partial | Harden / wrap with workflow metadata | Workflow role lanes must record resolved catalog metadata and restrictions. |
| AST-grep apply semantics | partial | Harden later | Not part of the first workflow spine unless the demonstrator needs it. |
| Wiki, broad research missions, broad extension manifests, OAuth MCP, executable plugins | gap/planned | Defer | Keep as source-backed future surfaces. Do not make them foundation blockers. |
| OMO-shaped public command contracts that conflict with workflow semantics | mixed | Replace only when actively wrong | Prefer Harness-canonical `/workflow ...` names with aliases as thin compatibility wrappers. |

Redline: do not replace working coordinator, continuation, task, team, session, or provider foundations merely because they came from the last parity slice. Redo semantic mismatches, not working invariants.

## Non-negotiable Harness invariants

- Coordinator owns every event append and state transition.
- Tools never append events directly and never schedule work outside coordinator mediation.
- Replay projects state only. It must not execute hooks, tools, MCPs, shell commands, provider calls, terminal actions, browser actions, team members, validators, wiki scans, or simulator actions.
- Workflow state that affects resume, status, continuation, signoff, or audit must be event-derived or stored as redacted artifacts referenced by events.
- Permissions must be resolved before side effects.
- Provider payloads, secrets, raw hidden thinking, auth headers, raw browser content, raw validator output, and unredacted external tokens must not be durable event data.
- Project-visible files are never authoritative workflow state unless a future explicit import command validates and translates them through coordinator-owned events.
- Compatibility with external workflow systems must be implemented as import, translation, manifest, or prompt/profile adapters unless and until a safe extension runtime exists.
- New public config keys require schema, docs, drift tests, and example config updates.
- New workflow surfaces require CLI/TUI visibility and test-lane mapping only when those surfaces are actually touched.

## Slice scope

This document is intentionally broad as a source-backed direction, but the first shippable slice is narrow. The slice is complete only when a deterministic goal-loop demonstrator proves the reusable workflow substrate end to end.

### First shippable slice

Included in the first slice:

- Minimal workflow lifecycle state and replay projection.
- Context snapshot or lightweight intake artifact.
- `/workflow run`, `/workflow status`, and `/workflow signoff` as canonical surfaces.
- Optional compatibility aliases for familiar workflow names, behind the normal command registry.
- Run Dossier projection with optional export artifact.
- Deterministic testkit-backed simulation path.
- Evidence classification and signoff blocking.
- Projection-only status, replay, and doctor inspection.
- Minimal operator decisions: approve-live, redirect, request-evidence, abort/signoff-failed, signoff-approved.
- Early first-party SSOT/drift checks for workflow commands, aliases, evidence categories, and doctor/docs entries.

### Follow-up surfaces after the demonstrator

Follow-up waves may add or harden:

- Full consensus planning workflow.
- Team operational policy and hardening.
- Durable goal ledger beyond the first demonstrator's minimal objective/story shape.
- Research missions and evaluator loops.
- Repository wiki.
- Broader hook policy library.
- Extension/package manifests beyond first-party SSOT verification.
- Browser/media/live provider workflow signoff.

### Explicitly deferred

Deferred unless the demonstrator exposes a blocking need:

- Loading arbitrary third-party executable plugins.
- Hidden infinite loops or implicit background execution without explicit workflow state.
- Replacing Harness event logs with `.omx/` style file authorities.
- A new public simulation runtime mode.
- Full OAuth MCP.
- Full research mission framework.
- Worktree/tmux team polish.
- Browser/media live interaction as a foundation requirement.
- Adding new external dependencies unless an existing repo tool cannot satisfy the requirement.
- Claiming full OMO/OMX/Senpi compatibility.

## First exit gate: deterministic goal-loop demonstrator

Phase 1 is not complete until a deterministic fixture proves one goal-loop run through the actual coordinator path. The demonstrator is the forcing function that prevents the slice from becoming scaffolding.

Minimum choreography:

1. Start a workflow run from `/workflow run` or equivalent CLI command against a fixture workspace.
2. Create a context snapshot or lightweight intake artifact.
3. Make a plan-or-direct decision.
4. Start a bounded work loop using existing continuation semantics.
5. Schedule coordinator-owned task/tool work or a deterministic no-op fixture tool through the normal tool path.
6. Resolve at least one permission or simulated operator decision through the coordinator path.
7. Classify evidence by acceptance criterion.
8. Produce a terminal outcome: finished, blocked, cancelled, or failed.
9. Build the Run Dossier projection and optional export artifact.
10. Show `workflow status --json`, doctor workflow checks, and replay inspection from projected state.
11. Prove one invalid/conflicting transition is rejected and projected.

Required negative cases:

- Permission denied after workflow start.
- Missing evidence blocks signoff.
- Transition denied for conflicting execution ownership.
- Iteration limit reached or bounded continuation stopped.
- User interlude/question state prevents false completion.
- Child task cancellation or late result does not mutate terminal workflow state incorrectly.
- Artifact write failure produces blocked or failed state with evidence.

The demonstrator must use the real coordinator, command registry, permission gates, event append path, projections, and artifact redaction. It may use mock provider fixtures, scripted agent outputs, fake/no-op side-effect adapters, and disposable fixture workspaces. It must not be a replay of canned events only.

## Execution surfaces: replay, simulation, and live

| Surface | What it proves | What it must not do |
| --- | --- | --- |
| Replay | Pure projection from existing events and referenced artifacts. | Append events, run validators, scan live workspace, execute tools, call providers, launch terminal/browser/MCP/team workers. |
| Simulation | Coordinator choreography with deterministic fakes: command parsing, permission decisions, task/tool events, evidence mapping, interruption, restart, and projection. | Claim real provider/tool correctness, require live env vars, create a public runtime mode before the internal testkit path proves useful. |
| Live | Real provider, terminal, browser, network, or native visual behavior under env-gated signoff lanes. | Replace deterministic proof, run without preflight, or become required for unrelated core/projection changes. |

Simulation is a `harness-testkit` capability first. A future public `harness workflow simulate` command may be considered only after the testkit path proves useful and has a clear operator contract.

## Run Dossier projection

The Run Dossier is the user-facing closeout view for a workflow run. It is a replay-derived projection with optional markdown/JSON export artifact. It is not authoritative workflow state.

The dossier should contain:

- workflow id, command, lane, phase, and terminal outcome;
- objective and snapshot refs;
- plan or plan-lite decision refs;
- active team/task/continuation refs when present;
- operator decisions and their event ids;
- evidence by acceptance criterion and category;
- blocker/failure/user-interlude state;
- simulation versus live differences when both lanes were used;
- redline risks and waived evidence with reasons;
- signoff state and export provenance.

Rules:

- Events plus referenced artifacts remain the source of truth.
- Dossier exports cite event ids, artifact refs, digests, and capped summaries.
- Agents must not edit a dossier file as the workflow authority.
- Re-importing or approving a dossier file is out of scope unless a future command validates it through coordinator-owned events.

## Naming and public surface

Harness should keep Harness-native names in config and docs while allowing familiar workflow aliases.

Recommended first-slice commands:

| Harness command | Compatibility aliases | Purpose |
| --- | --- | --- |
| `/workflow run` | `/goal-loop`, `$ralph`, `$ultragoal` when aliases are enabled | Start the goal-loop demonstrator or production workflow run. |
| `/workflow status` | `/workflow-status`, `/hud`, `$hud` | Projection-only workflow state and evidence summary. |
| `/workflow signoff` | none initially | Approve or fail signoff using evidence projection. |
| `/workflow cancel` | `/workflow-cancel`, `/cancel` | Stop active workflow state through coordinator validation. |
| `/interview` | `/deep-interview`, `$deep-interview` | Optional intent-first snapshot creation before run. |
| `/plan-consensus` | `/ralplan`, `$ralplan` | Follow-up consensus planning workflow after the demonstrator. |
| `/team` | `$team` | Follow-up durable multi-agent team workflow. |
| `/research-loop` | `/autoresearch`, `$autoresearch` | Follow-up mission plus evaluator workflow. |
| `/wiki` | `$wiki` | Follow-up repository markdown knowledge operations. |

The TUI slash command list should show Harness names first and aliases second. Doctor should report aliases separately from canonical commands. Unknown or disabled aliases must fail with actionable diagnostics.

## Operator decisions

First-slice decisions:

- `approve-live`: operator approves crossing from simulated choreography to live side effects.
- `redirect`: operator changes objective, plan, owner, or next step.
- `request-evidence`: operator blocks signoff until a missing evidence category is supplied or waived.
- `abort` / `signoff-failed`: operator terminates the workflow unsuccessfully with reason.
- `signoff-approved`: operator accepts the evidence projection.

Deferred unless existing session controls already satisfy them:

- pause;
- take over;
- fork plan;
- split run.

Operator decisions append durable events when they change workflow state or write artifacts. Status, HUD, doctor, dossier rendering, replay inspection, and wiki read/query/list are projection-only by default.

## Rust-native architecture boundaries

The workflow layer should be Rust-native, but that does not mean everything belongs in `harness-core`.

| Crate / area | Responsibility |
| --- | --- |
| `harness-core` | Workflow domain types, lifecycle policy, transition validation, evidence classification, projection models, redaction/capping contracts, additive event metadata. |
| `harness` | CLI commands, exports, doctor integration, config/schema/docs drift tests. |
| `harness-tools` | Thin model-visible tools that submit coordinator commands and return projected state. No low-level event mutation tools. |
| `harness-tui` | View models, status widgets/dialogs, operator decision UI, presentation only. |
| `harness-testkit` | Deterministic simulator, fixture workspaces, mock-provider/no-op-tool scenarios, PTY/live/native signoff helpers. |
| `harness-providers` | Provider transport, fixture streams, error classification, live-provider signoff support. |

No workflow implementation may introduce a second scheduler, a mutable `.omx`-style state authority, direct model-edited workflow state, or replay-time side effects.

## Codex/Rust architecture implications

Codex itself is useful as architecture inspiration because it keeps a Rust CLI surface, core business-logic boundary, execution/headless path, and TUI presentation separated. Harness should adapt that shape rather than copy plugin behavior directly:

- keep durable workflow state, policies, and projections in `harness-core`;
- keep command/subcommand wiring and export behavior in `harness`;
- keep model-visible workflow tools as thin `harness-tools` coordinator-command wrappers;
- keep provider transport and fallback behavior in `harness-providers`;
- keep presentation in `harness-tui`;
- keep deterministic simulation and live signoff orchestration in `harness-testkit`;
- keep sandbox/approval inspiration as explicit permission policy, not as plugin-owned side effects.

The useful pattern to borrow is layered configuration plus explicit approval/sandbox policy and additive hooks. The pattern to avoid is executable plugin code owning workflow state transitions, permission boundaries, or replay semantics.

## Projection and write boundary table

| Surface | Default behavior | May append/write when |
| --- | --- | --- |
| `workflow status`, HUD/status panel | Projection-only | Never for passive reads. |
| Doctor workflow checks | Projection/config inspection only, no network by default | Only an explicit future remediation/apply command writes. |
| Run Dossier view | Projection-only | Explicit export writes a derived artifact. |
| Replay/session inspection | Projection-only | Never. |
| Wiki read/query/list | Projection-only live command over repository files | Page add/delete/refresh writes through edit/workflow command and records refs. |
| Snapshot/plan/goal/mission writes | Coordinator-owned command and artifact write | When command validates schema/redaction and records artifact refs. |
| Workflow cancel/complete/signoff | Coordinator-owned state transition | Always records operator decision/outcome. |
| Simulator | Appends events in disposable fixture run | Only inside deterministic testkit scenario. |
| Live lane | Appends real run events | Only after permission/operator approval and env-gated preflight. |

## Architecture overview

The slice should add a workflow subsystem to `harness-core`, exposed through CLI, TUI, and native tools.

```text
User prompt / slash command / tool call
  -> command registry parses canonical workflow intent
  -> coordinator validates transition, idempotency, conflicts, and permissions
  -> workflow lifecycle or operator-decision event appended when needed
  -> optional artifact written through coordinator artifact path
  -> agent catalog resolves mode-specific profile/tools/skills
  -> task/team/provider/tool execution continues through existing scheduler
  -> projections expose status, dossier, doctor, replay, sessions, and CLI JSON
```

Recommended modules:

- `harness-core::workflow`: workflow ids, lifecycle, transition policy, conflict ownership, failure semantics, projection model, and validator types.
- `harness-core::workflow_evidence`: evidence categories, acceptance mapping, signoff policy, and redacted evidence refs.
- `harness-core::context_snapshot`: context snapshot artifact schema, redaction, and projection references.
- `harness-core::run_dossier`: replay-derived dossier projection and export schema.
- `harness-core::goal_ledger`: follow-up durable multi-goal ledger projection and checkpoint validator.
- `harness-core::mission`: follow-up mission/evaluator contract types for research loops.
- `harness-core::wiki`: follow-up markdown wiki metadata, index projection, lint/query contracts.
- `harness-core::extension_manifest`: first-party declarative SSOT for commands, aliases, assets, evidence categories, and doctor checks; broader extension runtime is deferred.
- `harness-tools::workflow_ops`: model-visible workflow tools where needed, all thin wrappers over coordinator commands.
- `harness-tui::workflow`: status/sidebar/dialog view models and operator decision UI.
- `harness::workflow_cli`: CLI subcommands for run, status, signoff, cancel, snapshot, dossier export, doctor integration, and later goal/mission/wiki surfaces.

Do not move scheduling or tool execution into these modules. They define state and validation. The coordinator remains the executor.

## Event model additions

Prefer optional metadata on existing events when the existing event is already the semantic boundary. Add new event variants only where a durable decision, lifecycle, transition, or independently validated artifact boundary cannot be reconstructed from existing events.

Minimal proposed event boundaries:

- `WorkflowStarted`
  - workflow id, source command, scope, initial phase, lane, objective summary, optional snapshot ref.
- `WorkflowUpdated`
  - phase, lane, lifecycle outcome, iteration, warnings, active owner refs, evidence counters.
- `WorkflowCompleted`
  - terminal outcome, signoff state, evidence refs, dossier export ref if written.
- `WorkflowTransitionDenied`
  - active owner, requested transition, reason, operator-safe next action.
- `WorkflowOperatorDecision`
  - decision kind, actor, reason, affected workflow id, refs.
- `WorkflowEvidenceRecorded`
  - only if `ToolCallFinished`, `TaskCompleted`, or `ArtifactWritten` metadata cannot represent the evidence boundary cleanly.

Remove/defer:

- `WorkflowStatusObserved`; passive status reads remain projection-only.
- durable HUD/status/read events;
- standalone per-feature status events;
- support-mode events for setup, doctor, or wiki reads.

Existing events should gain serde-defaulted optional workflow metadata when needed:

- `TaskScheduled`: workflow id, lane, role, owner story/task ref.
- `TaskCompleted`: workflow id, lane, evidence classification, acceptance refs, resolved role metadata.
- `ToolCallFinished`: workflow id, lane, evidence category, artifact refs, side-effect class.
- `PermissionRequested` / `PermissionResolved`: workflow id, operator decision correlation.
- `ArtifactWritten`: workflow id, artifact kind, digest, redaction/cap metadata.
- `ProviderRequestStarted` / `ProviderRequestFinished`: workflow id, lane, fallback/evidence metadata when relevant.
- `BackgroundTaskNotification`: workflow id when spawned by a workflow.
- `CompactionWritten`: workflow restoration summary and dossier-relevant operational memory.
- `AssistantMessageFinished`: terminal lifecycle metadata when the workflow uses it.
- `Team*`: workflow id, claim/blocker/synthesis/evidence refs where applicable.
- `Continuation*`: workflow id, lane, iteration, stop reason.

Replay must derive active workflow state, dossier state, evidence state, and signoff state from events only.

Every new event or metadata field requires architecture docs and event-doc drift test updates.

## Workflow state model

Adapt the OMX state model to Harness events, but keep support/read surfaces out of durable workflow mode state.

### Modes

Core first-slice modes:

- `workflow_run`
- `interview`
- `work_loop`
- `signoff`

Follow-up execution modes:

- `plan_consensus`
- `team`
- `goal`
- `research_loop`

Projection-only/support surfaces:

- `workflow_status`
- `hud`
- `setup_check`
- `doctor`
- `dossier_view`
- `wiki_read`

### Lanes

Start with two lane labels:

- `simulated`
- `live`

These are metadata/projection concepts, not a general lane framework. Add more lanes only after the first demonstrator proves a need.

### Lifecycle outcomes

Use these canonical outcomes in events and projections:

- `running`
- `finished`
- `blocked`
- `failed`
- `user_interlude`
- `asked_user_question`
- `cancelled_admin`

`cancelled_admin` is for explicit operator cleanup. Do not expose it as a normal successful user-facing outcome.

### Transition rules

Allowed with source auto-complete:

- `interview -> workflow_run`
- `workflow_run -> work_loop`
- `workflow_run -> signoff` only when evidence requirements are satisfied or explicitly waived by operator decision.
- `workflow_run -> plan_consensus` when configured or when a request is vague.
- `plan_consensus -> work_loop`
- `plan_consensus -> team`
- `plan_consensus -> goal`
- `plan_consensus -> research_loop` when the plan is evaluator-driven.
- `goal -> team` as an explicit execution lane under a goal story.
- `team -> goal` only as a checkpoint handoff, not hidden goal mutation.

Allowed overlap:

- `goal + team`
- `goal + work_loop`
- `goal + research_loop`
- `team + work_loop` only when the work loop owns final verification or blocker recovery and the team remains terminal or explicitly delegated.
- projection-only/support surfaces with any mode.

Denied by default:

- execution-like to planning-like rollback without explicit user command.
- starting a second execution mode that would own the same artifact set.
- starting team mode while an existing active team has pending or in-progress tasks, unless the command targets a new team id.
- completing a goal without checkpoint evidence.
- completing research without a validator artifact.
- signoff without required evidence refs or an explicit operator waiver.

Denied transitions should append `WorkflowTransitionDenied` with a short reason and an operator-safe next action.

### Restart and idempotency

- Workflow ids are stable, harness-owned ids derived at command acceptance time, never from provider ids.
- Duplicate start commands with the same idempotency key return the existing active workflow projection.
- Duplicate commands without matching idempotency key are treated as conflicts when they would own the same artifacts, tasks, or live lane.
- Coordinator resume restores active workflow state from replay before accepting workflow commands.
- Resumed workflows preserve iteration, provider/tool-call counts, evidence refs, and stop bounds.
- Old event logs without workflow metadata remain replayable with default empty workflow projections.

### Concurrency and late results

- Conflicting workflow commands are serialized by the coordinator.
- Cancellation does not erase child task/tool history; late child results become late/cancelled evidence and cannot resurrect a completed/aborted workflow.
- Permission denial after workflow start maps to `blocked`, `failed`, or `user_interlude` according to the command's recovery policy.
- Artifact write failure blocks or fails the workflow before signoff.
- Provider fallback changing model mid-loop is metadata only; replay derives behavior from the event sequence.

### Overlap ownership rules

- `goal + team`: goal owns story completion and final evidence; team owns team checklist state, mailbox, member outputs, and shutdown proof. Team synthesis maps outputs to goal evidence refs.
- `goal + work_loop`: goal owns story status and final signoff; work loop owns iteration, continuation limits, and tool/task execution evidence.
- `team + work_loop`: team owns member coordination; work loop owns final verification or blocker recovery only if explicitly delegated.
- File claims are advisory coordination state. Edit permissions and workspace safety remain authoritative.
- A workflow cannot complete an artifact set owned by another active workflow without an explicit handoff event or operator decision.

## Workstream A: Context snapshot and intent intake

Purpose: every large workflow starts from a durable, prompt-safe, evidence-backed context packet.

Harness adaptation:

- Store snapshots as artifacts under the session artifact root, not as authoritative `.omx/context` files.
- Optionally export a readable copy under `.agent-harness/context/` only when the user asks or config enables project-visible planning artifacts.
- Snapshot state is event-linked by workflow metadata or a semantic artifact boundary.
- Snapshot content is redacted and capped. Large source excerpts become artifact references or digests.
- The first demonstrator may use a lightweight snapshot if the prompt is already concrete.

Snapshot schema:

```json
{
  "schema_version": 1,
  "snapshot_id": "uuid",
  "slug": "short-task-slug",
  "created_at": "iso8601",
  "source_command": "/interview",
  "task_statement": "...",
  "desired_outcome": "...",
  "probable_intent": "...",
  "known_facts": [
    { "source": "from-code", "summary": "...", "refs": ["path:line"] }
  ],
  "constraints": [],
  "non_goals": [],
  "decision_boundaries": [],
  "unknowns": [],
  "likely_touchpoints": [],
  "ambiguity": {
    "score": 0.42,
    "threshold": 0.2,
    "dimensions": {}
  },
  "handoff_ready": false
}
```

Acceptance criteria:

- `/interview "..."` or `/workflow run` creates or updates one snapshot artifact when intake is needed.
- Snapshot projection survives replay without reading live workspace files.
- Doctor reports whether active workflows have snapshots when snapshots are required.
- TUI/CLI status shows active snapshot slug and ambiguity score when present.
- Tests cover creation, update, redaction, artifact digest, replay projection, and oversized input summary gating.

## Workstream B: Deep interview workflow

Purpose: convert vague requests into execution-ready specs before heavy planning or execution.

Harness adaptation:

- Use the existing `question` tool for one focused round at a time.
- Use `explore`/read/search before asking the user for discoverable code facts.
- Store rounds in workflow state events and snapshot artifacts.
- Do not implement `.omx state` commands. Use coordinator-owned events.
- The first demonstrator can bypass full interview when the fixture goal is concrete.

Behavior:

1. Parse `/interview`, `/deep-interview`, or `$deep-interview`.
2. Derive a slug.
3. Gather brownfield facts with read/search/explore where appropriate.
4. Create initial snapshot.
5. Score ambiguity across intent, outcome, scope, constraints, success criteria, context, non-goals, and decision boundaries.
6. Ask exactly one highest-leverage question per round.
7. Update snapshot and workflow state after each answer.
8. Finish when score is below threshold and mandatory gates are explicit.
9. Offer next handoffs: workflow run, plan consensus, research loop, goal, work loop, or team.

Acceptance criteria:

- Interview cannot mark `handoff_ready=true` while non-goals or decision boundaries are empty for broad tasks.
- Discoverable code facts are recorded as `from-code` without consuming a user question round.
- User answers are recorded as `from-user`.
- A direct, concrete request can bypass interview and use a lighter snapshot.
- Replay shows the interview transcript and final snapshot.

## Workstream C: Workflow run and work-loop spine

Purpose: make `/workflow run` the Harness-native goal-loop entrypoint that composes existing continuation, task, tool, and evidence primitives.

Harness adaptation:

- Reuse `crates/harness-core/src/continuation.rs` rather than adding hidden stop hooks or another loop scheduler.
- Continuation reminders must be coordinator-owned and event-visible.
- Completion requires prompt-to-artifact/evidence audit, not only green tests.
- If code changed, verification must include changed-file diagnostics and the narrowest test/build lane that proves behavior.
- Work-loop state references existing persistent tasks or team tasks; it does not create a third task system.

Behavior:

1. Start from a snapshot, plan-lite decision, or approved plan.
2. Maintain workflow id, lane, iteration count, max iterations, and stop condition.
3. Use todos or persistent tasks as progress substrate when the work outlives one agent turn.
4. Delegate independent research/implementation/review lanes through `task` when configured.
5. Run verification through configured lanes.
6. Classify evidence by acceptance criterion.
7. Complete only with evidence refs or explicit operator waiver.

Acceptance criteria:

- Work loop cannot complete while any workflow-owned persistent task is pending or in progress unless explicitly waived.
- Work loop cannot complete without evidence refs mapped to each required acceptance criterion.
- Continuation stops on max iterations, explicit cancel, user question, hard blocker, or done marker.
- Replay shows why a continuation was scheduled and why it stopped.
- Doctor warns on stale active work loops.
- Deterministic demonstrator proves at least one successful and one blocked work-loop outcome.

## Workstream D: Evidence classifier and signoff gates

Purpose: normalize what "done" means across workflow modes.

Evidence categories:

- `diagnostics`: LSP diagnostics or type checking.
- `format`: formatter or formatting check.
- `build`: cargo/npm/etc build.
- `test`: targeted or lane tests.
- `manual_qa`: artifact driven through the user-facing surface.
- `review`: architect/critic/code review verdict.
- `validator`: research/evaluator result.
- `artifact`: file/document/result exists and matches schema.
- `user_answer`: explicit user decision.
- `operator_decision`: approve-live, redirect, request-evidence, abort/signoff-failed, signoff-approved, or waiver.

Quality gate policy:

- Planning completion requires plan artifact and review verdict when consensus planning is used.
- Work loop completion requires prompt-to-artifact checklist and verification evidence.
- Team completion requires no pending/in-progress tasks and verification lane evidence, or explicit abort reason.
- Goal completion requires all stories complete plus final gate evidence.
- Research completion requires validator artifact.
- Wiki completion requires lint pass for changed pages.
- First-slice signoff requires evidence refs or explicit waiver for each required acceptance criterion.

Acceptance criteria:

- Workflow completion events include evidence refs by category.
- Missing required evidence blocks completion or marks `blocked` with reason.
- Waived evidence requires an operator decision event with reason.
- Final messages and dossier exports can be generated from evidence projection.

## Workstream E: Team mode hardening

Purpose: lift Team Mode from MVP to a durable operator workflow after the goal-loop spine is proven.

Current Harness baseline:

- `README.md`, `docs/architecture.md`, and `docs/parity-ledger.json` show event-sourced team tools exist.
- Remaining gaps include worktrees, file claims, tmux visualization, durable mailbox artifacts, and active runtime diagnostics.

Harness adaptation:

- Team state remains event-sourced.
- Team ritual is policy/projection over existing team events/tools, not a mandatory coordinator ceremony.
- Team tasks must not become scheduler tasks.
- Mailbox/task payloads that exceed event caps become artifacts referenced by team events.
- Tmux visualization is optional and dependency-gated.
- Worktrees must be explicit, per-member, and never created by replay.
- File claims are advisory coordination state, not filesystem locks that bypass permissions.

Required additions after the first demonstrator:

- Team spec parser with canonical validation and diagnostics.
- Team run projection with active/pending/in-progress/completed/failed counts.
- Durable mailbox artifact refs for large messages.
- File claim events or metadata with owner, path, claim reason, and release status.
- Optional per-member worktree creation through coordinator command.
- Tmux pane metadata with dependency errors and cleanup status.
- Team status TUI panel.
- Team shutdown proof with no pending/in-progress tasks or explicit abort reason.
- Lead synthesis artifact that maps member outputs to evidence refs.

Acceptance criteria:

- `team_create` can create a team from inline spec and persisted declaration.
- `team_task_*` tools expose ready/unblocked task projection.
- `team_send_message` records delivery, payload cap, artifact refs, and unread count.
- Blockers surface as workflow operator decisions, not buried messages.
- Optional worktree mode creates and records worktree paths only after permission approval.
- Optional tmux mode records pane ids and dependency errors.
- Doctor reports active teams, stale teams, missing tmux, dirty worktrees, and unread mailbox pressure.
- Replay renders teams without launching workers, creating worktrees, or attaching tmux.

## Workstream F: Consensus planning

Purpose: make `/plan-consensus` the Harness-native `$ralplan` after the first workflow spine exists.

Harness adaptation:

- Use existing Plan profile and specialist subagents where available.
- Planner, architect, and critic are role lanes resolved through `AgentCatalog`.
- Consensus loops schedule child tasks through normal `task`; no direct subagent bypass.
- Plan artifacts live under session artifacts by default, optionally exportable to `.agent-harness/plans/`.
- Plan completion produces evidence refs that the Run Dossier projection can cite.

Plan artifact sections:

- Task and snapshot reference.
- Principles.
- Decision drivers.
- Viable options with pros and cons.
- Chosen option and rejected alternatives.
- ADR.
- Work breakdown.
- Risk and pre-mortem for deliberate mode.
- Test and manual QA plan.
- Agent/team staffing guidance.
- Handoff options.
- Acceptance criteria.

Acceptance criteria:

- `/plan-consensus` writes a plan artifact and workflow evidence ref.
- Architect and critic reviews happen in the configured order when both are required.
- Critic can force iteration up to a bounded limit.
- Final plan includes an ADR and execution handoff choices.
- Vague direct `/workflow run`, `/work-loop`, or `/team` requests can be redirected to planning unless explicitly forced.
- TUI and replay expose current plan status and review verdicts.

## Workstream G: Durable goal ledger

Purpose: adapt `$ultragoal` into Harness goal tracking after the first demonstrator proves the minimal objective/story/evidence shape.

Harness adaptation:

- Add Harness-native goals as event-derived ledger state only when minimal workflow state is insufficient.
- Goals can group stories. Stories can be assigned to work loop, team, or research loop.
- Checkpoints require evidence refs.
- Final completion requires a quality gate.

Goal schema:

```json
{
  "schema_version": 1,
  "goal_id": "uuid",
  "objective": "...",
  "status": "active|complete|blocked|failed",
  "stories": [
    {
      "story_id": "G001",
      "objective": "...",
      "status": "pending|active|complete|blocked|failed",
      "owner_workflow_id": "uuid",
      "acceptance": [],
      "evidence_refs": []
    }
  ]
}
```

Acceptance criteria:

- `/workflow goal create` writes a goal ledger event or artifact ref after coordinator validation.
- `/workflow goal status` is replay-derived.
- Work loop/team/research workflows can checkpoint a story with evidence.
- Intermediate story completion does not complete the aggregate goal.
- Final goal completion requires verification and review evidence.
- Replay and session export show the goal timeline.

## Workstream H: Research missions and evaluator loops

Purpose: adapt OMX autoresearch missions into a Harness-native research loop after evidence and workflow state are stable.

Harness adaptation:

- Missions are artifacts with `mission.md`, `sandbox.md`, and `result.json` equivalents stored under the session artifact root or project-visible `.agent-harness/missions/` when configured.
- Validator commands run only through permissioned tools during live execution, never replay.
- Prompt+architect validation uses normal child tasks and stores verdict artifacts.

Validation modes:

- `mission_validator_script`: a command must produce a structured passing result.
- `prompt_architect_artifact`: an architect or critic review must approve the output artifact.

Acceptance criteria:

- `/research-loop init` creates mission and sandbox artifacts.
- `/research-loop run` records candidate, iteration ledger, and validator result refs.
- Research loop does not complete without the selected validation artifact.
- Failed validators schedule bounded continuation or mark blocked with evidence.
- Replay can show every iteration decision without rerunning validators.

## Workstream I: Repository wiki

Purpose: add a markdown-first project knowledge layer similar to OMX Wiki, adapted to Harness, after the first workflow spine and artifact rules are stable.

Harness adaptation:

- Canonical visible storage: `.agent-harness/wiki/` or `harness_wiki/`. Prefer `.agent-harness/wiki/` for consistency with existing runtime assets unless user-facing review wants a top-level directory.
- Event log records wiki page changes and digests only for write operations.
- Query is keyword/tag/category search first. No embeddings required.
- Session-end capture is opt-in and writes summarized, redacted pages.
- Read/query/list are projection or live read surfaces, not durable workflow mode state.

Wiki commands:

- `/wiki add`
- `/wiki query`
- `/wiki read`
- `/wiki list`
- `/wiki lint`
- `/wiki refresh`
- `/wiki delete`

Acceptance criteria:

- Wiki pages are markdown and reviewable in git.
- Query can search title, tags, category, and body text.
- `explore` and interview can prefer wiki hits before broad repository search when enabled.
- Lint catches missing title, duplicate slug, broken wiki links, and oversized pages.
- Replay uses event/artifact refs and does not scan the live wiki unless explicitly running a live command.

## Workstream J: Setup, doctor, and SSOT verification

Purpose: make the workflow layer discoverable and diagnosable without creating a broad plugin runtime.

Harness adaptation:

- No `omx setup` clone. Add Harness setup validation and optional bootstrap commands.
- Do not write user config destructively. Provide `--check`, `--print`, and explicit `--apply` modes.
- Treat first-party workflow commands, aliases, prompts, evidence categories, doctor checks, and docs links as a small single source of truth early in the slice.
- Defer third-party executable manifests and plugin runtime.

Required first-slice surfaces:

- integrated `harness doctor` workflow section.
- `harness workflow init --check` for project bootstrap diagnostics.
- `harness workflow init --apply` only for safe generated files under `.agent-harness/`, and only after explicit approval.
- Manifest/registry verification tests for first-party commands, aliases, evidence categories, prompts, and doctor/docs links.

Doctor checks:

- workflow commands registered.
- aliases present or explicitly disabled.
- active workflow state consistency.
- stale continuation loops.
- team dependency status when team workflow is enabled.
- tmux availability when terminal/team visualization is configured.
- wiki lint status when wiki is enabled.
- mission validator artifact state when missions are enabled.
- model/profile fallback diagnostics for workflow agents.
- MCP skill lifecycle status, without launching external servers.

Acceptance criteria:

- Doctor distinguishes install/config readiness from live provider readiness.
- Doctor output includes JSON and human formats.
- Generated first-party manifest/registry has drift tests in the foundation phase.
- `README.md`, `docs/config.md`, and examples link to the workflow slice when implemented.

## Workstream K: Hook policy implementations

Purpose: put the existing hook seam to work for real workflow behavior after the demonstrator proves the workflow spine.

Priority hook policies:

- prompt keyword and alias detector for workflow commands.
- pre-execution gate that redirects vague heavy execution requests to interview or consensus planning.
- context injection for active snapshot, goal/story, team, and wiki summary.
- tool result classification for evidence gathering.
- post-tool recovery hints for command-not-found, permission denied, missing path, and MCP transport failure.
- continuation policy for active work loop, research loop, and goal story.
- compaction preservation of active workflow state.
- final handoff warning when required evidence is missing.

Acceptance criteria:

- Hooks are typed and coordinator-mediated.
- Hook outputs are capped, redacted, and event-visible when they affect state.
- Replay sees hook decisions but never reruns hooks.
- Tests cover allow, deny, context injection, continuation, and compaction preservation.

## Config additions

Add under a harness-native namespace, not as broad compatibility keys:

```jsonc
{
  "runtime": {
    "workflow": {
      "enabled": true,
      "aliases": true,
      "project_artifacts": false,
      "run": {
        "default_lane": "simulated",
        "require_dossier": true,
        "require_evidence": true
      },
      "interview": {
        "default_profile": "standard",
        "threshold": 0.2,
        "max_rounds": 12
      },
      "plan_consensus": {
        "max_iterations": 5,
        "deliberate_triggers": ["auth", "security", "migration", "public api", "pii"]
      },
      "work_loop": {
        "max_iterations": 10,
        "require_manual_qa": true
      },
      "team": {
        "max_members": 8,
        "max_parallel_members": 4,
        "tmux_visualization": false,
        "worktrees": false
      },
      "goal": {
        "require_final_quality_gate": true
      },
      "research_loop": {
        "max_iterations": 10
      },
      "wiki": {
        "enabled": false,
        "root": ".agent-harness/wiki",
        "auto_capture": false
      }
    }
  }
}
```

Compatibility imports may recognize OMX/OMO names, but canonical docs and schemas should use `runtime.workflow`.

## TUI requirements

First slice:

- Compact workflow status footer/sidebar block.
- Workflow detail dialog from `/workflow status`.
- Operator decision UI for approve-live, redirect, request-evidence, abort/signoff-failed, and signoff-approved.
- Run Dossier projection view or export notice.
- Warnings when a workflow is active and the user tries to start a conflicting one.

Follow-up:

- Interview question modal reusing the existing question UI.
- Plan artifact preview with reviewer verdicts.
- Team status panel with member/task/mailbox counts.
- Goal story list.
- Research mission status and validator result.
- Wiki query/read overlay or command output view.

TUI implementation must keep presentation in `harness-tui`; core owns state only.

## CLI requirements

Recommended subcommands:

```bash
harness workflow run [--json]
harness workflow status [--json]
harness workflow signoff --approve|--fail|--request-evidence [--json]
harness workflow cancel --mode <mode|all> [--json]
harness workflow dossier export [--format json|markdown] [--json]
harness workflow snapshot list|read|export [--json]
harness workflow goal create|status|checkpoint|list|read [--json]
harness workflow mission init|status|run|read [--json]
harness workflow wiki add|query|read|list|lint|refresh|delete [--json]
harness workflow init --check|--apply [--json]
```

Shortcut slash commands in the TUI can call these through coordinator command handlers rather than shelling out.

## Tool requirements

Model-visible tools should be thin, stable, and permissioned:

- `workflow_run`
- `workflow_status`
- `workflow_signoff`
- `workflow_cancel`
- `workflow_snapshot_write`
- `workflow_dossier_export`
- `workflow_goal_create`
- `workflow_goal_checkpoint`
- `workflow_mission_create`
- `workflow_mission_checkpoint`
- `wiki_add`
- `wiki_query`
- `wiki_read`
- `wiki_lint`

Do not expose low-level event mutation tools. Tools submit coordinator commands and receive projected state.

## Existing simulation and signoff substrate

The first simulator should compose existing Harness verification pieces rather than inventing a parallel runner:

- `scripts/test-lanes.sh` and `docs/testing.md` define canonical fast, integration, PTY, browser, live, native visual, and stress lanes with artifact roots and per-stage evidence.
- `crates/harness-testkit/tests/pty_e2e.rs` and related PTY helpers already launch Harness/TUI, type prompts, wait on screen markers, and capture manifest-backed visual evidence.
- `crates/harness-testkit/tests/live_proxy_e2e.rs` and `crates/harness-testkit/tests/README.live-proxy.md` already define env-gated live provider prompt/TUI flows and run summaries.
- `crates/harness-testkit/tests/native_visual_e2e.rs` provides optional local screenshot provenance; it is not a portable correctness oracle.
- `crates/harness-providers/src/mock.rs` and scenario fixtures provide deterministic provider streams suitable for workflow simulation.
- `crates/harness-tools/src/terminal_session.rs` provides tmux-backed terminal tools that can be hardening targets, not a foundation dependency.

The simulator's job is to prove workflow choreography through real coordinator paths with deterministic fakes. Existing live, browser, native, and PTY lanes remain the matching-surface signoff layers when those surfaces change.

## Testing plan

Add focused tests before broad lanes:

- `harness-core` workflow transition, idempotency, overlap, and failure-semantics tests.
- replay projection tests for every new event and metadata field.
- old-log replay fixtures without workflow metadata.
- context snapshot redaction and digest tests.
- command registry tests for canonical commands and aliases.
- first-party workflow SSOT/drift tests for commands, aliases, evidence categories, doctor/docs links, and prompts.
- config schema and docs drift tests for `runtime.workflow`.
- doctor JSON/human output tests.
- Run Dossier projection/export tests.
- deterministic goal-loop simulator scenario in `harness-testkit`.
- team mode worktree/mailbox/file-claim tests with no actual worker launch during replay when team hardening begins.
- wiki parser/query/lint tests with fixture pages when wiki begins.
- mission validator tests with deterministic fake validator command output when missions begin.
- goal ledger checkpoint tests when goal ledger begins.
- TUI projection tests for workflow status view models.

Lane mapping:

- Docs-only changes: readback plus `python3 scripts/check-forbidden-branding.py` if public docs mention product names.
- Core workflow changes: `cargo fmt --all -- --check`, `cargo check --workspace`, targeted `harness-core` workflow tests, and `scripts/test-lanes.sh integration` when event/config/replay contracts change.
- Deterministic workflow simulator: fixture workspace, mock provider/scripted agent outputs, no-op tool adapter, status JSON, doctor JSON, replay equivalence, dossier projection/export.
- TUI workflow status changes: `cargo test -p harness-tui` and `scripts/test-lanes.sh signoff-pty` when rendering changes are meaningful.
- Team/terminal visualization changes: deterministic unit tests plus `scripts/test-lanes.sh signoff-pty` and dependency-gated tmux checks.
- Browser/media workflow changes: `scripts/test-lanes.sh signoff-browser` only when browser/media surface changes.
- Live provider behavior: `scripts/test-lanes.sh signoff-live` only with explicit env-gated live setup.

Tiered evidence rules:

| Touched surface | Required evidence |
| --- | --- |
| Docs only | readback, formatting/branding scan when applicable. |
| Event/core projection | unit tests, replay equivalence, event docs drift, old-log fixture. |
| Config/schema | config docs/schema drift, example config validate. |
| Model-visible tool | schema, permission, artifact, failure, docs tests. |
| CLI JSON | stable JSON assertion/snapshot, human output smoke. |
| TUI rendering | view-model tests and PTY signoff when layout changes. |
| Simulation | deterministic goal-loop scenario artifacts. |
| Live/provider/browser/native | env-gated signoff only when that surface changes. |

Universal gates:

- replay purity;
- redaction/capping for durable data;
- deterministic failure semantics;
- no alternate scheduler or mutable state authority.

## Delivery order

The slice is large in ambition, but delivery must be staged by proof gates.

### Phase 0: Contract review and scope trim

Deliver:

- Reuse/Harden/Defer/Replace review against `docs/parity-ledger.json`.
- Contract review for reused coordinator, continuation, task, team, session, provider, doctor, and replay surfaces.
- Canonical workflow command and alias inventory.
- Evidence category inventory.
- First-party command/alias/evidence/doctor/docs SSOT drift guard.
- Explicit list of deferred surfaces.

Exit criteria: implementer can state what is reused, what is wrapped with workflow metadata, what is hardened later, and what is deferred.

### Phase 1: Minimal workflow state and projection

Deliver:

- `workflow` core module.
- minimal workflow lifecycle events or metadata.
- transition policy.
- stable workflow ids and idempotency behavior.
- restart/resume projection.
- projection-only CLI/TUI status.
- config schema for `runtime.workflow.enabled` and aliases.
- doctor workflow section.

Exit criteria: workflow state is replay-derived, old logs still replay, duplicate/conflicting commands are deterministic, and status reads do not append events.

### Phase 2: Deterministic simulator path

Deliver:

- `harness-testkit` goal-loop fixture workspace.
- mock provider or scripted agent outputs.
- no-op/fake side-effect adapter where needed.
- permission decision fixture.
- one coordinator-owned task/tool path.
- invalid transition negative case.
- restart/replay equivalence check.

Exit criteria: the simulator appends real events through coordinator commands and proves choreography without live provider, terminal, browser, or network dependencies.

### Phase 3: Evidence and Run Dossier

Deliver:

- evidence categories and acceptance mapping.
- signoff blocking policy.
- operator decisions for approve-live, redirect, request-evidence, abort/signoff-failed, and signoff-approved.
- Run Dossier projection.
- optional dossier export artifact.
- blocked/failed/user-interlude/cancel/late-result/artifact-failure semantics.

Exit criteria: completion cannot claim success without evidence refs or explicit operator waiver, and dossier export is reproducible from replay.

### Phase 4: Inspection surfaces and first exit gate

Deliver:

- `harness workflow status --json`.
- `harness workflow signoff`.
- `harness workflow dossier export`.
- doctor workflow checks.
- replay inspection of workflow state and dossier.
- TUI status view model for active workflow state.

Exit criteria: deterministic goal-loop demonstrator passes from intake through evidence-backed completion, status/doctor/replay inspection, dossier projection/export, and invalid transition negative case.

### Phase 5: Work-loop and continuation hardening

Deliver:

- active workflow metadata on continuation loops.
- prompt-to-artifact completion audit.
- final quality gate projection.
- stale active workflow doctor checks.

Why here: it upgrades existing continuation work after the demonstrator proves the evidence spine.

### Phase 6: Team operational policy and hardening

Deliver:

- team claims/blockers/synthesis/shutdown proof as policy/projection over existing team events.
- mailbox artifact refs.
- file claims.
- worktree option.
- tmux metadata diagnostics.
- team status panel.

Why here: team hardening is broad and should not block the first proof.

### Phase 7: Consensus planning and goal ledger

Deliver:

- `/plan-consensus` and `/ralplan` alias.
- role lane routing through AgentCatalog.
- plan artifact and ADR.
- critic iteration loop.
- goal/story ledger.
- checkpoint evidence.
- final quality gate.

Why here: planning and goal structure compose with the workflow/evidence spine.

### Phase 8: Research missions and wiki

Deliver:

- mission/sandbox/result artifacts.
- validator modes.
- iteration ledger.
- prompt+architect validation.
- wiki markdown storage.
- add/read/list/query/lint.
- optional context injection and session capture.

Why here: these use the same artifact/evidence/replay rules with more specialized surfaces.

### Phase 9: Broader extension and hook policy expansion

Deliver:

- first-party workflow manifest expansion beyond the early SSOT guard.
- built-in hook policy library.
- broader skill/MCP/command/package metadata.
- doctor manifest diagnostics.

Why here: this prevents asset drift after core workflow behavior is proven, without prematurely building a plugin runtime.

## Open decisions

These need explicit design choices during implementation, but should not block writing the slice:

1. Wiki root: `.agent-harness/wiki/` vs `harness_wiki/`. Recommendation: `.agent-harness/wiki/` unless reviewers want top-level project knowledge to be more visible.
2. Project-visible artifacts: default off to avoid noisy repos. Add explicit export/apply commands.
3. Workflow command naming: keep Harness names canonical, aliases enabled by default.
4. Team worktrees: likely opt-in per team run, not global default.
5. Research validator commands: require explicit permission and record command/digest, not raw output beyond caps.
6. Extension manifests: first-party SSOT/drift guard early; third-party executable runtime deferred.
7. Simulator public surface: keep internal/testkit first; expose `harness workflow simulate` only after the internal lane proves useful.
8. Dossier export format: JSON for machine checks first, markdown as a human-readable export.

## Definition of done for the whole slice

The slice is complete only when:

- Workflow state is event-sourced and replay-safe.
- `/workflow run`, `/workflow status`, `/workflow signoff`, and `/workflow cancel` have canonical docs and alias behavior where aliases are enabled.
- Context snapshots, evidence artifacts, and Run Dossier exports have schemas and redaction rules.
- Doctor reports workflow readiness and stale/invalid workflow state.
- TUI shows active workflow status without treating status reads as events.
- Deterministic goal-loop simulator drives the real coordinator path through intake, plan-or-direct decision, work loop, evidence, signoff, dossier, replay, doctor/status inspection, and invalid transition.
- Completion gates block unsupported "done" claims for the demonstrator and later workflow modes.
- Reused last-slice surfaces have contract review evidence.
- Config schema, docs, README links, and examples are updated.
- Deterministic tests pass for core, tools, config, replay, and TUI projections touched by the slice.
- Manual QA has driven any changed user-facing surface through its matching lane: CLI for commands, PTY for TUI, env-gated live/browser/native only when those surfaces change.

## Why this is the right next slice

The previous parity work gave Harness many of the pieces: agents, categories, task/background output, continuation loops, team MVP, session tools, terminal tools, MCP basics, and doctor/config surfaces. The biggest missing product feeling is workflow coherence. OMX is valuable because it gives operators a path: clarify, plan, execute, team, persist, inspect, and prove. Senpi is valuable because it keeps this kind of power extension-shaped and configurable rather than bloating the core.

This slice brings those lessons into Harness without compromising the event-sourced runtime. It turns existing primitives into a coherent operator workflow and creates the state/evidence model needed for future compatibility work.
