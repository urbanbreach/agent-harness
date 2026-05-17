# OMX-style workflow closeout and policy slice specification

This document defines the next large workflow slice after `docs/omx-workflow-slice-spec.md`. The first workflow slice established the event-sourced workflow spine: lifecycle projection, context snapshots, Run Dossier export, deterministic simulation, workflow CLI/TUI surfaces, goal ledger, plan consensus, research mission, wiki, and doctor/drift checks. The next slice should make that spine trustworthy enough to use as the operator closeout path.

The goal is broad but not expansive for its own sake: make workflow completion real, policy-backed, inspectable, and deterministic while keeping the Harness tool and skill surface closer to OMX than OMO. Browser automation, Playwright-style runtime control, live browser sessions, and browser state are not product goals for this slice. Browser/native/live lanes remain optional verification environments only when those surfaces are intentionally changed.

## Product north star

The operator should be able to run a broad task through Harness and answer one question at the end: "Can this workflow close?" The answer must come from replay-derived evidence, not from a model's final sentence.

The intended path is:

```text
workflow run
  -> snapshot / plan / goal / task / team / mission evidence
  -> workflow status shows blockers and legal next actions
  -> dossier export explains what happened from events
  -> signoff enforces evidence, task, continuation, and waiver policy
  -> replay of the same event log yields the same terminal state
```

The slice should feel like an operating layer around existing Harness primitives. It should not add a second scheduler, a browser-control subsystem, a plugin runtime, or another state authority.

## Current baseline

Assume the previous slice delivered these foundations:

- `harness workflow run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/mission/wiki/init` CLI surfaces.
- Workflow lifecycle and evidence projections in `harness-core`.
- Redacted context snapshot artifacts.
- Run Dossier projection and export.
- Deterministic `harness-testkit` workflow simulator.
- Doctor checks for workflow registry, runtime config, simulator contract, snapshots, and stale loops.
- TUI slash intents and status/operator-sidebar rows for workflow state.
- Goal ledger, consensus planning, research mission, wiki, and team workflow metadata as first-party workflow domains.

Known weakness to fix: the public signoff surface must close an existing workflow run through the same policy gate as the deterministic simulator. A signoff audit run may still be useful as evidence, but it must not be the only terminal state when the operator intended to approve the original workflow.

## Non-negotiable boundaries

- Coordinator owns workflow transitions, event append, task scheduling, permission resolution, and tool execution re-entry.
- Replay remains side-effect free and never runs tools, providers, hooks, validators, browsers, terminals, MCP servers, team members, or wiki scans.
- Workflow state that affects closeout must be event-derived or stored as redacted artifacts referenced by events.
- Public config/schema/docs/tests must stay aligned for new workflow policy keys.
- Model-visible workflow tools, if any are added or changed, remain thin coordinator-command wrappers.
- Skills remain prompt/file/metadata assets. Skill loading must not become hidden executable workflow control.
- Browser automation is not a workflow product surface. Do not add a Playwright tool, browser session state, browser screenshots as required evidence, or browser MCP startup to the workflow path.
- Existing browser/media/native/live test lanes may remain documented as optional verification lanes; they must not become required for core workflow closeout.

## Slice scope

This is a broad hardening slice. Include every area needed to make closeout coherent across the existing workflow domains, but avoid adding new workflow domains.

Included:

- Evidence-gated signoff against an existing run/workflow.
- Typed workflow closeout policy.
- Operator decisions and waivers with reasoned audit trail.
- Readiness projection for evidence, tasks, continuations, artifacts, plans, goals, team state, missions, wiki lint, and dossier export.
- CLI/TUI surfaces that show blockers and legal next actions.
- Doctor diagnostics for invalid or stale closeout state.
- Deterministic end-to-end demonstrator using CLI/testkit only.
- Thin model-visible workflow tools only where they expose the same coordinator commands.
- Documentation and drift guards for the closeout policy contract.

Explicitly excluded:

- First-class Playwright, `agent-browser`, or `dev-browser` workflow integration.
- Browser session persistence or browser screenshots as mandatory workflow evidence.
- New third-party executable plugin runtime.
- Full OAuth MCP or executable skill MCP hardening.
- Team worktree/tmux orchestration beyond policy diagnostics unless already needed by closeout blockers.
- A public `workflow simulate` runtime mode unless it falls out naturally from the deterministic CLI demonstrator.
- Replacing `.agent-harness/` project files with authoritative workflow state.

## Closeout state model

Closeout should be represented as a replay-derived readiness model, not a mutable checklist file.

Recommended readiness dimensions:

| Dimension | Meaning | Blocks approval when |
| --- | --- | --- |
| evidence | Required acceptance categories are mapped to event/artifact refs. | A required category is missing and no valid waiver exists. |
| tasks | Workflow-owned persistent/team tasks are terminal. | Pending, claimed, or in-progress tasks remain. |
| continuation | Workflow-owned loops are stopped or terminal. | A continuation is active or reminder-queued. |
| artifact | Required artifacts exist with schema, digest, and redaction metadata. | Required artifact write failed or no artifact ref exists. |
| plan | Consensus plan is approved or explicitly bypassed. | Plan is required and unresolved, rejected, or stale. |
| goal | Goal/story checkpoints have evidence refs. | Aggregate goal is incomplete or final quality gate is missing. |
| team | Team work is synthesized or aborted with reason. | Team tasks/mailbox/shutdown proof are unresolved. |
| mission | Validator result exists for research missions. | Validator is missing, failed without accepted waiver, or still running. |
| wiki | Changed wiki pages pass lint when wiki writes are part of the workflow. | Lint fails or changed pages lack digest refs. |
| dossier | Dossier can be regenerated from events. | Required dossier export is missing or stale when policy requires export before approval. |

Each dimension should expose:

- `allowed`: boolean;
- `waived`: boolean;
- `blocking_refs`: workflow/task/artifact/event ids;
- `missing_categories` or equivalent domain-specific reasons;
- `recovery_hints`: short operator-safe next actions;
- `last_event_seq`: provenance for the readiness computation.

## Public signoff behavior

`harness workflow signoff` is the center of this slice.

Required behavior:

1. It targets an existing run directory, workflow id, or latest active workflow selected by the same status resolution rules as `workflow status`.
2. It reads the target event log, projects closeout readiness, and refuses approval when policy blocks closeout.
3. It appends the operator decision and terminal workflow completion to the target workflow's event log through the coordinator path.
4. It never creates a detached terminal workflow unless the command is explicitly an audit-only command.
5. It supports JSON and human output with the same fields as status/dossier readiness.
6. It prints legal next actions when denied.
7. It is idempotent for repeated approval of an already terminal workflow.
8. It records the policy version used for the decision.

Recommended commands:

```bash
harness workflow signoff --workflow-id <id> --run-dir <run> --approve [--json]
harness workflow signoff --workflow-id <id> --run-dir <run> --fail --reason <text> [--json]
harness workflow signoff --workflow-id <id> --run-dir <run> --request-evidence <category> --reason <text> [--json]
harness workflow signoff --workflow-id <id> --run-dir <run> --waive <category> --reason <text> [--json]
harness workflow signoff --workflow-id <id> --run-dir <run> --abort --reason <text> [--json]
```

Approval must require satisfied or waived gates. Failure, request-evidence, and abort may record decisions while gates are unsatisfied because those decisions explain why the workflow is not approved.

Acceptance criteria:

- Premature approval exits non-zero and appends no terminal success event.
- A waiver without reason is rejected.
- A valid waiver appends an operator decision and makes only the waived category non-blocking.
- Approval after evidence or waiver appends terminal success to the target workflow.
- Replaying the target event log shows the same signoff state and readiness.
- Dossier export after approval cites the signoff event, waiver events, evidence refs, and policy version.

## Operator decisions

Keep the decision set small but complete enough for closeout.

Canonical decisions:

- `signoff-approved`: accepts projected evidence and closes the workflow successfully.
- `signoff-failed`: closes the workflow unsuccessfully with reason.
- `request-evidence`: keeps workflow open or blocked and names missing evidence.
- `waive-evidence`: accepts a missing category for a specific reason.
- `abort`: operator cleanup/cancellation, not success.
- `redirect`: changes next action, owner, or required gate without pretending the workflow is complete.
- `approve-live`: optional approval to cross into live side effects when a workflow explicitly has a live lane.

Rules:

- Decisions that affect state append events.
- Passive reads never append decisions.
- Decision reasons are required for failure, request-evidence, waiver, abort, redirect, and approve-live.
- Decision payloads are capped and redacted.
- A decision must cite the workflow id and, when applicable, the readiness dimension it affects.
- A later decision cannot resurrect a terminal workflow except through an explicit future reopen command, which is out of scope.

## Typed closeout policy

The current workflow metadata should harden into an explicit policy layer that can be inspected, tested, and versioned.

Policy inputs:

- runtime workflow config;
- workflow mode and lane;
- workflow evidence categories;
- workflow-owned persistent tasks;
- active continuations;
- team projection;
- goal ledger projection;
- mission projection;
- wiki lint/write projection;
- dossier export policy;
- operator decisions and waivers.

Policy outputs:

- closeout readiness projection;
- denied transition event payloads;
- operator-safe recovery hints;
- doctor diagnostics;
- dossier quality gate section;
- TUI status/detail view models;
- JSON schema for CLI/tool output.

Start with a small built-in policy set:

| Policy id | Purpose |
| --- | --- |
| `workflow.closeout.default` | Requires evidence, terminal tasks, stopped continuations, and dossier readiness. |
| `workflow.closeout.simulated` | Requires deterministic fixture evidence and context snapshot evidence. |
| `workflow.closeout.goal` | Requires goal/story checkpoint evidence and final quality gate. |
| `workflow.closeout.team` | Requires team shutdown proof or abort reason plus synthesis evidence when team participated. |
| `workflow.closeout.mission` | Requires validator artifact or waived validator evidence. |
| `workflow.closeout.wiki` | Requires lint pass for changed wiki pages. |
| `workflow.closeout.live` | Requires explicit approve-live and live evidence only when live lane is used. |

Acceptance criteria:

- Policy ids are stable and documented.
- Readiness output is deterministic from projection inputs.
- Policy decisions are testable without provider/network/browser dependencies.
- Unknown policy ids fail closed with actionable diagnostics.
- Config docs and generated schema cover policy toggles.

## Evidence and artifact mapping

Evidence classification should become closeout-ready, not just display metadata.

Required evidence categories for this slice:

- `evidence.context_snapshot`
- `evidence.simulated_tool_result`
- `evidence.diagnostics`
- `evidence.test`
- `evidence.manual_qa`
- `evidence.review`
- `evidence.plan_consensus`
- `evidence.goal_ledger`
- `evidence.team_synthesis`
- `evidence.mission_validator`
- `evidence.wiki_lint`
- `evidence.operator_decision`
- `evidence.dossier_export`

Do not require every category for every workflow. The policy chooses required categories by mode and lane.

Evidence refs should include:

- category;
- acceptance criterion or gate id;
- source event seq/id;
- artifact path and digest when present;
- capped summary;
- side-effect class;
- redaction/capping metadata;
- producing workflow/task/team/mission/goal id when applicable.

Acceptance criteria:

- Evidence mapping survives replay without reading live workspace files except referenced artifacts.
- Artifact refs are normalized and workspace/session bounded.
- Missing artifacts produce blocked readiness, not a panic or false success.
- Dossier groups evidence by acceptance criterion and policy gate.

## Run Dossier hardening

The Run Dossier should become the operator's final closeout artifact.

Add or harden these sections:

- closeout summary;
- policy id and policy version;
- readiness matrix;
- evidence by acceptance criterion;
- missing and waived gates with reasons;
- task/team/continuation state at signoff;
- goal/story/mission/wiki state when those domains participated;
- operator decisions with event refs;
- terminal outcome and signoff event ref;
- export provenance and stale-export warning when applicable.

Rules:

- Dossier is still projection/export only, never authoritative state.
- Approval does not trust a dossier file; it trusts the event projection used to generate the dossier.
- Export may write JSON/markdown, but re-import is out of scope.
- Dossier export should be reproducible from the same event log and artifact refs.

Acceptance criteria:

- Export before approval clearly reports blocked gates.
- Export after approval cites the terminal signoff decision.
- Editing the exported dossier cannot change workflow status.
- JSON output is stable enough for tests and future tools.

## CLI and TUI surfaces

CLI is the primary manual QA surface for this slice. TUI surfaces should mirror projection state and provide operator clarity without becoming a second authority.

CLI requirements:

- `workflow status --json` includes closeout readiness and legal next actions.
- `workflow signoff` enforces target-run policy as described above.
- `workflow dossier export --json` includes the readiness matrix and policy version.
- `workflow cancel` records abort/cancel decisions with reasons and cannot erase evidence.
- Human output should be short, but it must name blockers and next commands.

TUI requirements:

- Slash/menu entries remain typed workflow intents, not shell snippets.
- Status dialog/sidebar shows closeout state, missing gates, and terminal outcome.
- Signoff prompt or command surface shows the same blockers as CLI.
- Replay mode stays read-only and cannot emit signoff decisions.
- No browser automation UI is added.

Acceptance criteria:

- CLI smoke test proves denied approval, waiver, approval, status, and dossier export.
- TUI unit tests cover readiness rendering and replay read-only behavior when TUI state changes.
- PTY signoff is required only when rendering/key behavior changes.

## Model-visible tools and skills

Keep this surface OMX-like: simple commands, skills, markdown assets, and coordinator-mediated primitives.

Allowed hardening:

- `workflow_status` returns closeout readiness.
- `workflow_signoff` wraps the same coordinator command as CLI and enforces the same policy.
- `workflow_dossier_export` writes only explicit export artifacts.
- `workflow_snapshot_write`, goal, mission, wiki, and plan tools may attach evidence refs through coordinator commands.
- Skills may document workflows or provide prompt guidance, but they do not own state transitions.

Not allowed in this slice:

- Playwright as a normal workflow tool.
- Browser command/state tools as workflow primitives.
- Hidden skill-side MCP execution that changes workflow state.
- Low-level event mutation tools.
- Tool outputs that claim signoff without coordinator approval.

Acceptance criteria:

- Tool schemas expose policy/readiness fields consistently with CLI JSON.
- Tool calls fail closed on missing permissions or invalid policy.
- Skill loading can contribute prompt guidance but not bypass closeout gates.

## Workflow skill discovery and setup proof

Skills can make the workflow easier to use, but they are not required to own workflow state. Treat them as discoverable guidance assets with diagnostics, not as executable policy modules.

Required behavior:

- Workflow doctor reports which first-party workflow skills are available, missing, disabled, or shadowed by project/user overrides.
- Skill diagnostics show the resolution root and skill name, not hidden prompt contents or secrets.
- Missing optional skills warn only when a command or profile would advertise them.
- Missing required workflow guidance fails setup checks only when the related workflow surface is enabled.
- Loaded skill content may instruct the agent, but closeout still depends on coordinator-owned evidence and policy decisions.

Acceptance criteria:

- Skill discovery diagnostics are stable in doctor JSON.
- Disabling a workflow skill does not break replay or dossier projection.
- No skill can mark a workflow complete without a coordinator signoff decision.

## Hook policy hardening

Hooks should become typed policy helpers, not arbitrary shell extension points.

Priority hook policies:

- pre-signoff readiness calculation;
- post-tool evidence classification;
- continuation stop/reminder closeout checks;
- compaction preservation of active closeout state;
- final handoff warning when evidence is missing;
- vague heavy-request redirect to interview or plan consensus;
- wiki summary/context injection when wiki is enabled;
- mission validator result classification.

Rules:

- Hooks are coordinator-mediated when they affect workflow state.
- Hook outputs are capped, redacted, and event-visible only when state-affecting.
- Replay sees hook decisions from events and never reruns hooks.
- Hook failure cannot create false approval.

Acceptance criteria:

- Tests cover allow, deny, classify, cap/redact, and replay behavior.
- Doctor reports hook policy readiness without launching external processes.

## Doctor and diagnostics

Doctor should answer whether workflows can be closed safely in the current configuration.

Add or harden checks:

- `workflow_closeout_policy`: policy ids, config, and schema shape are valid.
- `workflow_signoff_targeting`: signoff commands target existing workflows and do not default to detached audit success.
- `workflow_missing_evidence`: latest active workflow missing required categories.
- `workflow_stale_dossier`: exported dossier is older than events when policy requires export.
- `workflow_active_tasks`: workflow-owned tasks are still non-terminal.
- `workflow_active_continuations`: active/reminder-queued loops block closeout.
- `workflow_team_closeout`: team task/shutdown/synthesis blockers when team participated.
- `workflow_goal_closeout`: incomplete goal/story quality gates.
- `workflow_mission_closeout`: missing validator result.
- `workflow_wiki_closeout`: wiki lint blockers when wiki wrote pages.
- `workflow_tool_skill_surface`: confirms no browser automation is required for closeout.

Acceptance criteria:

- Doctor JSON has stable ids and actionable messages.
- Doctor human output distinguishes deterministic closeout readiness from optional live/browser/native signoff availability.
- Doctor never launches providers, browsers, terminals, MCP servers, validators, or team members by default.

## Configuration

Prefer harness-native policy names under the existing workflow namespace.

Possible shape:

```jsonc
{
  "runtime": {
    "workflow": {
      "closeout": {
        "enabled": true,
        "policy": "workflow.closeout.default",
        "require_dossier": true,
        "require_evidence": true,
        "require_manual_qa": true,
        "allow_waivers": true,
        "waiver_reason_required": true,
        "audit_only_signoff": false
      },
      "policies": {
        "workflow.closeout.default": {
          "required_evidence": ["evidence.context_snapshot", "evidence.test", "evidence.manual_qa"],
          "require_terminal_tasks": true,
          "require_stopped_continuations": true
        }
      }
    }
  }
}
```

Notes:

- Do not add browser policy keys for closeout; optional browser/media signoff lanes stay outside the workflow closeout contract.
- Existing `run.require_dossier`, `run.require_evidence`, and `work_loop.require_manual_qa` can feed this policy instead of being duplicated indefinitely.
- Unknown policy keys fail closed.
- Public config additions require schema, docs, examples, and drift tests.

## Deterministic demonstrator

The slice must end with one strong no-live demonstrator that proves the whole closeout path.

Minimum choreography:

1. Start a workflow run through CLI or command registry.
2. Write a context snapshot.
3. Record one test/simulated-tool evidence ref.
4. Create one workflow-owned task or continuation blocker.
5. Attempt signoff and observe denial with blockers.
6. Resolve or waive the blocker with required reason.
7. Export dossier and observe blocked/readiness state.
8. Approve signoff on the original workflow.
9. Export final dossier.
10. Replay status from the same event log and verify terminal state.
11. Attempt a late/conflicting transition and verify it is denied without mutating terminal success.

Negative cases:

- signoff without evidence;
- waiver without reason;
- stale active continuation;
- pending workflow-owned task;
- missing artifact ref;
- detached audit-only signoff attempt when policy expects target-run closeout;
- replay mode trying to sign off;
- late task/tool result after terminal closeout.

The demonstrator must use deterministic fakes and existing Harness paths. It must not depend on live providers, browsers, native screenshots, network access, or tmux panes.

## Testing plan

Focused tests before broad lanes:

- `harness-core` policy/readiness tests for evidence, task, continuation, waiver, terminal, and late-result behavior.
- Coordinator tests for target-run signoff append path and denied transition events.
- Replay tests for old logs and new closeout events/metadata.
- CLI tests for denied approval, waiver, approval, status, dossier export, JSON shape, and human smoke output.
- Tool tests if model-visible workflow tools change.
- Doctor tests for stable closeout check ids and no side effects.
- TUI view-model tests if readiness rendering changes.
- Config schema/docs drift tests for new policy keys.
- Deterministic testkit scenario for full closeout choreography.

Recommended verification commands:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p harness-core workflow
cargo test -p harness --test workflow_cli
cargo test -p harness-testkit workflow_simulator
cargo test -p harness --test config_docs_reference
cargo test -p harness --test event_docs_reference
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
```

Run `cargo test -p harness-tui` and `scripts/test-lanes.sh signoff-pty` only when TUI rendering or key behavior changes. Run live/browser/native signoff lanes only when those surfaces themselves change; they are not closeout policy requirements.

## Delivery order

### Phase 0: Contract inventory

Deliver:

- Existing signoff/status/dossier behavior map.
- Current policy config and evidence category inventory.
- Exact list of detached audit-run behavior to keep, replace, or rename.
- Stable closeout policy ids and readiness JSON shape.
- Explicit browser automation non-goal in docs/testing notes if needed.

Exit criteria: implementer can state how signoff targets an existing workflow and which old behavior remains audit-only.

### Phase 1: Core closeout policy

Deliver:

- Readiness projection struct with dimensions and recovery hints.
- Built-in closeout policy ids.
- Waiver validation.
- Dossier quality gate integration.
- Unit tests for allowed/blocked/waived states.

Exit criteria: policy can answer approval readiness from projections without CLI/TUI code.

### Phase 2: Coordinator-owned target signoff

Deliver:

- Signoff command path that loads the target run, projects readiness, appends decisions to the target workflow, and refuses premature approval.
- Denied transition event for blocked approval.
- Idempotent terminal behavior.
- Late-result protection remains intact.

Exit criteria: approving the original workflow is impossible until policy is satisfied or waived.

### Phase 3: CLI and dossier hardening

Deliver:

- `workflow status --json` readiness fields.
- `workflow signoff` approve/fail/request-evidence/waive/abort behavior.
- Dossier closeout matrix.
- Human output with blockers and next commands.

Exit criteria: CLI manual QA can drive denied approval, waiver, approval, status, and final dossier export.

### Phase 4: Doctor, config, and drift guards

Deliver:

- Closeout doctor checks.
- Config schema/docs/example updates for policy keys.
- Event/config docs drift tests.
- README/testing notes only where public surface changed.

Exit criteria: doctor distinguishes closeout readiness from optional browser/live/native lane availability.

### Phase 5: Tools, hooks, and TUI mirrors

Deliver only where needed by the public surface:

- Thin model-visible workflow tool updates.
- Typed hook policy tests for evidence classification and final handoff warnings.
- TUI readiness rows/dialog updates.

Exit criteria: secondary surfaces mirror coordinator policy and cannot bypass it.

### Phase 6: End-to-end deterministic closeout proof

Deliver:

- Testkit/CLI demonstrator covering the full choreography.
- Negative cases for missing evidence, missing waiver reason, pending tasks, stale continuation,
  unanswered questions, missing dossier export evidence, audit-only target mistakes, replay
  read-only behavior, and late result.
- Final dossier and replay equivalence assertions.

Exit criteria: one deterministic command/test proves closeout from run start through terminal signoff without live/browser/native dependencies.

## Open decisions

These should be resolved during implementation, not by expanding the slice blindly:

1. Whether the old audit-run signoff command becomes `workflow signoff --audit-only` or an internal helper.
2. Whether approval should require a dossier export event or only dossier regenerability when `require_dossier=true`.
3. Whether waiver scope is category-only or category plus acceptance criterion.
4. Whether `approve-live` belongs in `workflow signoff` or a separate `workflow live approve` command.
5. Whether status should auto-select latest active workflow or require explicit `--workflow-id` when multiple workflows are active.
6. Whether wiki lint is required only when wiki changed or whenever wiki context was injected.
7. Whether team synthesis evidence is required for any team participation or only when team owns a goal/story.

## Definition of done

The slice is complete when:

- `workflow signoff --approve` targets and closes the intended existing workflow, not a detached audit run.
- Approval is denied until required evidence/tasks/continuations/artifacts/domain gates are satisfied or waived with reason.
- Failure, request-evidence, waiver, abort, redirect, and approve-live decisions are durable, capped, redacted, and replay-visible.
- Status, dossier, doctor, CLI JSON, and TUI/readiness mirrors report the same closeout state.
- The deterministic demonstrator proves denied signoff, waiver or evidence satisfaction, approved signoff, final dossier, replay equivalence, and invalid late transition handling.
- Browser automation remains outside the workflow product surface and is not required for closeout.
- Config/schema/docs/examples/tests reflect every new public policy field and event/docs drift stays clean.

## Why this is the right next slice

The first workflow slice made Harness capable of recording and projecting workflow state. The next product gap is trust: the operator needs closeout to mean something stronger than "the agent said it is done." Evidence-gated target-run signoff, typed closeout policy, and deterministic dossier proof turn the workflow layer from scaffolding into an auditable operating surface.

This slice is broad because it connects every workflow domain already introduced: plan, goal, team, mission, wiki, task, continuation, evidence, doctor, CLI, TUI, and tools. It stays contained because it does not introduce new domains, browser automation, plugin execution, or a second runtime. It hardens what exists into one coherent closeout path.
