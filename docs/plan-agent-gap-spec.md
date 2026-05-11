# Plan agent gap specification

This document specifies the remaining gaps between the Harness `plan` agent and the
local upstream-style reference `plan` agent. It is intended to
guide follow-up implementation without weakening the runtime guardrails Harness
already has.

## Scope and evidence

Harness evidence:

- `crates/harness-core/src/config/public.rs` defines the shipped `plan` profile.
- `crates/harness-core/src/coord.rs` injects the plan-mode system reminder.
- `crates/harness-core/src/plan.rs` defines `plan`, `build`, `plan_exit`, and
  `.agent-harness/plans` path constants.
- `crates/harness-tools/src/plan.rs` implements `plan_exit`.
- `crates/harness-tools/src/agent_ops.rs` blocks write-capable child delegation
  from plan mode.
- `crates/harness-core/src/config.rs`, `crates/harness-core/tests/coord_auth.rs`,
  and `crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order.rs`
  cover the current behavior.

Reference evidence:

- `src/agent/agent.ts` in the reference tree defines the native `plan` agent.
- `src/tool/plan.ts` in the reference tree implements `plan_exit`.
- `src/tool/registry.ts` in the reference tree gates plan tooling behind a
  reference CLI feature flag.
- `src/session/prompt/plan.txt` and `plan-reminder-anthropic.txt` in the reference
  tree define the read-only planning reminders.
- `test/agent/agent.test.ts` and `plan-mode-subagent-bypass.test.ts` in the
  reference tree cover agent registration, edit boundaries, and plan-mode subagent
  permission inheritance.

## Current Harness status

Harness already has a functional Plan agent, not just a prompt placeholder:

- `plan` is a built-in primary profile.
- Edits are denied by default and allowed only under `.agent-harness/plans/`.
- `bash` is exposed behind shell permission and constrained by Plan instructions
  to read-only inspection.
- Read/search, questions, `task`, `background_output`, `plan_enter` from Build,
  and `plan_exit` from Plan are available.
- The coordinator appends a plan-mode reminder to every Plan turn.
- `plan_exit` asks the user for approval, spawns a `build` agent, and schedules a
  build continuation against the active plan file.
- Plan-mode delegation is stricter than the reference: Harness permits only the
  read-only `explore` profile, and rejects `general` or other write-capable child
  profiles.

## Gap classification

Use these labels when converting this spec into implementation work:

- **Parity gap**: Harness behavior is missing or materially weaker than the reference.
- **Intentional divergence**: Harness differs from the reference because the current
  runtime contract is stronger or more deterministic.
- **Documentation gap**: Behavior exists, but public/operator docs do not explain
  it with reference-level clarity.
- **Test gap**: Behavior exists, but there is no focused regression test for it.

## Detailed gaps

### G1. Plan-mode prompt depth

**Type:** Parity gap / documentation gap

The reference has richer prompt assets for Plan mode. `plan.txt` states the read-only
constraint plainly, while `plan-reminder-anthropic.txt` adds a structured workflow:
initial understanding, up to three Explore agents, planning, synthesis, final plan,
and `ExitPlanMode` as the expected end state.

Harness currently injects a shorter coordinator reminder. It includes the core
guardrails and says Plan may launch up to three `explore` subagents, ask
clarifying questions when exploration cannot resolve ambiguity, and write/update
the plan file. It does not yet provide reference-level guidance for plan quality,
phase transitions, synthesis, or the expected terminal action.

**Desired Harness behavior:**

- Keep the coordinator-injected reminder as the authoritative guardrail.
- Expand the reminder or add a shipped prompt asset so Plan consistently follows
  a structured workflow:
  1. understand the request using read-only tools;
  2. launch zero to three `explore` children only when useful;
  3. synthesize findings into one recommended plan;
  4. update the active plan file;
  5. ask a clarifying question or call `plan_exit` when ready.
- Explicitly say the plan file should contain the final recommended approach, not
  an exhaustive transcript of alternatives.

**Acceptance criteria:**

- A provider request for the `plan` profile includes the expanded workflow text.
- The text still says edits are allowed only for the active plan file and that
  non-read-only tools, config changes, and commits are forbidden.
- Tests assert the injected prompt includes the active Harness plan-file path and
  the terminal-action guidance.

### G2. Plan file lifecycle and creation guidance

**Type:** Parity gap

The reference reminder tells the model whether a plan file exists and where to create
it. Harness computes a deterministic active plan path from the run id, but the
reminder only tells the model to write or update that path.

**Desired Harness behavior:**

- The Plan reminder should distinguish the initial no-plan-file state from the
  update-existing-plan state when the runtime can determine it cheaply and safely.
- The reminder should tell the model that `.agent-harness/plans/<run>.md` is the
  only writable target during Plan mode.
- Plan-file path rendering should remain workspace-relative in model-facing text.

**Acceptance criteria:**

- The first Plan turn for a run explains that no plan file exists yet and gives
  the exact active path.
- Later Plan turns can say the active plan file exists, or at minimum continue to
  point at the same deterministic path.
- Tests cover path sanitization and prompt text for both create/update wording if
  existence-aware wording is implemented.

### G3. Plan-enter flow from Build

**Type:** Implemented parity behavior / regression coverage anchor

The reference has `plan_enter` guidance that lets the Build agent suggest switching to
Plan before complex or multi-file work. Harness now has a native `plan_enter`
tool that asks for approval, spawns a Plan agent through the coordinator, and
schedules a Plan continuation with the original goal and active plan-file path.

**Desired Harness behavior:**

- Add a `plan_enter` workflow if Harness wants reference-level two-way agent
  switching.
- Build should be able to ask the user whether to switch into Plan for complex,
  ambiguous, or high-risk changes.
- If approved, the coordinator should spawn/switch to `plan`, pass the original
  user goal, and keep Plan under the same read-only guardrails.

**Acceptance criteria:**

- `build` exposes `plan_enter` when the shipped profile/tool surface enables it.
- `plan` denies `plan_enter` to avoid recursive or nonsensical switching.
- Approval creates or selects a Plan agent and schedules a Plan turn with the
  original goal and active plan-file path.
- Decline leaves the Build agent active and records the user decision.

### G4. Tool exposure and feature gating semantics

**Type:** Implemented parity behavior / security guardrail

The reference exposes `plan_exit` through the tool registry only when its
experimental plan-mode CLI flag is active. Harness ships `plan_exit` as part of the
built-in Plan profile without an experimental gate.

This is likely intentional: Harness treats Plan as part of the public runtime
surface. The gap is that the stability contract is not stated as explicitly as the
implementation implies.

**Desired Harness behavior:**

- Keep `plan_exit` always available to the shipped `plan` profile unless the
  project deliberately disables it.
- Document that Plan mode is stable public Harness behavior, not an experimental
  feature flag.
- If a future gate is introduced, it must not silently remove `plan_exit` from
  existing configs without validation or migration guidance.

**Acceptance criteria:**

- Public docs say whether Plan is stable or experimental.
- Config validation/drift tests fail if the shipped `plan` profile loses
  `plan_exit`, `task`, or its edit-boundary rules unexpectedly.

### G5. Permission model differences for bash and edits

**Type:** Intentional divergence / documentation gap

The reference docs describe Plan as restricted and say edits/bash are controlled by
permission prompts, while current reference source denies broad edits and allows
only plan-file edits. Harness now exposes `bash` from the shipped Plan profile for
workflow parity, but keeps the Plan lane hard-bounded with shell permission prompts
plus a coordinator-side read-only inspection guard.

**Desired Harness behavior:**

- Keep `bash` available to Plan for reference-style tool parity.
- Deny mutating shell commands in Plan before execution, even if shell permission
  would otherwise allow or ask.
- Document that Plan bash is for read-only inspection only and remains secondary
  to native read/search/LSP tools.

**Acceptance criteria:**

- The shipped `plan` profile includes `bash` with shell permission `ask`.
- Tests prove Plan bash permits only read-only inspection commands and rejects
  mutating shell commands before execution.
- Docs explain how Harness combines reference-style bash exposure with stronger
  runtime enforcement.

### G6. Subagent permission inheritance vs subagent allowlist

**Type:** Intentional divergence / test gap

The reference fixes plan-mode subagent bypasses by deriving child session permissions
from the parent Plan agent ruleset. That permits subagents such as `general` while
ensuring they inherit Plan read-only restrictions.

Harness instead blocks Plan from launching anything except `explore`. This is
safer and simpler, but it is stricter than the reference and means Harness currently
does not need general child-permission inheritance to preserve Plan safety.

**Desired Harness behavior:**

- Keep the `explore`-only allowlist unless there is a deliberate product decision
  to allow `general` in Plan mode.
- If non-`explore` Plan subagents are ever allowed, implement reference-style
  parent permission inheritance first.
- Continue recording policy denials instead of spawning disallowed children.

**Acceptance criteria:**

- Existing tests continue to show Plan can launch `explore`, cannot launch
  `general`, and cannot use bash for mutating commands.
- Add/keep a regression test for user-defined subagents from Plan mode; the
  expected result should be denial before spawn under the current allowlist.
- If the allowlist changes, tests must prove child effective permissions deny
  edits outside the active plan file.

### G7. Plan-exit continuation semantics

**Type:** Partial parity gap

The reference `plan_exit` appends a synthetic Build user message saying the plan has
been approved and the agent can now edit files. Harness `plan_exit` asks approval,
spawns `build`, and schedules a continuation with a system reminder that the mode
changed and the plan file should be executed.

Harness is functionally equivalent for the handoff, but the prompt contract could
be tightened so Build receives enough context to execute consistently.

**Desired Harness behavior:**

- Keep the approval prompt and Build spawn behavior.
- Include the active plan-file path and explicit instruction to read/execute the
  plan.
- Consider including the final Plan summary or plan-file contents in the Build
  prompt only if doing so does not duplicate large context or bypass event/artifact
  boundaries.

**Acceptance criteria:**

- `plan_exit` output includes `agent`, `build_agent_id`, `request_id`,
  `plan_file`, and `approved`.
- The scheduled Build prompt includes mode-change text and the plan-file path.
- Cancellation/decline leaves the Plan agent active and does not spawn Build.

### G8. UI and operator affordances

**Type:** Documentation gap / possible parity gap

The reference docs emphasize switching between primary agents with the client keybind
and describe Plan as a selectable primary mode. Harness README says Plan is
available through the agent/model switcher, but the Plan-specific lifecycle is not
documented as an operator workflow.

**Desired Harness behavior:**

- Document the operator workflow: switch to Plan, allow it to create/update
  `.agent-harness/plans/<run>.md`, approve or decline `plan_exit`, then continue in
  Build.
- Ensure TUI/headless surfaces show enough metadata for the active agent,
  plan-file path, and `plan_exit` question.
- If a `plan_enter` tool is added, document when Build should use it.

**Acceptance criteria:**

- README or `docs/config.md` links to this workflow or absorbs it into a public
  Plan-mode section.
- CLI/TUI tests cover that Plan is selectable/listed if such coverage is not
  already present.

### G9. Public config and schema drift coverage

**Type:** Test gap

Harness has tests proving the public default `plan` profile exists, includes
`bash` behind shell permission, contains `task`, and contains `plan_exit`. It also
has focused tool tests for `plan_exit`, `plan_enter`, shell guardrails, and
delegation boundaries. The remaining drift risk is that public docs/schema
examples could diverge from the profile as Plan evolves.

**Desired Harness behavior:**

- Treat the shipped `plan` profile as part of the public config contract.
- Add drift checks whenever new Plan public keys or tools are introduced.
- Keep docs aligned with generated schemas and built-in defaults.

**Acceptance criteria:**

- Public config tests assert the complete minimum Plan contract:
  `mode=primary`, edit boundary, shell ask/`bash`, `task`, `background_output`,
  read/search tools, no `plan_enter`, and `plan_exit`.
- Config docs mention `.agent-harness/plans/` and the handoff to Build.

## Recommended implementation order

1. **Prompt/spec parity first:** expand the Plan reminder/workflow text and add
   prompt assertions. This improves behavior without changing permissions.
2. **Docs and operator workflow:** document Plan lifecycle, stability, guarded
   Plan shell inspection, and the stricter no-write-capable-subagent stance.
3. **Plan-enter design:** decide whether Harness should add Build-to-Plan
   switching. If yes, implement as a separate tool with approval and tests.
4. **Optional permission inheritance:** only needed if Harness relaxes the current
   `explore`-only Plan delegation allowlist.
5. **Additional drift tests:** lock the public Plan profile contract so future
   config/tool changes fail loudly.

## Non-goals

- Do not weaken Harness Plan mode to allow broad edits or mutating shell commands
  by default.
- Do not allow `general` or user-defined subagents from Plan unless parent
  permission inheritance is implemented and tested first.
- Do not make Plan-mode safety rely only on prompt text; permissions and
  coordinator/tool policy must remain the enforcement layer.
- Do not copy the reference's experimental gating unless Harness intentionally
  changes Plan from stable default behavior to an opt-in feature.
