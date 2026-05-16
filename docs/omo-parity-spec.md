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
- [x] hooks, commands, context injection, recovery, diagnostics, and compatibility;
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
| Agent catalog | Core exposes a resolved catalog projection; doctor, task results, and TUI switchable-agent order consume it | Partial |
| Plan workflow | `build` -> `plan` via `plan_enter`; `plan` -> `build` via `plan_exit` | Present, with stricter safety |
| Discipline workflow | Prompt-profile behavior only | Partial |
| Specialist agents | OMO specialist profile contracts are shipped as subagents and appear in AgentCatalog; task routing/TUI consumption still missing | Partial |
| Continuation loops | Explicit bounded Ralph/ulw start-stop events now queue coordinator-owned reminders, consume todo output state, detect done markers, stop on limits, and remain replay-safe | Present |
| Team mode | Event-sourced team run, members, messages, tasks, shutdown, delete | Partial MVP |
| Tools | Read/search/edit/bash/web/LSP/MCP/task/team/skill/question/todos | Partial |
| AST-grep | Model-visible tool ids exist with explicit unsupported diagnostics; safe adapter pending | Partial |
| Interactive terminal | Tmux-backed `interactive_bash`/`terminal_*` tools are registered with dependency-gated execution | Partial |
| Browser automation | Playwright/agent-browser/dev-browser skills ship with doctor dependency diagnostics and browser signoff gating | Partial |
| Media analysis | `look_at` extracts replay-safe text/media metadata and routes visual interpretation to `multimodal-looker` | Partial |
| Session tools for agents | Model-visible `session_*` ids exist with explicit unsupported diagnostics; replay-safe tool context seam pending | Partial |
| Persistent task system | `todowrite` remains per-session; general `task_create/list/get/update` are event-sourced persistent dependency tasks with ready-task projection | Present |
| Skills | `.agent-harness`, OpenCode, Claude, Agents, `.harness`, and global skill roots; starter OMO-adjacent built-ins | Partial |
| Skill MCP | Config-backed MCP exists; no skill-scoped MCP lifecycle/tool | Major gap |
| Built-in MCPs | Generic MCP support, no bundled Exa/Context7/Grep.app profiles | Gap |
| Hooks | Typed coordinator hook phases/effects over lifecycle command hooks | Partial |
| Slash commands | TUI workflow commands plus prompt-only imported command templates | Present |
| Context injection | AGENTS.md and instructions in prompts | Partial |
| Provider breadth | Mock plus OpenAI-compatible | Partial |
| Model fallback | Model profiles can list fallback, but runtime fallback is not OMO-level | Gap |
| Doctor | Config/tool/profile/session/MCP checks plus parity-ledger warnings | Partial |
| Claude Code/OpenCode compatibility | Some config compatibility, active product areas rejected | Partial by design |

## Design goals

1. [ ] **Harness-native parity.** Match the operator behavior of OMO while keeping
   the implementation inside Harness modules and event contracts.
2. [ ] **Deep modules, small interfaces.** Add seams that hide complexity behind
   stable interfaces: extension registration, hook execution, agent routing,
   skill bundles, MCP sessions, and terminal sessions.
3. [ ] **Event-derived state.** State needed for resume, replay, TUI, audit, or
   background wakeups must be persisted through events or redacted artifacts.
4. [x] **No side effects during replay.** Hooks, extensions, MCPs, terminal sessions,
   provider calls, and shell commands must never execute during replay.
5. [x] **Compatibility through adapters.** OpenCode/Claude/OMO compatibility should
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

- [x] Define hook phases in `harness-core` around existing coordinator phases:
  `message_received`, `agent_turn_started`, `provider_params`,
  `provider_context_transform`, `tool_preflight`, `tool_result`,
  `agent_turn_finished`, `session_idle`, and `compaction_requested`.
- [x] Hook results are typed: allow, deny, modify model-facing context, request a
  reminder, write a redacted artifact, or add diagnostics.
- [x] Critical hook failure can cancel the current operation only through coordinator
  events.
- [x] Hook output must be capped and redacted before persistence.

**Acceptance criteria:**

- [x] Built-in lifecycle hooks migrate onto the same seam.
- [ ] Tool guard hooks can block a tool before permission resolution or before
  execution, with a durable reason. Current typed deny effects block the current
  coordinator operation through durable cancellation/failed-tool events; earlier
  pre-permission guard placement remains pending.
- [x] Transform hooks can publish provider-context transform intent without mutating
  events; applying arbitrary context rewrites remains limited to existing
  coordinator-owned compaction-summary override behavior.
- [x] Replay sees hook events and artifacts but never executes hooks.

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

- [x] Doctor reports every primary, specialist, and category route with model and
  fallback status.
- [x] TUI model/agent picker uses the same catalog for switchable-agent ordering.
- [ ] Task results expose resolved catalog metadata.
- [ ] Config drift tests fail if shipped OMO-profile contracts disappear.

### 4. Skill bundle seam

**Purpose:** Turn skills into bundles that can carry instructions, tools, MCP
servers, permissions, command templates, and verification guidance.

**Recommended shape:**

- [x] Extend `SKILL.md` frontmatter to include MCP definitions, tool permissions,
  command templates, environment policy, and optional verification hooks.
- [ ] Keep skill loading explicit through `skill` or `task(load_skills=[...])`.
- [x] Skill-provided MCP servers are session-scoped and visible only through
  `skill_mcp` or first-class tool ids granted by the loaded skill.

**Acceptance criteria:**

- [x] Skill content and MCP availability are injected only for the intended turn or
  child task.
- [ ] Skill MCP sessions clean up on idle, cancellation, and run finish.
- [ ] Skill permissions cannot broaden a statically denied profile permission.
- [ ] Doctor can show skill discovery roots and denied/invalid skills without
  loading MCP servers.

### 5. Terminal session seam

**Purpose:** Support persistent tmux-like interactive terminal sessions without
using one-shot bash as an interactive surrogate.

**Recommended shape:**

- [x] A `terminal_session` module in `harness-tools` backed by tmux or portable-pty
  adapters.
- [x] Tools: `interactive_bash`, `terminal_spawn`, `terminal_write`,
  `terminal_screenshot`, `terminal_resize`, `terminal_kill`, and
  `terminal_list` can be exposed progressively.
- [x] Session ids, pane/window metadata, screenshots, and captured text become
  redacted artifacts and event summaries.
- [ ] Plan mode may inspect terminal output only if shell policy and Plan guard allow
  it.

**Acceptance criteria:**

- [ ] Interactive sessions survive across tool calls within a run.
- [ ] A TUI app can be launched, driven, captured, and killed through tools.
- [x] Missing tmux/pty support yields actionable errors and doctor warnings.
- [ ] PTY signoff covers happy path, bad command, and cleanup.

## Parity workstreams

### A. Specialist agents and orchestration roles

**Target parity:** OMO exposes Sisyphus, Hephaestus, Prometheus, Oracle,
Librarian, Explore, Multimodal-Looker, Metis, Momus, Atlas, and
Sisyphus-Junior.

**Recommended Harness outcome:**

- [x] Keep existing `build`, `plan`, and `discipline` as stable Harness names.
- [x] Add OMO names as first-class profiles or aliases with explicit role metadata:
  - [x] `sisyphus`: primary orchestrator, todo-driven, may plan/delegate/verify;
  - [x] `hephaestus`: autonomous deep worker, close to current `build`/`discipline`;
  - [x] `prometheus`: read-only strategic planner, mapped to Plan workflow;
  - [x] `metis`: read-only pre-plan consultant;
  - [x] `momus`: read-only plan/work reviewer;
  - [x] `atlas`: execution orchestrator that reads plans, delegates implementation,
    and verifies results but does not edit code directly;
  - [x] `sisyphus-junior`: category executor with no redelegation;
  - [x] `oracle`: read-only architecture/debugging/review consultant;
  - [x] `librarian`: read-only docs/remote-code research agent;
  - [x] `explore`: read-only local codebase search agent;
  - [x] `multimodal-looker`: media analysis specialist.
- [x] Preserve tool restrictions as runtime policy, not just prompt text.
- [x] Implement deterministic primary-agent display ordering for TUI cycling.

**Current first-slice status:** OMO specialist profile contracts now ship in the
default public config and are visible through `AgentCatalog`/doctor. Runtime
specialist behavior is still limited by the existing prompt/profile system; TUI
catalog consumption and task metadata recording remain open.

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
- [x] Record the resolved category route in child task metadata.

**Acceptance criteria:**

- [x] `task(category="visual-engineering", load_skills=[...])` records category,
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

- [x] Add a coordinator-owned `ContinuationController` module.
- [x] Continuation is explicit: activated by a command/tool, bounded by config, and
  visible in events.
- [x] Add events such as `ContinuationStarted`, `ContinuationReminderQueued`,
  `ContinuationStopped`, and `ContinuationLimitReached` only if existing task
  events cannot express the state clearly.
- [x] Add commands/tools:
  - [x] `/ralph-loop` starts goal continuation with done-marker detection;
  - [x] `/ulw-loop` starts Ralph plus ultrawork mode settings;
  - [x] `/stop-continuation` stops all continuation mechanisms for the run;
  - [x] `ultrawork` keyword or command activates the orchestrator mode for one turn
    or one explicit loop.
- [x] Todo continuation should use the same task/todo projection as the TUI, not a
  separate memory-only hook.

**Safety requirements:**

- [x] Every loop has max iterations, max wall-clock, max provider calls, and max tool
  calls.
- [x] User interruption and `/stop-continuation` take priority over reminders.
- [x] Reminders are persisted and replay-rendered, but replay never schedules them.
- [x] Continuation is disabled in Plan unless a Plan-specific reviewed flow is added.

**Acceptance criteria:**

- [x] A loop can start, continue after an idle turn, stop, hit max iterations, and
  resume after process restart.
- [x] TUI shows active continuation state and stop action.
- [x] Deterministic tests cover done-marker detection, incomplete todos, stop, and
  limit reached.

### F. Persistent task system

**Target parity:** OMO has `task_create`, `task_get`, `task_list`, and
`task_update` for persistent dependency-aware tasks.

**Recommended Harness outcome:**

- [x] Keep `todowrite` as the lightweight per-session visible checklist.
- [x] Add a separate persistent task module for dependency-aware work items.
- [x] Prefer event-sourced task state over ad hoc JSON files. If file artifacts are
  needed for human editing, derive them from events or treat them as artifacts.
- [x] Use schema fields compatible with OMO/Claude naming: `subject`, `description`,
  `status`, `active_form`, `blocked_by`, `blocks`, `owner`, `metadata`, and
  `thread_id`/`run_id`.
- [x] Integrate with Atlas, Team Mode, and continuation controller by exposing
  replay-projected `ready_task_ids` through `task_list` while keeping execution
  coordinator-owned.

**Acceptance criteria:**

- [x] Task dependency projection computes `blocks` from `blocked_by` deterministically.
- [x] Tasks survive restart and replay.
- [x] Parallel-ready tasks can be surfaced to the orchestrator, but execution remains
  coordinator-owned.
- [ ] `tasks-todowrite-disabler` behavior, if implemented, is a policy option rather
  than a hidden hook.

### G. Team Mode completion

**Target parity:** OMO Team Mode includes declared teams, 12 tools, shared
mailbox, shared task list, file-locked claims, worktrees, tmux layout,
diagnostics, and active runtime directories.

**Current Harness state:** Harness has event-sourced `team_create`, `team_list`,
`team_status`, `team_send_message`, `team_task_create/list/get/update`,
`team_shutdown_request/approve/reject`, and `team_delete`. `team_list` shows
replay-derived active runs plus declared team specs from Harness team roots.
Replay projections derive workflow ids, task status counts, durable mailbox artifact
refs, advisory file claims, worktree/tmux diagnostics, and shutdown proof. Missing
pieces are optional worktree/tmux creation and cleanup adapters.

**Recommended Harness outcome:**

- [x] Add `team_list` for active teams and declared team listing through the
  declared team registry.
- [x] Add declared team specs under `.agent-harness/teams/<name>.json` and user
  equivalents under the XDG Harness config directory.
- [x] Keep active team state event-sourced. Use artifacts for large mailbox bodies or
  delivery diagnostics, not as the source of truth.
- [ ] Add optional per-member worktrees through a `TeamWorktreeAdapter`. Worktree
  creation is permission-gated and never automatic for read-only research roles.
- [x] Add advisory file claims as team task metadata if they affect coordination.
- [ ] Add optional tmux visualization through the terminal session seam. Missing tmux
  must warn, not block team creation.

**Acceptance criteria:**

- [x] `team_list` shows declared teams and active runs.
- [x] Declared team specs validate lead/member eligibility before spawning.
- [x] Team deletion requires shutdown proof or explicit abort metadata; replay does
  not launch workers, worktrees, or tmux.
- [ ] Worktree path validation rejects bare branch names, traversal, and unsafe
  external paths unless explicitly allowed.
- [ ] Tmux visualization starts, rebalances, and cleans up panes without changing the
  underlying team state if tmux fails.
- [x] Doctor reports team spec count, active team count, git/tmux availability, and
  stale runtime artifacts.

### H. Tool parity

**Target parity:** OMO exposes native tools for search, edits, LSP, AST-grep,
delegation, visual analysis, skills, session history, task management, and
interactive terminal use.

**Recommended Harness additions:**

1. [ ] **AST-grep tools**
   - [x] Register `ast_grep_search` and `ast_grep_replace` with strict schemas and
     explicit unsupported diagnostics.
   - [x] Replace diagnostics with first-class executable tool implementations.
   - [x] Use a Rust adapter or CLI adapter with strict argument schemas.
   - [x] Dry-run replace by default.
   - [x] Persist large results as artifacts.

2. [ ] **Delegation aliases**
   - [ ] Consider `call_omo_agent` as a compatibility wrapper for direct
     `task(subagent_type=...)` calls to `oracle`, `librarian`, and `explore`.
   - [ ] Keep `task` canonical.

3. [ ] **Background cancellation**
   - [x] Add `background_cancel` as a compatibility wrapper around
     `background_output(cancel=true, ...)`.
   - [x] Support individual cancellation; avoid global cancel unless scoped to the
     current parent run.

4. [x] **Visual analysis**
   - [x] Register `look_at` with strict schema and explicit unsupported diagnostics.
   - [x] Add `look_at` backed by `multimodal-looker`.
   - [x] Accept workspace files and explicitly provided image data.
   - [x] Store extracted text and media summaries as artifacts when large.

5. [ ] **Session tools**
   - [x] Register model-visible `session_list`, `session_read`, `session_search`, and
     `session_info` with strict schemas and explicit unsupported diagnostics.
   - [x] Replace diagnostics with implementations of `session_list`, `session_read`,
     `session_search`, and `session_info` tools over Harness session logs.
   - [x] Reuse replay/transcript projections.
   - [x] Never execute tools while reading sessions.

6. [x] **Persistent task tools**
   - [x] Register `task_create`, `task_get`, `task_list`, and `task_update` with
     strict schemas and explicit unsupported diagnostics.
   - [x] Replace diagnostics with event-sourced implementations as described in
     workstream F.

7. [x] **Interactive terminal tools**
   - [x] Add `interactive_bash` or the broader terminal session toolset described in
     the terminal session seam.

8. [ ] **Skill MCP tool**
   - [x] Add `skill_mcp` for skill-scoped MCP operations.

**Acceptance criteria:**

- [ ] Every new tool has strict JSON schema, capability mapping, permission policy,
  artifact behavior, registry tests, docs, and prompt examples.
- [ ] Tool failures return actionable structured errors.
- [ ] Tool ids are included in `native_tool_parity_matrix`-style coverage.

### I. Browser automation and media workflows

**Target parity:** OMO provides browser automation through Playwright MCP,
agent-browser CLI, and dev-browser, plus `look_at` for images/PDFs.

**Recommended Harness outcome:**

- [x] Implement browser automation as skills first, not as always-on global tools.
- [x] Built-in skills:
  - [x] `playwright`: launches a Playwright MCP server through skill-embedded MCP;
  - [x] `agent-browser`: wraps the agent-browser CLI when installed;
  - [x] `dev-browser`: supports persistent browser state for iterative work.
- [x] Add browser capability diagnostics to doctor.
- [x] Add media extraction through `look_at` and `multimodal-looker`.

**Acceptance criteria:**

- [ ] A visual-engineering task can load `frontend-ui-ux` and `playwright`, open a
  page, interact, screenshot, and report evidence.
- [ ] Browser artifacts are written under session artifacts with redacted metadata.
- [x] Missing browser dependencies produce doctor warnings and tool errors, not
  panics.
- [x] Live/browser lanes are environment-gated.

### J. Skills and built-in skills

**Target parity:** OMO ships skills such as `git-master`, browser skills,
`frontend-ui-ux`, `review-work`, `ai-slop-remover`, and `team-mode`; custom
skills can come from OpenCode, Claude, Agents, and user paths.

**Recommended Harness outcome:**

- [x] Extend discovery order to include:
  1. [x] project `.agent-harness/skills/*/SKILL.md`;
  2. [x] project `.opencode/skills/*/SKILL.md`;
  3. [x] project `.claude/skills/*/SKILL.md`;
  4. [x] project `.agents/skills/*/SKILL.md`;
  5. [x] user Harness, OpenCode, Claude, and Agents skill directories.
- [x] Keep Harness-owned paths first unless an explicit compatibility mode says
  otherwise.
- [ ] Add built-in skill packs:
  - [x] `git-master`;
  - [x] `playwright`, `agent-browser`, `dev-browser`;
  - [x] `frontend-ui-ux`;
  - [x] `review-work`;
  - [x] `ai-slop-remover`;
  - [x] `team-mode` usage documentation.
- [x] Support frontmatter fields for MCP, permissions, tools, commands, and
  environment allowlists.

**Acceptance criteria:**

- [x] Skill discovery reports visible, denied, invalid, and shadowed skills.
- [x] `task(load_skills=[...])` injects skill content and parsed policy metadata
  only for the child session.
- [ ] Skill-declared tools/MCP servers are activated only for the intended child
  session through the skill MCP lifecycle.
- [x] Built-in skills have docs, tests, and config disable switches.

### K. MCP parity

**Target parity:** OMO has three MCP tiers: built-in remote MCPs,
Claude/OpenCode `.mcp.json` compatibility, and skill-embedded MCPs with OAuth.

**Recommended Harness outcome:**

- [x] Keep current config-backed MCP server support.
- [ ] Add bundled MCP profiles for:
  - [x] Exa/Tavily web search;
  - [x] Context7 documentation lookup;
  - [x] Grep.app/GitHub code search.
- [x] Add `.mcp.json` compatibility loader as a translation adapter, with explicit
  env variable expansion policy.
- [x] Add skill-embedded MCP session management.
- [ ] Add OAuth 2.1 support with PKCE, dynamic registration when available,
  protected-resource discovery, token refresh, and secure user-token storage.

**Acceptance criteria:**

- [x] MCP discovery never blocks deterministic tests unless explicitly enabled.
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

- [x] Hooks are individually disableable by stable id.
- [x] Hook effects are visible in events, artifacts, or provider-context metadata.
- [x] Critical hook failure behavior is deterministic and tested.
- [ ] Hook output truncation is context-aware and redacted. Current hook output is
  redacted and capped before persistence, and typed truncation effects are
  recorded; dynamic per-tool truncation policies remain pending.

### M. Slash command system

**Target parity:** OMO supports built-in commands and custom command discovery.

**Recommended Harness outcome:**

- [x] Add a command registry distinct from TUI-only slash commands.
- [x] Commands are templates that can:
  - [x] load a prompt;
  - [x] load skills;
  - [x] request a profile switch;
  - [x] call a native tool;
  - [x] start continuation;
  - [x] create a plan or handoff artifact.
- [ ] Built-in commands:
  - [x] `/init-deep`;
  - [x] `/ralph-loop`;
  - [x] `/ulw-loop`;
  - [x] `/cancel-ralph`;
  - [x] `/refactor`;
  - [x] `/start-work`;
  - [x] `/stop-continuation`;
  - [x] `/remove-ai-slops`;
  - [x] `/handoff`;
  - [x] `/hyperplan` if team/parallel planning support is present.
- [x] Custom command roots should include Harness, OpenCode, and Claude-compatible
  locations.
- [x] Imported command templates are prompt-only slash commands that submit through
  the coordinator path and never execute shell code directly.

**Acceptance criteria:**

- [x] Commands are listed in TUI and doctor.
- [ ] Unknown/disabled commands produce actionable errors.
- [x] Command execution re-enters the coordinator and is recorded.
- [x] Custom command templates cannot execute shell code without a tool permission.

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

- [x] Add runtime consumption of resolved model fallback chains.
- [x] Add provider error classification: auth, rate limit, overload, context window,
  malformed stream, unsupported tool, and transport failure.
- [x] Add fallback cooldowns and per-run fallback telemetry.
- [x] Add model capability diagnostics/cache from the generated model catalog;
  optional models.dev refresh remains deferred until it has offline fixtures.
- [ ] Add provider-native transports beyond OpenAI-compatible only after each has
  fixture and live signoff coverage.

**Acceptance criteria:**

- [x] Doctor reports effective model resolution for every profile/category and warns
  about unsupported tools/modalities.
- [x] Runtime fallback switches model only on classified retryable failures and records
  the reason.
- [x] Fallback never changes replay semantics.
- [x] Provider-specific details remain in `harness-providers` adapters.

### P. Recovery and session repair

**Target parity:** OMO recovers from missing tool results, thinking block errors,
empty messages, context-window failures, JSON parse errors, and session errors.

**Recommended Harness outcome:**

- [x] Keep replay pure, but add explicit recovery inspection and repair commands for
  operator-approved session repair.
- [x] Add provider-context validators before sending model input.
- [x] Add recovery paths for malformed tool-result content and unsupported provider
  tool-call formats.
- [x] Add context-window overflow recovery through existing compaction retry path and
  model fallback only when configured.

**Acceptance criteria:**

- [x] `harness sessions inspect` reports recovery issues and suggested repair actions.
- [x] Automatic recovery never rewrites `events.jsonl` silently.
- [x] Repair commands write new events or copied child sessions, not in-place edits.

### Q. Compatibility surfaces

**Target parity:** OMO loads Claude Code/OpenCode agents, commands, skills,
hooks, MCPs, and plugins.

**Recommended Harness outcome:**

- [x] Support compatibility in this order:
  1. [x] import agents as Harness profiles;
  2. [x] import skills as Harness skills;
  3. [x] import commands as command templates;
  4. [x] import `.mcp.json` as MCP server config;
  5. [x] import safe hook subsets as typed hook records; execution requires
     explicit `compatibility.enable_imported_hooks` opt-in;
  6. [x] only then consider plugin manifests.
- [x] Unsupported active plugin/server/share/autoupdate behavior should remain
  rejected until the extension seam can enforce safety.

**Acceptance criteria:**

- [x] Compatibility imports are visible in doctor with source path and enabled state.
- [x] Imported items can be disabled individually.
- [x] Import errors do not abort startup unless the item is explicitly required.

### R. Diagnostics and doctor

**Target parity:** OMO doctor checks registration, config, models, environment,
team mode, MCPs, capabilities, and compatibility warnings.

**Recommended Harness outcome:**

- [ ] Expand `harness doctor` with checks for:
  - [x] agent catalog completeness;
  - [x] category/model fallback health;
  - [x] provider credential status;
  - [x] model tool/modality capability;
  - [ ] skill discovery and skill MCP readiness;
  - [x] built-in MCP configuration;
  - [x] browser dependencies;
  - [x] tmux/pty/git availability;
  - [x] team spec/runtime health;
  - [ ] continuation state;
  - [ ] hook registry and disabled hook ids;
  - [x] compatibility imports;
  - [x] session directory/index health;
  - [ ] performance evidence freshness.

**Acceptance criteria:**

- [x] `doctor --json` exposes stable machine-readable check ids.
- [ ] Text output is concise and actionable.
- [x] No doctor check performs provider/MCP/browser network calls unless explicitly
  requested.

### S. TUI and operator UX

**Target parity:** OMO adds agent ordering, background notifications, tmux panes,
commands, status, toggles, session tools, and rich workflow affordances.

**Recommended Harness outcome:**

- [x] TUI agent picker uses resolved agent catalog order.
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
  - [x] `terminal` for persistent interactive sessions;
  - [x] `browser` for browser automation;
  - [ ] `mcp` or per-MCP capability if transport policy is insufficient;
  - [ ] `continuation` for loops;
  - [ ] `external_directory` for explicit outside-workspace access.
- [x] Add shell command mediation with dangerous-pattern classification and Plan-mode
  read-only guard reuse.
- [x] Add compatibility permission translation from Claude/OpenCode where safe.

**Acceptance criteria:**

- [ ] New capabilities have scalar policy, selector rules where meaningful, tests,
  docs, and TUI permission prompts.
- [x] Static deny always beats grants and compatibility imports.
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
- [x] Add a parity evidence ledger in machine-readable form, for example
  `docs/parity-ledger.json` or `configs/parity-ledger.json`.

**Acceptance criteria:**

- [ ] Numeric public claims cite an artifact path and run id.
- [ ] Perf budget changes require reviewed contract edits.
- [ ] CI can run deterministic perf smoke checks without live providers.

## Recommended delivery order

### Phase 0: Ledger and docs foundation

- [x] Add a machine-readable parity ledger with owner/status/evidence fields.
- [x] Cross-link this spec from `docs/inspiration-gap-analysis.md` and README once it
  is accepted.
- [x] Add doctor checks for current known gaps as warnings.

**Exit criteria:** parity status is visible and testable without changing runtime
behavior.

### Phase 1: Agent catalog, categories, and specialist profiles

- [ ] Implement `AgentCatalog`.
- [ ] Add OMO specialist profiles and display ordering.
- [x] Add per-agent/category fallback metadata to doctor.
- [ ] Add `oracle`, `librarian`, `metis`, `momus`, `atlas`, `hephaestus`,
  `sisyphus`, `sisyphus-junior`, and `multimodal-looker` profile contracts.

**Exit criteria:** all OMO agent names resolve; read-only/write restrictions are
enforced and tested.

### Phase 2: Tool parity core

- [x] Add AST-grep tools.
- [x] Add session tools.
- [x] Add `background_cancel` wrapper.
- [x] Add persistent task tools.
- [x] Add `look_at` if multimodal provider support is available, otherwise add the
  profile and a clear unsupported error.

**Exit criteria:** core OMO tool list is available or explicitly unsupported with
doctor warnings.

### Phase 3: Skill bundles and skill MCP

- [x] Extend skill discovery roots.
- [ ] Extend skill frontmatter.
- [x] Add `skill_mcp`.
- [ ] Add built-in skills.
- [x] Add skill-scoped MCP lifecycle.

**Exit criteria:** a `visual-engineering` child can load `frontend-ui-ux` and
`playwright` and see only the intended skill tools.

### Phase 4: Hook middleware and built-in hooks

- [x] Add typed hook seam.
- [x] Port existing lifecycle hooks.
- [ ] Add quality/safety, context, truncation, and recovery hooks. Typed effects
  for block/transform/truncate/notify/recover are persisted and tested; the
  built-in hook library remains pending.
- [x] Add compatibility hook import only for safe typed subsets; imported hook
  execution is explicitly opt-in via `compatibility.enable_imported_hooks`.

**Exit criteria:** hooks can block, transform, truncate, notify, and recover
through coordinator-owned events and artifacts.

### Phase 5: Continuation and orchestration loops

- [x] Add continuation controller.
- [x] Add `/ralph-loop`, `/ulw-loop`, `/stop-continuation`.
- [x] Add todo continuation.
- [ ] Add unstable-agent babysitter.
- [x] Add ultrawork keyword/command routing.

**Exit criteria:** bounded continuation survives restart, can be stopped, and is
visible in TUI/replay.

### Phase 6: Team mode completion

- [ ] Add `team_list`.
- [x] Add declared team registry.
- [ ] Add worktree adapter.
- [ ] Add file claims.
- [ ] Add tmux visualization through terminal session seam.
- [x] Add team doctor checks.

**Exit criteria:** team mode matches OMO user-visible lifecycle while keeping
Harness state event-sourced.

### Phase 7: Browser, terminal, and media signoff

- [x] Add terminal session seam and `interactive_bash`.
- [x] Add browser skills and dependency diagnostics.
- [x] Add media analysis tooling.
- [x] Add PTY/browser/live signoff lanes.

**Exit criteria:** agents can drive a TUI app, a web UI, and media analysis
through their matching surfaces with persisted evidence.

### Phase 8: Provider and model fallback depth

- [x] Add runtime fallback chains.
- [x] Add provider error classification and cooldowns.
- [x] Add model capability diagnostics.
- [ ] Add additional native provider adapters only with fixtures and live signoff.

**Exit criteria:** provider/model fallback is observable, testable, and does not
alter replay semantics.

### Phase 9: Compatibility import and extension runtime

- [x] Add command/skill/agent/MCP compatibility imports.
- [x] Add safe hook subset imports.
- [x] Add manifest-only extension registration.
- [x] Defer executable plugin loading until command mediation and sandbox evidence
  are complete.

**Exit criteria:** compatibility improves operator migration without weakening
Harness safety.

## Definition of parity done

Harness reaches OMO parity when all of the following are true:

- [ ] All OMO specialist agent names resolve through the agent catalog with correct
  tool restrictions, model routing, fallback status, and TUI visibility.
- [x] `task`, category routing, skill injection, background output, cancellation, and
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
- [x] Slash commands cover OMO built-ins and custom command discovery.
- [ ] Skills can carry scoped MCP servers and permissions.
- [ ] Model fallback, runtime fallback, and model capability diagnostics are visible
  and tested.
- [ ] Replay remains side-effect free for every new feature.
- [ ] All public config keys have schemas, docs, examples, and drift tests.
- [ ] Every feature has deterministic tests and the appropriate manual signoff lane:
  CLI, TUI/PTY, browser, live provider, MCP, or native visual.
- [x] `harness doctor --json` reports pass/warn/fail status for the parity surface.

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
- [x] the parity ledger status and evidence links.

## Open design decisions

These decisions should be made before implementation begins in each area:

1. [ ] Whether OMO names are aliases over Harness profiles or separate shipped
   profiles with their own prompts.
2. [ ] Whether `call_omo_agent` should be exposed as a compatibility wrapper or kept
   out in favor of canonical `task`.
3. [x] Whether persistent tasks need new event variants or can reuse existing task
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

1. [x] Add the parity ledger and doctor warnings.
2. [x] Implement the first AgentCatalog projection and doctor visibility; task
   routing/TUI/catalog metadata consumption remains follow-up work.
3. [x] Add OMO specialist profiles as read-only or orchestration-only where possible.
4. [x] Register `session_*`, `background_cancel`, and `ast_grep_*` tool ids;
   unsafe/larger surfaces still return explicit unsupported diagnostics.
5. [x] Extend skill discovery and add `frontend-ui-ux`, `git-master`, and
   `review-work` as built-in skills without MCP first.
6. [x] Add active-run `team_list` over replay-derived team state.
7. [x] Add team doctor checks.
8. [x] Add declared team listing.

This slice gives users visible parity progress while avoiding the hardest unsafe
areas: executable plugins, OAuth MCP, browser automation, and continuation loops.
