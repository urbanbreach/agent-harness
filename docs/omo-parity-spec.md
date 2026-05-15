# Oh My OpenAgent parity specification

This document is the recommended specification for bringing Agent Harness to
feature parity with the local Oh My OpenAgent reference under
`inspirations/oh-my-openagent/`.

Parity does not mean copying the TypeScript plugin architecture. Harness should
reach the same or better operator outcomes through its Rust-native,
event-sourced runtime. The implementation must preserve the core Harness
contract: the coordinator owns event append, scheduling, permission resolution,
tool execution re-entry, compaction, and run/agent lifecycle; replay stays pure
and side-effect free.

## Scope

This specification covers everything required for practical OMO parity:

- [ ] specialist agents, model routing, category routing, and fallback policy;
- [ ] planning and execution orchestration;
- [ ] continuation loops, todo/task persistence, and idle recovery;
- [ ] team mode, shared work coordination, worktrees, and tmux visualization;
- [ ] native tools, AST-grep, LSP, browser/media tools, session tools, and terminal
  tools;
- [ ] skills, skill-embedded MCP, built-in MCPs, and OAuth MCP;
- [ ] hooks, commands, context injection, recovery, diagnostics, and compatibility;
- [ ] provider breadth, model capabilities, performance evidence, and test gates.

## Evidence baseline

Current Harness evidence:

- [ ] `README.md` documents the shipped `build`, `plan`, `discipline`, `explore`,
  `general`, category profiles, Plan workflow, stress harness, and session
  lineage commands.
- [ ] `docs/architecture.md` defines the event schema, coordinator invariants,
  task/background notification flow, team events, permission model, tool surface
  policy, compaction, and replay contract.
- [ ] `docs/config.md` defines the public runtime config, Plan operator workflow,
  category profiles, task/background output metadata, permissions, MCP, and
  compaction knobs.
- [ ] `docs/inspiration-gap-analysis.md` ranks broader inspiration gaps and should
  remain the high-level intake document.
- [ ] `docs/plan-agent-gap-spec.md` is the focused Plan-mode gap document.
- [ ] `crates/harness-core/src/config/public.rs` defines shipped agent profiles and
  category routing profiles.
- [ ] `crates/harness-core/src/coord.rs` owns scheduler, permissions, agent turns,
  tools, compaction, background wakeups, and team commands.
- [ ] `crates/harness-tools/src/lib.rs` registers the current native tool surface.
- [ ] `crates/harness-tools/src/agent_ops.rs` implements `task`,
  `background_output`, and `batch` orchestration.
- [ ] `crates/harness-tools/src/team_ops.rs` exposes the current team tools.

OMO reference evidence:

- [ ] `inspirations/oh-my-openagent/AGENTS.md` summarizes the plugin architecture:
  agents, hooks, tools, feature modules, MCP tiers, and initialization.
- [ ] `inspirations/oh-my-openagent/docs/reference/features.md` lists agents,
  categories, skills, commands, tools, hooks, MCPs, model capabilities, context
  injection, and Claude Code compatibility.
- [ ] `inspirations/oh-my-openagent/docs/guide/orchestration.md` describes the
  Prometheus/Metis/Momus planning layer, Atlas execution layer, Sisyphus-Junior
  workers, category+skill routing, continuation reminders, and wisdom
  accumulation.
- [ ] `inspirations/oh-my-openagent/docs/guide/team-mode.md` describes declared
  teams, member eligibility, lifecycle, 12 team tools, bounds, worktrees, tmux
  visualization, diagnostics, and storage layout.

## Current parity status

| Area | Current Harness status | Parity state |
| --- | --- | --- |
| Event-sourced coordinator | Coordinator-owned events, scheduling, permissions, tools, compaction, replay | Stronger than OMO plugin baseline |
| Native `task` and background output | Sync/background child turns, skill injection, cancellation, continuation | Mostly present |
| Category names | OMO category names are shipped as profiles | Present but model fallback is partial |
| Plan workflow | `build` -> `plan` via `plan_enter`; `plan` -> `build` via `plan_exit` | Present, with stricter safety |
| Discipline workflow | Prompt-profile behavior only | Partial |
| Specialist agents | `build`, `plan`, `discipline`, `explore`, `general`, category profiles | Major gap |
| Continuation loops | No Ralph/ulw loop, no todo continuation enforcer | Major gap |
| Team mode | Event-sourced team run, members, messages, tasks, shutdown, delete | Partial MVP |
| Tools | Read/search/edit/bash/web/LSP/MCP/task/team/skill/question/todos | Partial |
| AST-grep | Not first-class | Gap |
| Interactive terminal | Bash exists, but no tmux `interactive_bash` user tool | Gap |
| Browser automation | No Playwright/agent-browser/dev-browser skill bundle | Gap |
| Media analysis | No `look_at`/multimodal-looker surface | Gap |
| Session tools for agents | CLI/TUI sessions exist, but no model-visible session tools | Gap |
| Persistent task system | `todowrite` plus team tasks; no general `task_create/list/get/update` | Gap |
| Skills | `.agent-harness/skills`, `.harness/skills`, global harness skills | Partial |
| Skill MCP | Config-backed MCP exists; no skill-scoped MCP lifecycle/tool | Major gap |
| Built-in MCPs | Generic MCP support, no bundled Exa/Context7/Grep.app profiles | Gap |
| Hooks | Lifecycle command hooks | Partial |
| Slash commands | TUI workflow commands; no file/template slash command system | Gap |
| Context injection | AGENTS.md and instructions in prompts | Partial |
| Provider breadth | Mock plus OpenAI-compatible | Partial |
| Model fallback | Model profiles can list fallback, but runtime fallback is not OMO-level | Gap |
| Doctor | Config/tool/profile/session/MCP checks | Partial |
| Claude Code/OpenCode compatibility | Some config compatibility, active product areas rejected | Partial by design |

## Design goals

1. [ ] **Harness-native parity.** Match the operator behavior of OMO while keeping
   the implementation inside Harness modules and event contracts.
2. [ ] **Deep modules, small interfaces.** Add seams that hide complexity behind
   stable interfaces: extension registration, hook execution, agent routing,
   skill bundles, MCP sessions, and terminal sessions.
3. [ ] **Event-derived state.** State needed for resume, replay, TUI, audit, or
   background wakeups must be persisted through events or redacted artifacts.
4. [ ] **No side effects during replay.** Hooks, extensions, MCPs, terminal sessions,
   provider calls, and shell commands must never execute during replay.
5. [ ] **Compatibility through adapters.** OpenCode/Claude/OMO compatibility should
   be import, translation, or wrapper adapters. It should not turn Harness into a
   pass-through plugin host before the extension safety seam exists.
6. [ ] **Observable completion.** Each parity item is done only when it has config,
   tool/profile exposure, event/artifact behavior, CLI/TUI/operator visibility,
   tests, and documentation.

## Non-goals

- [ ] Do not load arbitrary OpenCode plugins until Harness has a coordinator-owned
  extension seam, command mediation, permission gating, and replay-safe
  manifests.
- [ ] Do not add hidden infinite loops. Continuation must be explicit, bounded,
  stoppable, and event-visible.
- [ ] Do not add broad compatibility aliases that bypass the canonical native tool
  surface. Aliases are acceptable only as thin wrappers that resolve to canonical
  tool ids and preserve permission checks.
- [ ] Do not persist raw provider payloads, secrets, hidden thinking, raw MCP tokens,
  or unredacted browser/session content in events.
- [ ] Do not weaken Plan safety to mimic OMO. Plan may gain capabilities only behind
  parent-permission inheritance and coordinator validation.

## Required architecture seams

These seams should be implemented before or alongside the feature areas that
depend on them. They provide locality and leverage.

### 1. Extension registration seam

**Purpose:** Register optional tools, hooks, commands, prompts, MCP bundles,
diagnostic checks, and provider decorators without widening the coordinator
interface for every feature.

**Recommended shape:**

- [ ] A manifest-first module in `harness-core` that describes extension-provided
  tool ids, schemas, permission kinds, hook interests, config schema fragments,
  prompt assets, command templates, and artifact contracts.
- [ ] Rust in-process adapters first. Executable or scripting adapters later, only
  after command mediation and sandbox evidence exist.
- [ ] Registration returns declarative data. Runtime callbacks must re-enter the
  coordinator through commands, not append events directly.
- [ ] Disable/unload must remove live tools and hooks while old event logs remain
  replayable.

**Acceptance criteria:**

- [ ] Extension tools are visible in doctor, config schema, and tool registry.
- [ ] Permission denial blocks extension tools before adapter execution.
- [ ] Replay can render old extension events without loading extension code.
- [ ] Tests cover enable, disable, permission deny, artifact persistence, and replay.

### 2. Hook middleware seam

**Purpose:** Replace ad hoc lifecycle command hooks with a typed middleware
interface that can support OMO-like pre-tool, post-tool, message, transform,
params, event, and compaction hooks.

**Recommended shape:**

- [ ] Define hook phases in `harness-core` around existing coordinator phases:
  `message_received`, `agent_turn_started`, `provider_params`,
  `provider_context_transform`, `tool_preflight`, `tool_result`,
  `agent_turn_finished`, `session_idle`, and `compaction_requested`.
- [ ] Hook results are typed: allow, deny, modify model-facing context, request a
  reminder, write a redacted artifact, or add diagnostics.
- [ ] Critical hook failure can cancel the current operation only through coordinator
  events.
- [ ] Hook output must be capped and redacted before persistence.

**Acceptance criteria:**

- [ ] Built-in lifecycle hooks migrate onto the same seam.
- [ ] Tool guard hooks can block a tool before permission resolution or before
  execution, with a durable reason.
- [ ] Transform hooks can modify provider-visible context without mutating events.
- [ ] Replay sees hook events and artifacts but never executes hooks.

### 3. Agent catalog and routing seam

**Purpose:** Make specialist agents, categories, model variants, fallback chains,
and tool restrictions first-class instead of spreading routing rules through
config defaults and task code.

**Recommended shape:**

- [ ] Add a resolved `AgentCatalog` projection from config, prompt assets, skills,
  model catalog, and compatibility imports.
- [ ] Each catalog entry includes role, mode, default tools, permission profile,
  model target, fallback chain, category binding, prompt appenders, display
  order, and hidden/primary/subagent flags.
- [ ] `task(category=...)` should resolve through the catalog and record the chosen
  route, model, variant, fallback policy, and skill bundle in task metadata.

**Acceptance criteria:**

- [ ] Doctor reports every primary, specialist, and category route with model and
  fallback status.
- [ ] TUI model/agent picker uses the same catalog.
- [ ] Task results expose resolved catalog metadata.
- [ ] Config drift tests fail if shipped OMO-profile contracts disappear.

### 4. Skill bundle seam

**Purpose:** Turn skills into bundles that can carry instructions, tools, MCP
servers, permissions, command templates, and verification guidance.

**Recommended shape:**

- [ ] Extend `SKILL.md` frontmatter to include MCP definitions, tool permissions,
  command templates, environment policy, and optional verification hooks.
- [ ] Keep skill loading explicit through `skill` or `task(load_skills=[...])`.
- [ ] Skill-provided MCP servers are session-scoped and visible only through
  `skill_mcp` or first-class tool ids granted by the loaded skill.

**Acceptance criteria:**

- [ ] Skill content and MCP availability are injected only for the intended turn or
  child task.
- [ ] Skill MCP sessions clean up on idle, cancellation, and run finish.
- [ ] Skill permissions cannot broaden a statically denied profile permission.
- [ ] Doctor can show skill discovery roots and denied/invalid skills without
  loading MCP servers.

### 5. Terminal session seam

**Purpose:** Support persistent tmux-like interactive terminal sessions without
using one-shot bash as an interactive surrogate.

**Recommended shape:**

- [ ] A `terminal_session` module in `harness-tools` backed by tmux or portable-pty
  adapters.
- [ ] Tools: `interactive_bash`, `terminal_spawn`, `terminal_write`,
  `terminal_screenshot`, `terminal_resize`, `terminal_kill`, and
  `terminal_list` can be exposed progressively.
- [ ] Session ids, pane/window metadata, screenshots, and captured text become
  redacted artifacts and event summaries.
- [ ] Plan mode may inspect terminal output only if shell policy and Plan guard allow
  it.

**Acceptance criteria:**

- [ ] Interactive sessions survive across tool calls within a run.
- [ ] A TUI app can be launched, driven, captured, and killed through tools.
- [ ] Missing tmux/pty support yields actionable errors and doctor warnings.
- [ ] PTY signoff covers happy path, bad command, and cleanup.

## Parity workstreams

### A. Specialist agents and orchestration roles

**Target parity:** OMO exposes Sisyphus, Hephaestus, Prometheus, Oracle,
Librarian, Explore, Multimodal-Looker, Metis, Momus, Atlas, and
Sisyphus-Junior.

**Recommended Harness outcome:**

- [ ] Keep existing `build`, `plan`, and `discipline` as stable Harness names.
- [ ] Add OMO names as first-class profiles or aliases with explicit role metadata:
  - [ ] `sisyphus`: primary orchestrator, todo-driven, may plan/delegate/verify;
  - [ ] `hephaestus`: autonomous deep worker, close to current `build`/`discipline`;
  - [ ] `prometheus`: read-only strategic planner, mapped to Plan workflow;
  - [ ] `metis`: read-only pre-plan consultant;
  - [ ] `momus`: read-only plan/work reviewer;
  - [ ] `atlas`: execution orchestrator that reads plans, delegates implementation,
    and verifies results but does not edit code directly;
  - [ ] `sisyphus-junior`: category executor with no redelegation;
  - [ ] `oracle`: read-only architecture/debugging/review consultant;
  - [ ] `librarian`: read-only docs/remote-code research agent;
  - [ ] `explore`: read-only local codebase search agent;
  - [ ] `multimodal-looker`: media analysis specialist.
- [ ] Preserve tool restrictions as runtime policy, not just prompt text.
- [ ] Implement deterministic primary-agent display ordering for TUI cycling.

**Dependencies:** Agent catalog seam, model fallback policy, dynamic prompt
builder, permission profile inheritance.

**Acceptance criteria:**

- [ ] `harness doctor` reports all OMO agent names, roles, tools, permissions,
  model targets, and fallback status.
- [ ] `task(subagent_type="oracle")`, `task(subagent_type="librarian")`, and
  `task(subagent_type="multimodal-looker")` resolve without config overrides.
- [ ] Read-only agents cannot edit, run mutating shell commands, or redelegate.
- [ ] TUI can select primary agents in stable order without showing hidden agents.
- [ ] Existing `build`, `plan`, and `discipline` remain valid public profiles.

### B. Category and model routing

**Target parity:** OMO categories combine model, variant, prompt append, tools,
reasoning, and skills. Built-ins are `visual-engineering`, `ultrabrain`, `deep`,
`artistry`, `quick`, `unspecified-low`, `unspecified-high`, and `writing`.

**Recommended Harness outcome:**

- [ ] Keep the current category names.
- [ ] Move category resolution into the agent catalog.
- [ ] Add per-category fallback chains and model capability validation.
- [ ] Add category-level prompt append, tool allow/deny, reasoning effort, text
  verbosity, max output, and unstable-agent behavior.
- [ ] Record the resolved category route in `TaskCompletionMetadata` or child task
  metadata.

**Acceptance criteria:**

- [ ] `task(category="visual-engineering", load_skills=[...])` records category,
  selected profile, model ref, variant, fallback chain, and loaded skills.
- [ ] Unknown category falls back only when config explicitly allows fallback.
- [ ] Category tool restrictions cannot exceed parent/runtime permission policy.
- [ ] Doctor warns when a category model lacks tool-call support or required
  modality.

### C. Prometheus, Metis, Momus, and plan review

**Target parity:** OMO has a planning layer where Prometheus interviews, Metis
finds hidden gaps, and Momus reviews plans until they meet quality thresholds.

**Recommended Harness outcome:**

- [ ] Extend current Plan workflow rather than creating a separate planning store.
- [ ] `prometheus` writes under `.agent-harness/plans/` and can ask questions,
  launch read-only `explore`/`librarian`, consult `metis`, and optionally submit
  to `momus`.
- [ ] `metis` and `momus` are read-only profiles with structured response contracts.
- [ ] Plan files should include context, task dependency graph, parallel execution
  waves, category+skill recommendations, acceptance criteria, and verification
  strategy.
- [ ] `plan_exit` should pass the plan file path and a concise execution directive to
  `build`, `sisyphus`, or `atlas` based on user approval.

**Acceptance criteria:**

- [ ] Plan mode can request Metis/Momus review without write-capable subagents.
- [ ] Momus review status is stored as event/artifact metadata or plan-file section.
- [ ] A rejected plan can be revised without losing the same active plan path.
- [ ] Tests cover Plan subagent restrictions and plan approval handoff.

### D. Atlas execution and wisdom accumulation

**Target parity:** Atlas reads a plan, decomposes tasks, accumulates learnings,
delegates implementation, verifies results, and reports final status.

**Recommended Harness outcome:**

- [ ] Add `atlas` as an orchestration-only profile.
- [ ] Store Atlas learnings in event-derived artifacts under the run, not in a hidden
  `.sisyphus/notepads` tree. Recommended artifacts:
  - [ ] `artifacts/orchestration/<plan>/learnings.md`;
  - [ ] `artifacts/orchestration/<plan>/decisions.md`;
  - [ ] `artifacts/orchestration/<plan>/issues.md`;
  - [ ] `artifacts/orchestration/<plan>/verification.md`.
- [ ] Expose read-only projection summaries to later child tasks through child prompt
  metadata.

**Acceptance criteria:**

- [ ] Atlas cannot edit implementation files directly unless explicitly reconfigured.
- [ ] Delegated child prompts include accumulated learnings and constraints.
- [ ] Verification failures cause additional task delegation or a final blocked
  report, not a false success.
- [ ] Replay can show Atlas decisions without executing child tasks.

### E. Continuation controller, ultrawork, and Ralph loop

**Target parity:** OMO supports `ultrawork`/`ulw`, `/ulw-loop`, `/ralph-loop`,
todo continuation, unstable-agent babysitting, and `/stop-continuation`.

**Recommended Harness outcome:**

- [ ] Add a coordinator-owned `ContinuationController` module.
- [ ] Continuation is explicit: activated by a command/tool, bounded by config, and
  visible in events.
- [ ] Add events such as `ContinuationStarted`, `ContinuationReminderQueued`,
  `ContinuationStopped`, and `ContinuationLimitReached` only if existing task
  events cannot express the state clearly.
- [ ] Add commands/tools:
  - [ ] `/ralph-loop` starts goal continuation with done-marker detection;
  - [ ] `/ulw-loop` starts Ralph plus ultrawork mode settings;
  - [ ] `/stop-continuation` stops all continuation mechanisms for the run;
  - [ ] `ultrawork` keyword or command activates the orchestrator mode for one turn
    or one explicit loop.
- [ ] Todo continuation should use the same task/todo projection as the TUI, not a
  separate memory-only hook.

**Safety requirements:**

- [ ] Every loop has max iterations, max wall-clock, max provider calls, and max tool
  calls.
- [ ] User interruption and `/stop-continuation` take priority over reminders.
- [ ] Reminders are persisted and replay-rendered, but replay never schedules them.
- [ ] Continuation is disabled in Plan unless a Plan-specific reviewed flow is added.

**Acceptance criteria:**

- [ ] A loop can start, continue after an idle turn, stop, hit max iterations, and
  resume after process restart.
- [ ] TUI shows active continuation state and stop action.
- [ ] Deterministic tests cover done-marker detection, incomplete todos, stop, and
  limit reached.

### F. Persistent task system

**Target parity:** OMO has `task_create`, `task_get`, `task_list`, and
`task_update` for persistent dependency-aware tasks.

**Recommended Harness outcome:**

- [ ] Keep `todowrite` as the lightweight per-session visible checklist.
- [ ] Add a separate persistent task module for dependency-aware work items.
- [ ] Prefer event-sourced task state over ad hoc JSON files. If file artifacts are
  needed for human editing, derive them from events or treat them as artifacts.
- [ ] Use schema fields compatible with OMO/Claude naming: `subject`, `description`,
  `status`, `active_form`, `blocked_by`, `blocks`, `owner`, `metadata`, and
  `thread_id`/`run_id`.
- [ ] Integrate with Atlas, Team Mode, and continuation controller.

**Acceptance criteria:**

- [ ] Task dependency projection computes `blocks` from `blocked_by` deterministically.
- [ ] Tasks survive restart and replay.
- [ ] Parallel-ready tasks can be surfaced to the orchestrator, but execution remains
  coordinator-owned.
- [ ] `tasks-todowrite-disabler` behavior, if implemented, is a policy option rather
  than a hidden hook.

### G. Team Mode completion

**Target parity:** OMO Team Mode includes declared teams, 12 tools, shared
mailbox, shared task list, file-locked claims, worktrees, tmux layout,
diagnostics, and active runtime directories.

**Current Harness state:** Harness has event-sourced `team_create`,
`team_status`, `team_send_message`, `team_task_create/list/get/update`,
`team_shutdown_request/approve/reject`, and `team_delete`. Missing pieces include
`team_list`, declared team registry, durable mailbox artifacts, file claims,
worktrees, tmux visualization, and team doctor checks.

**Recommended Harness outcome:**

- [ ] Add `team_list` for declared and active teams.
- [ ] Add declared team specs under `.agent-harness/teams/<name>.json` and user
  equivalents under the XDG Harness config directory.
- [ ] Keep active team state event-sourced. Use artifacts for large mailbox bodies or
  delivery diagnostics, not as the source of truth.
- [ ] Add optional per-member worktrees through a `TeamWorktreeAdapter`. Worktree
  creation is permission-gated and never automatic for read-only research roles.
- [ ] Add file claims or reservations as team events if they affect coordination.
- [ ] Add optional tmux visualization through the terminal session seam. Missing tmux
  must warn, not block team creation.

**Acceptance criteria:**

- [ ] `team_list` shows declared teams and active runs.
- [ ] Declared team specs validate lead/member eligibility before spawning.
- [ ] Worktree path validation rejects bare branch names, traversal, and unsafe
  external paths unless explicitly allowed.
- [ ] Tmux visualization starts, rebalances, and cleans up panes without changing the
  underlying team state if tmux fails.
- [ ] Doctor reports team spec count, active team count, git/tmux availability, and
  stale runtime artifacts.

### H. Tool parity

**Target parity:** OMO exposes native tools for search, edits, LSP, AST-grep,
delegation, visual analysis, skills, session history, task management, and
interactive terminal use.

**Recommended Harness additions:**

1. [ ] **AST-grep tools**
   - [ ] Add `ast_grep_search` and `ast_grep_replace` as first-class tools.
   - [ ] Use a Rust adapter or CLI adapter with strict argument schemas.
   - [ ] Dry-run replace by default.
   - [ ] Persist large results as artifacts.

2. [ ] **Delegation aliases**
   - [ ] Consider `call_omo_agent` as a compatibility wrapper for direct
     `task(subagent_type=...)` calls to `oracle`, `librarian`, and `explore`.
   - [ ] Keep `task` canonical.

3. [ ] **Background cancellation**
   - [ ] Add `background_cancel` as a compatibility wrapper around
     `background_output(cancel=true, ...)`.
   - [ ] Support individual cancellation; avoid global cancel unless scoped to the
     current parent run.

4. [ ] **Visual analysis**
   - [ ] Add `look_at` backed by `multimodal-looker`.
   - [ ] Accept workspace files and explicitly provided image data.
   - [ ] Store extracted text and media summaries as artifacts when large.

5. [ ] **Session tools**
   - [ ] Add model-visible `session_list`, `session_read`, `session_search`, and
     `session_info` tools over Harness session logs.
   - [ ] Reuse replay/transcript projections.
   - [ ] Never execute tools while reading sessions.

6. [ ] **Persistent task tools**
   - [ ] Add `task_create`, `task_get`, `task_list`, and `task_update` as described
     in workstream F.

7. [ ] **Interactive terminal tools**
   - [ ] Add `interactive_bash` or the broader terminal session toolset described in
     the terminal session seam.

8. [ ] **Skill MCP tool**
   - [ ] Add `skill_mcp` for skill-scoped MCP operations.

**Acceptance criteria:**

- [ ] Every new tool has strict JSON schema, capability mapping, permission policy,
  artifact behavior, registry tests, docs, and prompt examples.
- [ ] Tool failures return actionable structured errors.
- [ ] Tool ids are included in `native_tool_parity_matrix`-style coverage.

### I. Browser automation and media workflows

**Target parity:** OMO provides browser automation through Playwright MCP,
agent-browser CLI, and dev-browser, plus `look_at` for images/PDFs.

**Recommended Harness outcome:**

- [ ] Implement browser automation as skills first, not as always-on global tools.
- [ ] Built-in skills:
  - [ ] `playwright`: launches a Playwright MCP server through skill-embedded MCP;
  - [ ] `agent-browser`: wraps the agent-browser CLI when installed;
  - [ ] `dev-browser`: supports persistent browser state for iterative work.
- [ ] Add browser capability diagnostics to doctor.
- [ ] Add media extraction through `look_at` and `multimodal-looker`.

**Acceptance criteria:**

- [ ] A visual-engineering task can load `frontend-ui-ux` and `playwright`, open a
  page, interact, screenshot, and report evidence.
- [ ] Browser artifacts are written under session artifacts with redacted metadata.
- [ ] Missing browser dependencies produce doctor warnings and tool errors, not
  panics.
- [ ] Live/browser lanes are environment-gated.

### J. Skills and built-in skills

**Target parity:** OMO ships skills such as `git-master`, browser skills,
`frontend-ui-ux`, `review-work`, `ai-slop-remover`, and `team-mode`; custom
skills can come from OpenCode, Claude, Agents, and user paths.

**Recommended Harness outcome:**

- [ ] Extend discovery order to include:
  1. [ ] project `.agent-harness/skills/*/SKILL.md`;
  2. [ ] project `.opencode/skills/*/SKILL.md`;
  3. [ ] project `.claude/skills/*/SKILL.md`;
  4. [ ] project `.agents/skills/*/SKILL.md`;
  5. [ ] user Harness, OpenCode, Claude, and Agents skill directories.
- [ ] Keep Harness-owned paths first unless an explicit compatibility mode says
  otherwise.
- [ ] Add built-in skill packs:
  - [ ] `git-master`;
  - [ ] `playwright`, `agent-browser`, `dev-browser`;
  - [ ] `frontend-ui-ux`;
  - [ ] `review-work`;
  - [ ] `ai-slop-remover`;
  - [ ] `team-mode` usage documentation.
- [ ] Support frontmatter fields for MCP, permissions, tools, commands, and
  environment allowlists.

**Acceptance criteria:**

- [ ] Skill discovery reports visible, denied, invalid, and shadowed skills.
- [ ] `task(load_skills=[...])` injects skill content and activates allowed skill
  tools only for the child session.
- [ ] Built-in skills have docs, tests, and config disable switches.

### K. MCP parity

**Target parity:** OMO has three MCP tiers: built-in remote MCPs,
Claude/OpenCode `.mcp.json` compatibility, and skill-embedded MCPs with OAuth.

**Recommended Harness outcome:**

- [ ] Keep current config-backed MCP server support.
- [ ] Add bundled MCP profiles for:
  - [ ] Exa/Tavily web search;
  - [ ] Context7 documentation lookup;
  - [ ] Grep.app/GitHub code search.
- [ ] Add `.mcp.json` compatibility loader as a translation adapter, with explicit
  env variable expansion policy.
- [ ] Add skill-embedded MCP session management.
- [ ] Add OAuth 2.1 support with PKCE, dynamic registration when available,
  protected-resource discovery, token refresh, and secure user-token storage.

**Acceptance criteria:**

- [ ] MCP discovery never blocks deterministic tests unless explicitly enabled.
- [ ] Skill MCP servers are started on demand, scoped, cleaned up, and visible in
  doctor status.
- [ ] OAuth tokens are never stored in session events or artifacts.
- [ ] MCP tool calls use canonical permissions based on transport and declared
  capability.

### L. Hook ecosystem

**Target parity:** OMO has hooks for context injection, productivity/control,
quality/safety, recovery/stability, truncation, notifications, task management,
continuation, integration, and specialized agent guardrails.

**Recommended Harness built-ins:**

- [ ] **Context hooks**: AGENTS.md injection, README injection, conditional rules,
  compaction context preservation, context-window monitor, pre-prompt
  compaction.
- [ ] **Productivity hooks**: keyword detector, think-mode, category/skill reminder,
  auto slash command handler.
- [ ] **Quality hooks**: comment checker, write-existing-file guard, hashline read
  enhancer, hashline diff enhancer, webfetch redirect guard, bash file-read guard.
- [ ] **Recovery hooks**: session recovery, missing tool result recovery, thinking
  block validation, JSON error recovery, runtime fallback, model fallback,
  edit-error recovery.
- [ ] **Truncation hooks**: dynamic tool-output truncation for grep/glob/LSP/AST-grep
  and large MCP outputs.
- [ ] **Notification hooks**: background completion notification, session idle
  notification, agent usage reminder.
- [ ] **Task hooks**: task resume info, delegate-task retry, empty task response
  detector, TodoWrite/task-system interaction policy.
- [ ] **Continuation hooks**: todo continuation enforcer, compaction todo preserver,
  unstable-agent babysitter.
- [ ] **Specialized hooks**: Prometheus markdown-only, model-family guardrails for
  specialist agents, Sisyphus-Junior notepad/artifact management.

**Acceptance criteria:**

- [ ] Hooks are individually disableable by stable id.
- [ ] Hook effects are visible in events, artifacts, or provider-context metadata.
- [ ] Critical hook failure behavior is deterministic and tested.
- [ ] Hook output truncation is context-aware and redacted.

### M. Slash command system

**Target parity:** OMO supports built-in commands and custom command discovery.

**Recommended Harness outcome:**

- [ ] Add a command registry distinct from TUI-only slash commands.
- [ ] Commands are templates that can:
  - [ ] load a prompt;
  - [ ] load skills;
  - [ ] request a profile switch;
  - [ ] call a native tool;
  - [ ] start continuation;
  - [ ] create a plan or handoff artifact.
- [ ] Built-in commands:
  - [ ] `/init-deep`;
  - [ ] `/ralph-loop`;
  - [ ] `/ulw-loop`;
  - [ ] `/cancel-ralph`;
  - [ ] `/refactor`;
  - [ ] `/start-work`;
  - [ ] `/stop-continuation`;
  - [ ] `/remove-ai-slops`;
  - [ ] `/handoff`;
  - [ ] `/hyperplan` if team/parallel planning support is present.
- [ ] Custom command roots should include Harness, OpenCode, and Claude-compatible
  locations.

**Acceptance criteria:**

- [ ] Commands are listed in TUI and doctor.
- [ ] Unknown/disabled commands produce actionable errors.
- [ ] Command execution re-enters the coordinator and is recorded.
- [ ] Custom command templates cannot execute shell code without a tool permission.

### N. Context injection and dynamic prompts

**Target parity:** OMO injects AGENTS.md, README.md, conditional rules, skills,
category guidance, model-family guidance, and compaction context.

**Recommended Harness outcome:**

- [ ] Deepen the prompt builder module so it composes:
  - [ ] base agent prompt;
  - [ ] profile role and tool rules;
  - [ ] project instructions;
  - [ ] directory-scoped AGENTS.md;
  - [ ] README snippets when useful;
  - [ ] active skills;
  - [ ] category+skill guidance;
  - [ ] model-family protocol reminders;
  - [ ] compaction recap and operational memory;
  - [ ] Plan/continuation/team mode reminders.
- [ ] Prompt inputs should be traceable in provider metadata by digest and source
  label, not raw full text unless already visible.

**Acceptance criteria:**

- [ ] Prompt assembly is deterministic for the same event/config state.
- [ ] Directory instructions are injected for relevant file operations without
  polluting unrelated turns.
- [ ] Tests cover precedence, deduplication, and redaction.

### O. Provider, model capability, and fallback parity

**Target parity:** OMO has per-agent fallback chains, model capability diagnostics,
runtime fallback on retryable errors, and provider/model option tuning.

**Recommended Harness outcome:**

- [ ] Add runtime consumption of resolved model fallback chains.
- [ ] Add provider error classification: auth, rate limit, overload, context window,
  malformed stream, unsupported tool, and transport failure.
- [ ] Add fallback cooldowns and per-run fallback telemetry.
- [ ] Add model capability cache from the generated model catalog and optional
  models.dev refresh.
- [ ] Add provider-native transports beyond OpenAI-compatible only after each has
  fixture and live signoff coverage.

**Acceptance criteria:**

- [ ] Doctor reports effective model resolution for every profile/category and warns
  about unsupported tools/modalities.
- [ ] Runtime fallback switches model only on classified retryable failures and records
  the reason.
- [ ] Fallback never changes replay semantics.
- [ ] Provider-specific details remain in `harness-providers` adapters.

### P. Recovery and session repair

**Target parity:** OMO recovers from missing tool results, thinking block errors,
empty messages, context-window failures, JSON parse errors, and session errors.

**Recommended Harness outcome:**

- [ ] Keep replay pure, but add explicit recovery inspection and repair commands for
  operator-approved session repair.
- [ ] Add provider-context validators before sending model input.
- [ ] Add recovery paths for malformed tool-result content and unsupported provider
  tool-call formats.
- [ ] Add context-window overflow recovery through existing compaction retry path and
  model fallback only when configured.

**Acceptance criteria:**

- [ ] `harness sessions inspect` reports recovery issues and suggested repair actions.
- [ ] Automatic recovery never rewrites `events.jsonl` silently.
- [ ] Repair commands write new events or copied child sessions, not in-place edits.

### Q. Compatibility surfaces

**Target parity:** OMO loads Claude Code/OpenCode agents, commands, skills,
hooks, MCPs, and plugins.

**Recommended Harness outcome:**

- [ ] Support compatibility in this order:
  1. [ ] import agents as Harness profiles;
  2. [ ] import skills as Harness skills;
  3. [ ] import commands as command templates;
  4. [ ] import `.mcp.json` as MCP server config;
  5. [ ] import safe hook subsets as typed hooks;
  6. [ ] only then consider plugin manifests.
- [ ] Unsupported active plugin/server/share/autoupdate behavior should remain
  rejected until the extension seam can enforce safety.

**Acceptance criteria:**

- [ ] Compatibility imports are visible in doctor with source path and enabled state.
- [ ] Imported items can be disabled individually.
- [ ] Import errors do not abort startup unless the item is explicitly required.

### R. Diagnostics and doctor

**Target parity:** OMO doctor checks registration, config, models, environment,
team mode, MCPs, capabilities, and compatibility warnings.

**Recommended Harness outcome:**

- [ ] Expand `harness doctor` with checks for:
  - [ ] agent catalog completeness;
  - [ ] category/model fallback health;
  - [ ] provider credential status;
  - [ ] model tool/modality capability;
  - [ ] skill discovery and skill MCP readiness;
  - [ ] built-in MCP configuration;
  - [ ] browser dependencies;
  - [ ] tmux/pty/git availability;
  - [ ] team spec/runtime health;
  - [ ] continuation state;
  - [ ] hook registry and disabled hook ids;
  - [ ] compatibility imports;
  - [ ] session directory/index health;
  - [ ] performance evidence freshness.

**Acceptance criteria:**

- [ ] `doctor --json` exposes stable machine-readable check ids.
- [ ] Text output is concise and actionable.
- [ ] No doctor check performs provider/MCP/browser network calls unless explicitly
  requested.

### S. TUI and operator UX

**Target parity:** OMO adds agent ordering, background notifications, tmux panes,
commands, status, toggles, session tools, and rich workflow affordances.

**Recommended Harness outcome:**

- [ ] TUI agent picker uses resolved agent catalog order.
- [ ] TUI status dialog shows provider, model, active continuation, team runs, hooks,
  MCP servers, skill state, and browser/terminal availability.
- [ ] TUI toggles can enable/disable agents, skills, tools, hooks, MCP servers, and
  continuation mode for the session when config allows it.
- [ ] Background and team notifications link to `background_output`, session child
  navigation, or team status.
- [ ] Terminal/tmux panes are optional and degrade gracefully.

**Acceptance criteria:**

- [ ] PTY signoff covers agent switching, status, toggles, background notification,
  and session navigation.
- [ ] Native visual signoff covers diff display, command/status dialogs, and team
  status when available.

### T. Security and permission expansion

**Target parity:** OMO permissions include edit, bash, webfetch, task,
external-directory, doom-loop, and per-agent policies; hooks guard risky tools.

**Recommended Harness outcome:**

- [ ] Extend permission kinds only when there is a real capability seam:
  - [ ] `terminal` for persistent interactive sessions;
  - [ ] `browser` for browser automation;
  - [ ] `mcp` or per-MCP capability if transport policy is insufficient;
  - [ ] `continuation` for loops;
  - [ ] `external_directory` for explicit outside-workspace access.
- [ ] Add shell command mediation with dangerous-pattern classification and Plan-mode
  read-only guard reuse.
- [ ] Add compatibility permission translation from Claude/OpenCode where safe.

**Acceptance criteria:**

- [ ] New capabilities have scalar policy, selector rules where meaningful, tests,
  docs, and TUI permission prompts.
- [ ] Static deny always beats grants and compatibility imports.
- [ ] Durable grants store redacted matchers only.

### U. Performance and evidence governance

**Target parity:** The broader inspiration set tracks performance claims,
session scale, memory, startup, resume, and visual evidence. OMO itself relies on
runtime polish; Harness should exceed it with evidence.

**Recommended Harness outcome:**

- [ ] Add `perf` or `stress-perf` test lane.
- [ ] Track startup time, provider first-token latency, JSONL append latency, replay
  time by event count, resume time, session tree/fork/clone time, compaction
  checkpoint time, tool output artifact spill cost, TUI render cost, and peak RSS.
- [ ] Add a parity evidence ledger in machine-readable form, for example
  `docs/parity-ledger.json` or `configs/parity-ledger.json`.

**Acceptance criteria:**

- [ ] Numeric public claims cite an artifact path and run id.
- [ ] Perf budget changes require reviewed contract edits.
- [ ] CI can run deterministic perf smoke checks without live providers.

## Recommended delivery order

### Phase 0: Ledger and docs foundation

- [ ] Add a machine-readable parity ledger with owner/status/evidence fields.
- [ ] Cross-link this spec from `docs/inspiration-gap-analysis.md` and README once it
  is accepted.
- [ ] Add doctor checks for current known gaps as warnings.

**Exit criteria:** parity status is visible and testable without changing runtime
behavior.

### Phase 1: Agent catalog, categories, and specialist profiles

- [ ] Implement `AgentCatalog`.
- [ ] Add OMO specialist profiles and display ordering.
- [ ] Add per-agent/category fallback metadata to doctor.
- [ ] Add `oracle`, `librarian`, `metis`, `momus`, `atlas`, `hephaestus`,
  `sisyphus`, `sisyphus-junior`, and `multimodal-looker` profile contracts.

**Exit criteria:** all OMO agent names resolve; read-only/write restrictions are
enforced and tested.

### Phase 2: Tool parity core

- [ ] Add AST-grep tools.
- [ ] Add session tools.
- [ ] Add `background_cancel` wrapper.
- [ ] Add persistent task tools.
- [ ] Add `look_at` if multimodal provider support is available, otherwise add the
  profile and a clear unsupported error.

**Exit criteria:** core OMO tool list is available or explicitly unsupported with
doctor warnings.

### Phase 3: Skill bundles and skill MCP

- [ ] Extend skill discovery roots and frontmatter.
- [ ] Add `skill_mcp`.
- [ ] Add built-in skills.
- [ ] Add skill-scoped MCP lifecycle.

**Exit criteria:** a `visual-engineering` child can load `frontend-ui-ux` and
`playwright` and see only the intended skill tools.

### Phase 4: Hook middleware and built-in hooks

- [ ] Add typed hook seam.
- [ ] Port existing lifecycle hooks.
- [ ] Add quality/safety, context, truncation, and recovery hooks.
- [ ] Add compatibility hook import only for safe typed subsets.

**Exit criteria:** hooks can block, transform, truncate, notify, and recover
through coordinator-owned events and artifacts.

### Phase 5: Continuation and orchestration loops

- [ ] Add continuation controller.
- [ ] Add `/ralph-loop`, `/ulw-loop`, `/stop-continuation`.
- [ ] Add todo continuation and unstable-agent babysitter.
- [ ] Add ultrawork keyword/command routing.

**Exit criteria:** bounded continuation survives restart, can be stopped, and is
visible in TUI/replay.

### Phase 6: Team mode completion

- [ ] Add `team_list`.
- [ ] Add declared team registry.
- [ ] Add worktree adapter.
- [ ] Add file claims.
- [ ] Add tmux visualization through terminal session seam.
- [ ] Add team doctor checks.

**Exit criteria:** team mode matches OMO user-visible lifecycle while keeping
Harness state event-sourced.

### Phase 7: Browser, terminal, and media signoff

- [ ] Add terminal session seam and `interactive_bash`.
- [ ] Add browser skills and dependency diagnostics.
- [ ] Add media analysis tooling.
- [ ] Add PTY/browser/live signoff lanes.

**Exit criteria:** agents can drive a TUI app, a web UI, and media analysis
through their matching surfaces with persisted evidence.

### Phase 8: Provider and model fallback depth

- [ ] Add runtime fallback chains.
- [ ] Add provider error classification and cooldowns.
- [ ] Add model capability diagnostics.
- [ ] Add additional native provider adapters only with fixtures and live signoff.

**Exit criteria:** provider/model fallback is observable, testable, and does not
alter replay semantics.

### Phase 9: Compatibility import and extension runtime

- [ ] Add command/skill/agent/MCP compatibility imports.
- [ ] Add safe hook subset imports.
- [ ] Add manifest-only extension registration.
- [ ] Defer executable plugin loading until command mediation and sandbox evidence
  are complete.

**Exit criteria:** compatibility improves operator migration without weakening
Harness safety.

## Definition of parity done

Harness reaches OMO parity when all of the following are true:

- [ ] All OMO specialist agent names resolve through the agent catalog with correct
  tool restrictions, model routing, fallback status, and TUI visibility.
- [ ] `task`, category routing, skill injection, background output, cancellation, and
  continuation work through coordinator-owned events.
- [ ] OMO-equivalent tools exist or have explicit documented Harness-native
  replacements: AST-grep, session tools, persistent task tools, look_at,
  interactive terminal, browser skills, skill_mcp, team tools, and built-in MCPs.
- [ ] Team mode has declared teams, active team listing, mailbox/task state,
  shutdown/delete, optional worktrees, optional tmux visualization, and doctor
  checks.
- [ ] Hooks cover context injection, quality guards, truncation, recovery,
  continuation, notifications, and compatibility imports through typed
  coordinator middleware.
- [ ] Slash commands cover OMO built-ins and custom command discovery.
- [ ] Skills can carry scoped MCP servers and permissions.
- [ ] Model fallback, runtime fallback, and model capability diagnostics are visible
  and tested.
- [ ] Replay remains side-effect free for every new feature.
- [ ] All public config keys have schemas, docs, examples, and drift tests.
- [ ] Every feature has deterministic tests and the appropriate manual signoff lane:
  CLI, TUI/PTY, browser, live provider, MCP, or native visual.
- [ ] `harness doctor --json` reports pass/warn/fail status for the parity surface.

## Test matrix

| Workstream | Required tests |
| --- | --- |
| Agent catalog | config schema, bootstrap profiles, doctor, TUI picker |
| Categories/model routing | config drift, task metadata, fallback validation |
| Specialist restrictions | permission denial tests, Plan subagent tests, read-only profile tests |
| Task/background | coordinator lifecycle, cancellation, late result, replay projection |
| Persistent tasks | event projection, dependency graph, restart/replay |
| Team mode | core coordinator tests, tool tests, TUI/doctor tests, worktree/tmux gated tests |
| AST-grep | schema tests, dry-run replace, unsupported language, artifact spill |
| Session tools | replay projection, redaction, large session, no side effects |
| Terminal | PTY happy path, bad input, screenshot/capture, cleanup |
| Browser | gated Playwright/agent-browser/dev-browser signoff |
| Skill MCP | stdio/http lifecycle, cleanup, permission deny, OAuth gated tests |
| Hooks | per-phase unit tests, critical failure, disable, redaction, replay purity |
| Commands | discovery, disable, execution, permission gating |
| Continuation | start, reminder, stop, limits, restart, replay no-op |
| Provider fallback | fixture streams, classified errors, cooldown, redacted metadata |
| Doctor | JSON stability, no-network default, actionable text |

## Documentation obligations

Each parity feature must update or add:

- [ ] public config docs and generated schema expectations;
- [ ] architecture docs if events, coordinator behavior, permissions, or replay
  contracts change;
- [ ] tool docs and examples for model-facing schemas;
- [ ] README quick-start notes only when the feature is part of the recommended
  everyday path;
- [ ] testing docs for any new lane or signoff gate;
- [ ] the parity ledger status and evidence links.

## Open design decisions

These decisions should be made before implementation begins in each area:

1. [ ] Whether OMO names are aliases over Harness profiles or separate shipped
   profiles with their own prompts.
2. [ ] Whether `call_omo_agent` should be exposed as a compatibility wrapper or kept
   out in favor of canonical `task`.
3. [ ] Whether persistent tasks need new event variants or can reuse existing task
   lifecycle plus metadata.
4. [ ] Whether skill MCP first-class tools are globally registered at load time or
   only exposed through `skill_mcp`.
5. [ ] Whether `.opencode` and `.claude` compatibility roots are enabled by default
   or only when a compatibility flag is set.
6. [ ] Which provider transport should be the first non-OpenAI-compatible adapter.
7. [ ] Whether browser automation should be MCP-only at first or include CLI adapters
   in the same phase.
8. [ ] Whether worktree creation belongs inside Team Mode or a general git/worktree
   adapter seam reused by Team Mode.

## Recommended first implementation slice

The highest-leverage first slice is:

1. [ ] Add the parity ledger and doctor warnings.
2. [ ] Implement the agent catalog seam.
3. [ ] Add OMO specialist profiles as read-only or orchestration-only where possible.
4. [ ] Add `session_*`, `background_cancel`, and `ast_grep_*` tools.
5. [ ] Extend skill discovery and add `frontend-ui-ux`, `git-master`, and
   `review-work` as built-in skills without MCP first.
6. [ ] Add `team_list` and team doctor checks.

This slice gives users visible parity progress while avoiding the hardest unsafe
areas: executable plugins, OAuth MCP, browser automation, and continuation loops.
