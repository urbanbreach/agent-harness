# Plan 001: Give subagents independent role permissions

> **Executor instructions:** Implement this plan in order. Reuse the existing
> profiles, registry, permission evaluators, coordinator gates, and tests. This is
> a scoped change to delegated agents. Complete each verification gate before
> continuing, preserve unrelated work, and update the index when finished.

- **Status:** COMPLETE
- **Follow-up (2026-09-14):** The user subsequently requested the reference research
  roles' broader tool access. Research defaults now include bash, LSP, skills, and
  configured MCP discovery. The original shell/LSP omissions and MCP opt-in policy
  below describe the initial delivery and are superseded for Explore/Librarian;
  see [the current contract](../docs/operations/generic-agent-and-tasks.md).
- **Priority:** P1
- **Effort:** M
- **Risk:** MED — changes permission precedence for delegated work
- **Depends on:** none
- **Category:** direction / correctness
- **Planned at:** commit `062e5ea8`, 2026-09-14
- **Workspace:** `/home/urbanbreach/Projects/agent-harness`

## Outcome and confirmed decision

Harness should have research subagents with exploration tools and an implementation
subagent with editing tools. Each role owns its tool list and permissions. The user
explicitly confirmed that a parent whose **own role** denies editing may delegate
to a child that allows editing, provided shared project policy permits it.

Keep the existing names: `explore`, `librarian`, and `general`. `general` is the
implementation-capable role, matching vanilla OpenCode's name. No new `implementer`
alias, agent configuration format, or role registry is needed.

There are three separate decisions:

1. The parent's current toolset and `task` permission decide whether it can start
   or continue the selected child.
2. The child's configured toolset and own role permissions decide its capabilities.
3. The effective shared policy constrains every child action. It does not include
   the parent's role overlay.

For children, combine the existing child-role decision with
`PermissionPolicy::evaluate_request(None, kind, selector)`: **deny wins; otherwise
ask wins; otherwise allow**. Existing approval/grant handling may satisfy an ask,
but may never override a deny. Keep the current primary-agent precedence unchanged.
This deliberately makes shared policy a ceiling for delegated work; it is not a
rewrite of permission precedence for the whole product.

| Parent role edit | Child role edit | Shared edit | Child result, if edit tool is listed |
|---|---|---|---|
| deny | allow | allow | allow |
| allow or deny | deny | allow | deny |
| allow or deny | allow | deny | deny |
| allow or deny | allow | ask | ask through existing approval flow |

An absent tool remains unavailable regardless of these permission decisions.

## Comparison and current state

This plan uses the checked-out reference implementations in the local Inspirations
folder, not claims about newer upstream releases.

| Reference | Relevant behavior | What Harness should adopt |
|---|---|---|
| [Vanilla OpenCode roles](/home/urbanbreach/Projects/agent-harness/inspirations/opencode/packages/opencode/src/agent/agent.ts:182) | `general` can implement; `explore` uses a deny-first role with selected research tools. | Distinct, enforced toolsets for each role. |
| [OpenCode child permissions](/home/urbanbreach/Projects/agent-harness/inspirations/opencode/packages/opencode/src/agent/subagent-permissions.ts:4) | Parent session restrictions are separate from parent agent restrictions. Child capabilities come from its own agent. | Stop copying the parent's role denies into children; retain shared restrictions separately. |
| [Senpi agent definitions](/home/urbanbreach/Projects/agent-harness/inspirations/senpi/packages/coding-agent/examples/extensions/subagent/agents.ts:88) and [subagent extension](/home/urbanbreach/Projects/agent-harness/inspirations/senpi/packages/coding-agent/examples/extensions/subagent/index.ts:305) | The example extension reads each role's prompt/model/tools and passes the tools through `--tools`. | Make the existing role tool list authoritative. Keep Harness's coordinator-based execution. |

Harness already has most of the required machinery:

- [Public profiles](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/config/public/agents.rs:79)
  materialize the four fixed profiles, with separate tools and permissions.
- [Worker execution](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/coord/tool_execution.rs:94)
  resolves the registered worker identity, checks exact tool membership, and ignores
  a caller-supplied profile name. Preserve this authority boundary.
- [Provider tool definitions](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/agent/provider_boundary.rs:498)
  consume the runtime profile toolset and select the model's editing surface.
- [Batch execution](/home/urbanbreach/Projects/agent-harness/crates/harness-tools/src/agent_ops/batch.rs:72)
  submits inner calls through the coordinator with the original actor. It needs
  behavioral verification, not a second permission implementation.

The changes address these concrete gaps:

1. Both research roles currently include `bash`, and their `shell` permission is
   allow. An edit deny cannot prevent a shell program from modifying files.
   Librarian also exposes broad `lsp`, whose [request surface includes
   `installDecision`](/home/urbanbreach/Projects/agent-harness/crates/harness-tools/src/code_lsp.rs:237).
2. Explore lists `ast_grep_search` but denies `codesearch`; [the permission
   mapping](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/perm.rs:810)
   uses `CodeSearch` for both tools, so its native AST search is filtered out.
3. [Bootstrap](/home/urbanbreach/Projects/agent-harness/crates/harness/src/bootstrap.rs:432)
   appends auto-discovered MCP tools to every profile:

   ```rust
   normalize_profile_toolset(&profile_cfg.tools, editing_surface)
       .iter()
       .map(String::as_str)
       .chain(extra_tool_ids.iter().map(String::as_str))
   ```

4. [Spawn](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/coord/run_lifecycle.rs:737)
   supplies parent **role** rules where shared restrictions are intended:

   ```rust
   let derived = crate::perm::derive_subagent_session_permission(
       &parent_profile.permission_ruleset,
       &child_permission,
   );
   ```

   [Resume](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/coord/run_lifecycle.rs:285)
   instead inserts the configured profile without the same preparation/pruning.
5. [Permission evaluation](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/perm.rs:579)
   returns a matching role rule before consulting shared policy:

   ```rust
   Some(PermissionAction::Allow) => PolicyDecision::Allow,
   // ...
   None => self.evaluate_request(profile, kind, selector),
   ```

   Simply deleting inheritance would therefore let a child allow override shared
   deny/ask settings. The child-specific combination above is required.

Repository constraints: the coordinator owns permissions, lifecycle, cancellation,
and event appends. Durable history remains authoritative; replay/inspection must
not execute tools or perform network work. Permissions are policy checks, not an
OS sandbox. Follow existing `Result<_, CoordinatorError>` handling in
[tool execution](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/coord/tool_execution.rs:94)
and existing deterministic fixtures. Use nextest, never `cargo test`.

## Target defaults

Keep the defaults below in the existing functions in
[public/agents.rs](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/config/public/agents.rs:231).
Tool lists remain configurable through the existing `agent.<name>.tools` and
`agent.<name>.permission` fields.

| Role | Default tools |
|---|---|
| `explore` | `read`, `glob`, `grep`, `list`, `ast_grep_search`, `webfetch`, `websearch`, `session_list`, `session_read`, `session_search`, `session_info`, `batch` |
| `librarian` | Explore's tools plus `codesearch` |
| `general` | Librarian's tools plus `edit`, `write`, `apply_patch`, `bash`, `lsp` |
| `default` | Existing primary defaults |

- Research roles: deny edit, shell, LSP, question, task, and todo mutation. Keep
  read/research permissions compatible with the listed tools. Set explore's
  `codesearch` permission to allow so `ast_grep_search` works; the external
  `codesearch` tool stays absent from explore's list.
- General: retain editing, shell, LSP, and research permissions. Remove the already
  denied `question` and `skill` entries from its default list. It remains unable
  to redelegate by default; parent `load_skills` continues to supply task context.
- No child gets MCP tools automatically. An operator can list exact registered
  MCP tool IDs in a role. Its role and shared policies must also permit the tool's
  existing capability. A stdio MCP tool is classified as shell, so explicitly
  admitting one to a research role also requires permitting that capability;
  this does not add native `bash` to the role's tool list. HTTP MCP uses network.
  Do not infer safety from the transport or MCP read-only hints, or add a generic
  `mcp.<server>.tool.call` gateway to research defaults.

Removing bash is intentionally stricter than vanilla OpenCode's explore default.
Broad LSP is omitted from research defaults because it includes server-management
operations; `lsp.rename` is already a separate edit capability. Do not introduce
a shell classifier or split LSP operations in this change. Existing research
prompts already describe the intended behavior and need no rewrite.

## Scope, drift check, and working tree

The following array is the exact modification scope, including test registration
only if new coordinator test entry points are necessary. Run these commands from
the workspace above and retain the array for the final scope check.

```bash
role_plan_paths=(
  crates/harness-core/src/config/public/agents.rs
  crates/harness-core/src/config/tests/agents_profiles_test.rs
  crates/harness-core/src/config/tests/discovery_merge_test.rs
  crates/harness/src/bootstrap.rs
  crates/harness/tests/bootstrap_profiles/permission_ruleset_export_test.rs
  crates/harness/tests/snapshots/v1_composed_prompts/general.txt
  crates/harness-core/src/perm.rs
  crates/harness-core/src/perm/tests.rs
  crates/harness-core/src/coord/permission.rs
  crates/harness-core/src/coord/tool_execution.rs
  crates/harness-core/src/coord/run_lifecycle.rs
  crates/harness-core/src/coord/tests.rs
  crates/harness-core/src/coord/tests/permission_flow_tests.rs
  crates/harness-core/tests/coord/11_resume_existing_run_restores_sequence_and_test.rs
  crates/harness-tools/src/agent_ops/child_metadata.rs
  crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/03b_child_agent_toolset_boundary_test.rs
  crates/harness-tools/tests/native_agent_spawn_child_session_observability_test.rs
  docs/operations/generic-agent-and-tasks.md
  docs/permissions/permissions.md
  plans/001-role-scoped-subagents.md
  plans/README.md
)
git status --short
git diff --stat 062e5ea8..HEAD -- "${role_plan_paths[@]}"
git diff -- "${role_plan_paths[@]}"
```

Expected: current-state excerpts still describe the runtime. Compare both committed
drift and uncommitted changes before editing. Unrelated config and adaptive-compaction
work was in progress while this plan was written, including compaction test
registration in `coord/tests.rs`. Preserve those edits and keep any additional
test registration surgical. Do not regenerate
`configs/config.json` or edit `harness.jsonc`: this plan adds no schema fields.

The user approved extending the scope on 2026-09-14 to update the three existing
role fixtures: `discovery_merge_test.rs`, `permission_ruleset_export_test.rs`, and
the general composed-prompt snapshot. These paths are included above.

Out of scope: schedulers/concurrency limits, agent teams or messaging, arbitrary
custom role names, subprocess agents, worktrees/sandboxes, model selection,
provider transports, task/background API changes, skill loading semantics,
historical event schema changes, primary-agent permission precedence, and the
reference repositories. Resume uses current configuration as today; this plan
does not persist historical permission snapshots.

If a branch is needed, use `codex/role-scoped-subagents`. Do not reset the dirty
worktree, commit unrelated changes, push, or publish anything as part of this plan.

## Implementation steps

### 1. Make each role's configured tools authoritative

Update `research_tools`, `librarian_tools`, `general_tools`, and their permission
functions to the target defaults. In `interactive_agent_profiles_with_extra_tools`,
append `extra_tool_ids` only when `profile_cfg.mode == AgentMode::Primary`.
Explicit IDs already supplied in a child's own list must survive normalization.

Remove the raw `Tools: {tools}` suffix in `task_tool_description_for_profile`;
it currently describes config before runtime pruning. Retain role descriptions
and the parent's existing task-target filter. Actual provider definitions and
child runtime toolset metadata remain the sources for available tools.

Extend `public_agent_config_materializes_primary_and_named_subagents` and the
existing bootstrap defaults/MCP tests. Check research roles omit all mutation
surfaces, general retains its implementation tools, AST search has a compatible
permission, discovered MCP remains automatic for the parent, and only an exact
explicitly listed MCP ID appears in the selected child.

**Verify:**

```bash
cargo nextest run --offline --profile ci -p harness-core --lib -E 'test(agents_profiles)'
cargo nextest run --offline --profile ci -p harness --lib -E 'test(bootstrap::)'
```

Expected: nonzero selected tests, all pass; no network services required.

### 2. Enforce independent child roles under shared policy

In `perm.rs`, add a small child-specific evaluator, e.g.
`evaluate_child_request_with_ruleset`, which combines the two existing evaluations
described above. Preserve last-match rules **within** each policy layer; combine
only their resulting decisions. No new parser, policy engine, config field, or
parent-role argument.

Extend the existing fully-denied visibility logic for children. Hide a tool if
either its own role or shared policy categorically denies its capability. Keep
partially permitted tools visible and check the actual selector at execution.
The shared check must inspect `default_rules` for non-deny selector exceptions,
just as the existing helper inspects profile selectors: blindly calling
`is_tool_call_fully_denied(None, ..., &[])` would wrongly hide a shared
deny-by-default tool with an allowed path. Be conservative about hiding when
rules have exceptions; no wildcard-overlap solver is needed. Apply this exception
awareness when a role without a matching rule falls back to shared policy too,
so neither side incorrectly removes a partially permitted tool.

Route child evaluation through the existing
`evaluate_permission_rule_requests_with_ruleset` aggregator in `coord/permission.rs`.
Select the child path from the registered actor's membership in
`run_state.subagent_parent_by_id`, not from profile names or task arguments.
Update all three calls in `tool_execution.rs`: ordinary permission, doom-loop,
and external-directory gates. Keep question's existing prompt workflow and
batch's inner-call routing; configured child tools must still pass deny pruning.

Check static deny **before** reusable grants or always-approve shortcuts. In the
doom-loop gate specifically, keep the streak threshold, but after it is reached
evaluate child deny before `doom_loop_always_granted` or remembered grants can
skip the gate. Ordinary/external-directory denial must retain the same precedence.

In `run_lifecycle.rs`, extract one small profile-preparation function and use it
both for fresh spawn and resume. It takes the configured profile, whether it has
a parent, and the existing registry/policy; it does not take the parent's profile.
For a child, preserve the current task/todowrite default behavior and explicit
tool/rule opt-ins. Reuse `derive_subagent_session_permission(&[], &child_permission)`
for those defaults instead of copying parent rules. Prune the resulting toolset
with the child/shared visibility check. Resolve restored parent identity before
preparation so the resumed provider receives the same filtered toolset as a new
child under the same configuration. Preserve missing-profile restore errors.

In `child_permission_metadata`, report the existing
`isolated_by_child_profile` relation even when parent and child use the same
profile name. Keep the existing fields and toolset-qualified posture labels;
listed tools may still ask or be denied for particular arguments. Do not rewrite
historical metadata or create new durable policy events.

Add the tests in the next section as part of this step. Test the coordinator
boundary and absence of tool side effects, not just helper return values.

**Verify:**

```bash
cargo nextest run --offline --profile ci -p harness-core --lib -E 'test(perm::) | test(coord::tests::)'
cargo nextest run --offline --profile ci -p harness-core --test coord_test -E 'test(resume_existing_run)'
cargo nextest run --offline --profile ci -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test --test native_agent_spawn_child_session_observability_test
```

Expected: all selected tests pass. Child edit allow survives a parent-role edit
deny, shared restrictions hold, and spawn/resume expose equivalent toolsets.

### 3. Document and validate the delivered contract

Update the permission/toolset sections of
[generic-agent-and-tasks.md](/home/urbanbreach/Projects/agent-harness/docs/operations/generic-agent-and-tasks.md)
and [permissions.md](/home/urbanbreach/Projects/agent-harness/docs/permissions/permissions.md).
Include the role matrix, independent parent/child roles, shared child ceiling,
MCP opt-in rule, shell/LSP omissions, and current-config behavior on resume.
Use public permission names such as `bash`, not the internal `shell` field name.
Explain that skills supply instructions and cannot grant tools. Preserve the
existing statement that permissions do not provide OS confinement.

**Verify:** run each command once after the scoped gates pass:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
scripts/test-lanes.sh quality-gates
git diff --check
git diff --stat -- "${role_plan_paths[@]}"
git status --short
```

Expected: formatting, compilation, lints, quality gates, and changed-behavior tests
pass; no new test failures or out-of-scope edits. If a broader failure predates
this change, reproduce it without this plan's diff and record the evidence rather
than fixing unrelated code. Do not label a failing full suite as passing.

## Minimum behavioral coverage

Extend existing tests first. Add a case only for a plausible regression below;
do not snapshot every tool list at multiple layers or introduce a test framework.

| Behavior to protect | Existing location / approach |
|---|---|
| Child role and shared decision precedence; primary precedence unchanged; selector exceptions keep tools visible | Extend [permission tests](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/perm/tests.rs) with a table of allow/ask/deny and shared deny plus path-allow cases for both an explicit role allow and a role falling back to shared rules. Keep the existing primary override test. |
| Parent-role edit deny with child edit allow really writes; research roles reject direct and batched writes and shell calls without side effects | Extend [child toolset boundary tests](/home/urbanbreach/Projects/agent-harness/crates/harness-tools/tests/native_agent_spawn_and_batch_preserve_lineage_permissions_and_order/03b_child_agent_toolset_boundary_test.rs). Reuse native tools, temporary workspaces, and the existing skill claiming `allowed_tools: task, edit`; forge a caller profile as the existing test does. Include positive read/search behavior so an empty child toolset cannot pass. |
| Shared ask pauses a child until approved; shared deny beats a child allow, remembered grants, and always-approve | Extend [permission flow tests](/home/urbanbreach/Projects/agent-harness/crates/harness-core/src/coord/tests/permission_flow_tests.rs), following `static_deny_overrides_permission_grant` and existing ask tests. Cover the external-directory and threshold-triggered doom-loop gates through coordinator calls; use counters/events to assert no denied tool execution. Register new entry points in `coord/tests.rs` only if extending existing functions is insufficient. |
| Spawn and resume agree on tool availability and execution decisions | Extend [resume tests](/home/urbanbreach/Projects/agent-harness/crates/harness-core/tests/coord/11_resume_existing_run_restores_sequence_and_test.rs), using the existing child-lineage fixture and capturing provider. Check a child allowed despite parent deny and a shared-denied tool before/after resume; compare advertised definitions and `AgentRuntimeInfo`. Inspection alone must not call the provider/tools. |
| MCP discovery cannot broaden a child; an exact configured MCP tool remains available | Extend the existing bootstrap MCP test. The direct/inner worker membership tests above protect execution of absent tools; no live MCP service is needed. |
| Task results describe the child's role, including same-profile parent/child cases | Extend [child observability tests](/home/urbanbreach/Projects/agent-harness/crates/harness-tools/tests/native_agent_spawn_child_session_observability_test.rs). Preserve child IDs, ownership, and toolset-qualified posture fields. |

The two native agent integration binaries passed **41 tests** during the comparison
at the planned commit/worktree. This is baseline evidence only, not validation of
the future implementation. Foreground/background execution, model inheritance,
reentry ownership, cancellation, and loaded-skill checks in those binaries remain
regression coverage; do not replace them with new duplicate tests.

## Done criteria

- [x] Target defaults and automatic-versus-explicit MCP admission are asserted by passing tests.
- [x] A real child write succeeds under parent-role deny/shared allow; research writes remain blocked, including through batch and loaded skills.
- [x] Shared deny/ask precedence and selector exceptions pass; primary override behavior remains covered.
- [x] Denied calls have no tool side effects even with applicable approval shortcuts.
- [x] Same-config spawn/resume tool definitions, runtime metadata, and authorization agree.
- [x] All verification commands ran; any pre-existing broad-suite failures are separately evidenced.
- [x] `git diff --check` passes and the diff adds no dependencies, schema fields, or historical event changes.
- [x] No new edits outside `role_plan_paths`; baseline dirty work is preserved.
- [x] This plan and its row in [the index](/home/urbanbreach/Projects/agent-harness/plans/README.md) record completion and validation evidence.

## Stop conditions and maintenance

Stop and report a concrete mismatch if live code no longer has the policy/lifecycle
seams described here, current configuration cannot represent the target lists,
or implementation requires a new permission schema or changing primary precedence.
If a scoped gate still fails after a reasonable fix attempt, report the failing
case and cause before broadening scope. Routine test-fixture updates within this
plan do not require a new approval.

Future native tools and MCP discovery must respect explicit child admission.
If research later needs LSP query operations or command execution, add a separately
bounded surface with its own behavioral evidence. Do not restore unrestricted
shell/LSP or automatic MCP exposure merely to make a research task convenient.

## Delivery evidence (2026-09-14)

The runtime change is implemented. Scoped checks passed: 115 core/config/permission
checks, 10 bootstrap checks, 14 resume checks, and 41 native task/batch/metadata
checks. Formatting, workspace compilation, Clippy, static test gates and
`git diff --check` pass. Simulation passed all seven stages. Real Harness PTY
captures in Chromium/xterm.js passed at 120×40 and 80×24, with native tool events
and filesystem effects verified.

The final full workspace run had **4,613 passes and three failures** (11 skipped).
All three config-dependent failures reproduce in an isolated baseline with the
original dirty worktree and without this plan's changes. The user approved the
three-file fixture scope extension on 2026-09-14. Those updates are applied and
all three affected tests pass in the working tree. The general composed-prompt
snapshot was regenerated with `HARNESS_UPDATE_PROMPT_SNAPSHOTS=1` and then
verified without that flag. No new full-suite failures remain.

The quality lane's remaining branding failure is reproduced on the original plan
and index reference names. Its static test-suite gate now passes. New coordinator
gate tests live as entry points in `coord/tests.rs` to respect the existing test-file
line limit; the baseline compaction-test removal in that file is preserved.

Detailed logs, baseline comparison, the approved fixture patch and visual proof:
[verification report](/home/urbanbreach/.codex/visualizations/2026/09/14/01a0a146-2adf-7951-b8ba-89b2ae28db92/role-scoped-subagents/verification.md).

## Research-access follow-up verification (2026-09-14)

The requested broader research tool access is implemented. Core default/permission
checks passed (36), the scoped CLI/tool suites passed (574), all seven simulation
stages passed, and real xterm.js captures passed at 120×40 and 80×24. The final
workspace suite reports 4,613 passes, the same three pre-existing failures, and
11 skips. Formatting, Clippy and static test gates pass; the original branding
gate remains failing. See the [follow-up evidence](/home/urbanbreach/.codex/visualizations/2026/09/14/01a0a146-2adf-7951-b8ba-89b2ae28db92/research-tool-parity/verification.md).
