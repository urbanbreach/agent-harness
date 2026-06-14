# V1 release roadmap

This roadmap defines the first public release target for Agent Harness.

V1 should be a vanilla local-coding harness first: a complete,
trustworthy CLI/TUI runtime with safe native tools, durable sessions, clear
permissions, simple subagents, and stable extension seams. Advanced orchestration
features are welcome only where they strengthen that baseline without turning V1
into a full agent-OS or arbitrary plugin host.

Use this document as the shared direction for maintainers and agents working in
this workspace. Checked items are included only when the current tree already
documents and implements the behavior through first-party files that were
reviewed for this roadmap. Unchecked items are release work or post-V1 work.

Agents working from this roadmap should use the checked-in reference material
under `inspirations/` when comparing upstream behavior or orchestration-style features.
Do not rely on memory or external summaries when deciding whether a roadmap item
matches the intended user experience; inspect the local reference files first,
then adapt the idea to Harness's Rust-native, event-sourced architecture.

## V1 end state

V1 ends when the local CLI/TUI coding workflow is complete and reliable. That
means Harness should feel like a vanilla local coding agent for the
core loop, not that it must match every feature in every inspiration harness. A
user should be able to install it, configure one provider, launch the TUI, ask
for a code change, approve/deny tool use, inspect the diff, resume the session
later, replay what happened, and understand failures through doctor and docs.

The V1 product promise is:

> Local coding UX on a Harness-native Rust runtime, with event-sourced
> sessions, replay-safe tools, hashline editing, simple subagents with their own
> system prompts, markdown skills, and clear extension seams.

V1 should differ from the vanilla reference only where the difference is a deliberate
Harness strength:

- [x] Rust-native runtime and crate seams instead of copying the upstream app
  architecture.
- [x] Event log and replay invariants are core product behavior.
- [x] Hashline editing is first-class rather than an optional editing style.
- [x] Simple task subagents are part of the core workflow.
- [x] Category routes exist only for delegation, not as main engine modes.
- [x] Doctor/readiness/evidence-gate foundations exist as product surfaces; V1
  hardening remains tracked below rather than treated as maintainer-only scripts.
- [x] Extension seams are designed before arbitrary plugin compatibility is
  promised.

Orchestration-inspired V1 inclusions are intentionally narrow. Checked items in this list
mean the current mechanism exists; unchecked items are V1 hardening work that
must still land before the roadmap can treat the surface as release-ready:

- [x] Intent-gate and prompt-rigor ideas.
- [x] Read-only `explore` as a subagent.
- [x] Category delegation names and routing concepts.
- [x] Markdown skill loading as a runtime mechanism.
- [x] Skill progressive-disclosure behavior is documented and tested.
- [x] Candidate built-in skills such as `git-master`, `review-work`, and
  `frontend-ui-ux` ship with V1-quality bodies, docs, disablement, and tests.
- [x] Stronger doctor checks, prompt snapshots, and evidence gates cover prompt,
  skill, task-route, and asset readiness.
- [x] AST-grep search, model-visible session tools, and a dedicated `background_cancel`
  land as practical tool-surface improvements with docs and parity tests.

Everything else from the heavy-orchestration references is post-V1 by default unless this document explicitly
moves it into the release scope. That includes full specialist-agent catalogs,
media automation, remote collaboration bots, and broad plugin compatibility.

## Post-V1 direction

Post-V1 work should build optional layers on top of the stable Harness core, not
reshape V1 around orchestration features. The expected order is:

- [ ] Harden the extension manifest and built-in capability system.
- [ ] Add richer built-in skills and skill bundles beyond the small V1 candidate
  set.
- [ ] Expand subagents only after the `AgentCatalog`, prompt snapshots, and
  permission fixtures are stable, keeping any richer orchestration in an
  optional layer if it proves useful.
- [ ] Add skill-embedded MCP and OAuth only after ordinary MCP, skills, and
  extension-state contracts are boring.
- [ ] Add browser/media/desktop automation as installable capabilities, not core
  release blockers.
- [ ] Revisit upstream plugin compatibility only after Harness-native extension
  seams have their own conformance suite.

## Local inspiration map

- [x] The vanilla CLI/TUI reference material under `inspirations/` is the primary source for
  CLI, TUI, sessions, MCP, plugins, permissions, providers, and app-level product
  expectations.
- [x] Optional visual reference material is comparison context only, not runtime authority.
- [x] `inspirations/codex/` is the Codex CLI reference for install simplicity,
  sandbox-conscious Rust architecture, TUI snapshot rigor, and provider/tool
  execution ergonomics.
- [x] The heavy-orchestration reference under `inspirations/` is available as
  comparison material, but do not treat complete agent-OS parity as a V1
  goal.
- [x] A workflow-layer reference under `inspirations/` is available for pairing a
  base harness with setup, doctor, real execution smoke tests, durable plans, and
  evidence-gated release discipline.
- [x] The TypeScript baseline under `inspirations/` is available for package seams,
  provider abstraction, configurable keybindings, interactive testing practice,
  and supply-chain hardening.
- [x] A Rust performance/security reference under `inspirations/` covers
  single-binary release posture, structured concurrency, capability-gated
  extensions, session indexing, evidence-gated claims, and crash-resilient
  persistence.
- [x] The extension-first reference under `inspirations/` is available: keep core
  changes small, ship useful builtin extensions, and leave heavier features as
  installable packages.
- [x] A product-polish reference under `inspirations/` covers for review diffs,
  session sidebars, search, status surfaces, mobile/desktop clients, and user
  interface experiments pending upstream.

## Product stance

- [x] V1 targets a vanilla local-coding operator experience rather than full
  orchestration parity.
- [x] The runtime remains Rust-native and event-sourced; replay must stay
  side-effect free.
- [x] Hashline editing stays the normal file-changing path.
- [x] `build` is the default implementation agent.
- [x] `plan` is a selectable primary agent alongside `build`.
- [x] `explore` is a read-only subagent profile used through
  `task(subagent_type = "explore")`, not a main engine view.
- [x] Public docs describe the V1 stance clearly from the README and config
  guide.
- [x] Docs remove or update stale references to missing runtime assets such as
  `.agent-harness/native-agents/*.toml` and `.agent-harness/agents/operator.md`.

## Deepening criteria for checked foundations

Checked foundation items in this roadmap mean the runtime mechanism exists. They
do not automatically mean the surface is V1-quality. The boxes below define what
turns those working mechanisms into well-specified release behavior.

### Reference prompt-system lessons

- [x] Use the vanilla reference's agent, skill, project-instruction, and command
  directory layouts as references for markdown-defined agents,
  skills, and commands.
- [x] Use Codex's base-instruction and core-skill design as the reference for
  instruction precedence, preamble rigor, and progressive disclosure.
- [x] Use orchestration prompt libraries as references for explicit scope guards, output
  contracts, role-specific tool restrictions, and lifecycle hook maps.
- [x] Use the extension-first and TypeScript baselines as references for extension-first design, disableable builtin
  capabilities, compaction safety, prompt presets, and release evidence gates.
- [x] Each adopted reference pattern names the Harness seam that owns it before
  implementation starts.
- [x] Reference behavior is copied as user-observable behavior, not as source
  architecture, package layout, or brand-specific terminology.

### Agent prompt depth

- [x] Runtime profiles resolve from `configs/harness.example.jsonc` into
  coordinator `AgentProfile` values.
- [x] The dynamic prompt builder composes base/model prompt, environment,
  delegation reminder, project instructions, and skill guidance.
- [x] `build` and `plan` prompt asset files exist under
  `.agent-harness/agents/`.
- [x] `build.md` has a source-controlled prompt body, not only frontmatter.
- [x] `plan.md` has a source-controlled prompt body, not only frontmatter.
- [x] Every primary prompt uses a shared skeleton: identity, goal, use when, do
  not use when, scope guard, tool/permission posture, operating loop, ask gate,
  failure recovery, output contract, and verification gate.
- [x] Every subagent prompt uses the same skeleton, with stronger output
  contracts and clearer stop conditions than primary agents.
- [x] Prompt bodies declare what is enforced by permissions versus what is only
  behavioral guidance.
- [x] Prompt precedence is documented and tested: system/developer/user,
  AGENTS.md, runtime agent prompt, config instructions, loaded skills, and task
  delegation context.
- [x] Prompt bodies for primary agents, subagents, and category routes are
  near-exact adaptations of the relevant reference model implementation prompt bodies, with only branding,
  unsupported agent-OS workflows, and features not present or not planned for
  Harness removed. Any retained reference model implementation behavior must map to an explicit Harness
  runtime seam, permission policy, tool, documentation contract, or roadmap item.
- [x] Primary prompts include an intent-gate pattern before tool use for ambiguous
  requests: state the interpreted intent, then route to explain, investigate,
  implement, plan, or ask exactly one blocking question.
- [x] Dynamic prompt sections are named modules with golden tests for each section
  and for full composed prompts.
- [x] Model-specific prompt tuning is either intentionally absent for V1 or
  represented as explicit prompt presets with tests; substring heuristics do not
  become the only long-term seam.
- [x] Prompt golden tests cover `build`, `plan`, `general`,
  `explore`, all category routes, and hidden title/summary/compaction profiles.

### Subagent and category depth

- [x] `task(subagent_type = ...)` can spawn named subagent profiles.
- [x] `task(category = ...)` maps category names to ordinary non-primary profiles.
- [x] `task(run_in_background = true)` schedules background child sessions and
  `background_output` retrieves results.
- [x] Plan can delegate only to `explore` through profile-aware task description
  filtering and parent-child policy enforcement.
- [x] `general` has a real prompt body defining when it should handle multistep
  work, how much context to return, and when to refuse work that belongs to
  primary Build.
- [x] `explore` has a real prompt body defining read-only behavior, search
  strategy, output contract, and stop condition.
- [x] `explore` returns structured findings such as files, relationships, answer,
  and next steps, rather than freeform summaries.
- [x] Category routes have category-specific prompt appends for their domains,
  especially `visual-engineering`, `ultrabrain`, `deep`, `quick`, and `writing`.
- [x] Category route descriptions include use-when and do-not-use-when guidance so
  parent agents can choose correctly.
- [x] The task tool contract recommends or enforces a structured delegation body:
  context, goal, downstream use, request, required tools, must-do, and must-not-do.
- [x] Child task summaries are capped and structured so parent context stays lean.
- [x] Category route model, variant, prompt append, tools, permissions, hidden
  status, and fallback are centralized behind an `AgentCatalog`-style seam.
- [x] Fallback from an unknown category to `general` is visible in task output and
  doctor/readiness diagnostics.

### Skill depth

- [x] Skills are discovered from configured project/global roots.
- [x] `skill` can load a discovered skill into the current turn.
- [x] `task(load_skills = [...])` injects loaded skill content into child prompts.
- [x] Skill loading supports allow, ask, and deny permission modes.
- [x] Skill discovery documents implemented V1 precedence across configured
  project/workspace roots, git-root walking, and user/XDG global roots.
- [x] Skill discovery documents and tests full compatibility/built-in precedence
  across built-in scopes and imported external editor/assistant/agent roots before
  V1 ships.
- [x] External editor, assistant, and agent compatibility skill roots are tracked as adapter
  work: `.external-editor/skills/*/SKILL.md`, `.assistant/skills/*/SKILL.md`,
  `.agents/skills/*/SKILL.md`, and user-level equivalents. Harness-owned roots
  stay first unless an explicit compatibility mode says otherwise.
- [x] Skill frontmatter has a V1 schema for name, description, argument hint,
  allowed/expected tools, target agent/category, and deferred MCP/resource
  metadata; skill loading permission modes are config/catalog metadata, not
  frontmatter.
- [x] The skill authoring guide documents a quality template: purpose, use when,
  do not use when, execution policy, steps, tool usage, escalation/stop
  conditions, final checklist, and advanced notes.
- [x] Skill loading follows progressive disclosure: metadata in catalog and
  SKILL.md plus declared bundled resources on activation, with resource caps and
  escape tests.
- [x] Skill prompt injection order and conflict resolution with agent prompts and
  AGENTS.md is documented.
- [x] Built-in skills are disableable by stable ids before V1 adds more of them.
- [x] Built-in skill candidates are reviewed against the V1 stance before being
  checked: `git-master`, `review-work`, and `frontend-ui-ux` are useful; browser,

### Built-in extension and state depth

- [x] V1 names which shipped behaviors are core runtime behavior and which are
  disableable built-in capabilities.
- [x] Disableable built-ins have stable ids, default states, config shape, doctor
  visibility, and tests.
- [x] Built-in capability order is intentional and tested when ordering affects
  prompt assembly, tool registration, permission checks, or compaction.
- [x] New built-in capabilities use the public Harness interfaces; they do not
  reach around coordinator permissions, event storage, or tool registry seams.
- [x] Any JSONL or artifact state written by built-ins has a documented schema,
  migration policy, and replay behavior before it is release-blocking.
- [x] Compaction has explicit V1 contracts for threshold policy, retained recent
  turns, file/tool context preservation, todo/plan bridging, and post-compaction
  restoration hints.
- [x] Compaction failures have a bounded fallback policy with user-visible status;
  repeated failures do not silently erase context or loop forever.
- [x] Provider/model prompt presets, if added, are thin tuning layers over the base
  prompt skeleton, not duplicate full system prompts.

### Command and hook depth

- [x] V1 decides whether markdown-defined slash commands, distinct from the
  unsupported upstream config `command` area, are first-class; strict V1 keeps
  markdown command files, `$ARGUMENTS` substitution, and command interpolation
  intentionally unsupported while first-party TUI slash actions stay native.
- [x] Command interpolation is intentionally unsupported for strict V1, so no
  markdown command rendering executes during replay; any future support must add
  permission and replay-safety tests first.
- [x] Hook phases have a lifecycle map with status labels: native, fallback,
  intentionally unsupported, and post-V1.
- [x] Minimal V1 hook seams are limited to coordinator-owned native lifecycle
  phases; context transform, session idle, markdown command files, and arbitrary
  plugin hooks remain intentionally unsupported or post-V1 in the lifecycle map.
- [x] Rules/context injection with source files, glob matching, provenance text,
  and session-scoped priority/consume semantics is explicitly unsupported for
  strict V1 command hooks.
- [x] Hooks cannot bypass coordinator-owned permissions, event append authority,
  or replay side-effect boundaries.

### Tool and permission depth

- [x] Permission policy supports profile overrides, defaults, and selector rules.
- [x] Plan's codebase-edit restrictions and category recursion limits are
  enforced by configuration and permission checks, not prompt text alone.
- [x] Tool allow/deny posture is visible for every resolved profile in doctor JSON.
- [x] Read-only subagent restrictions are covered by tests that attempt edit, bash,
  task, and MCP calls where relevant.
- [x] Tool schemas and prompt descriptions agree on exact ids, aliases,
  permissions, and replay behavior.
- [x] Bash timeout, output cap, and blocked-command guidance are stated in both
  tool docs and agent prompt guidance.
- [x] Permission docs state the V1 threat model clearly: permissions are an
  operator approval layer, not a sandbox, and dangerous approvals can still affect
  the local workspace.

### Prompt-system evidence

- [x] A drift test fails when `.agent-harness/agents/*.md` assets referenced by
  docs/config are missing, empty, or frontmatter-only where a body is required.
- [x] A generated prompt snapshot exists for every shipped profile and category.
- [x] Prompt skeleton adherence is checked by a fixture that asserts every shipped
  profile includes identity, goal, use-when, do-not-use-when, scope guard,
  tool/permission posture, operating loop, ask gate, failure recovery, output
  contract, and verification gate sections.
- [x] A task delegation fixture proves skill content, category prompt append,
  parent/child lineage, sync/background behavior, and summary capping.
- [x] A permission fixture proves prompt promises match runtime enforcement for
  plan, explore, general, and category routes.
- [x] Doctor reports prompt asset status, skill catalog status, task route status,
  and profile tool/permission posture separately from provider health.

## V1 release blockers

Current release-readiness closeout evidence is recorded in
`docs/claim-evidence-matrix.md` and in the active implementation PRD evidence
artifacts. Remaining unchecked items below are still broader V1 or post-slice
work; checked items are only checked when the current tree has command output
or artifact roots cited there.

- [x] The README has one clear install, configure, and run path for a new user.
- [x] `docs/config.md` and checked-in schemas/examples agree on every public key.
- [x] `docs/architecture.md` describes all V1 runtime invariants and public
  events accurately.
- [x] `docs/testing.md` names the required V1 verification lanes and artifact
  expectations.
- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo check --workspace` passes.
- [x] `cargo test --workspace --all-features` passes, or any live-only exclusions
  are explicitly documented.
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  passes.
- [x] `cargo run -p harness -- --config configs/harness.example.jsonc config validate`
  passes.
- [x] `cargo run -p harness -- --config configs/harness.example.jsonc doctor`
  gives actionable output for missing provider, model, MCP, tool, and agent
  setup.
- [x] A manual TUI happy path has been recorded for start, prompt, permission,
  tool call, edit, resume, and quit.
- [x] Release-facing speed, provider breadth, compatibility, or parity claims are
  backed by current evidence artifacts, not README assertions or inspiration
  claims.
- [x] The V1 closeout bundle includes artifact roots for the release-blocking
  lanes, with `summary.txt`, `env.txt`, per-stage `command.txt`, `stdout.txt`,
  `stderr.txt`, `status.txt`, and `verification.txt` where the lane runner emits
  them.

## Verification and evidence posture

- [x] A canonical lane runner exists for deterministic, integration, signoff, and
  stress evidence.
- [x] The testing guide documents PTY, live, native visual, simulation, and stress
  lanes as separate provenance classes.
- [x] V1 defines release smoke tests for `harness --help`, `harness --version`,
  config validation, doctor, and one real or explicitly mocked provider call from
  outside the repository.
- [x] Release smoke includes one outside-repository TUI startup and one
  tool-enabled prompt path, so V1 evidence proves more than config preflight or
  `doctor` success.
- [x] V1 defines which checks are release blockers and which are local development
  aids.
- [x] V1 defines startup/readiness, TUI render, session resume, and binary size
  budgets before making performance claims.
- [x] Performance claims cite current artifacts with run provenance; stale or
  partial artifacts do not support release-facing claims.
- [x] Provider/model compatibility claims are backed by fixture or live-gated
  evidence for the named transport, not by provider catalog metadata alone.
- [x] Prompt, permission, compaction, and built-in-capability tests use faux/mock
  providers by default; real-provider tests are live-gated and never required for
  deterministic lanes.
- [x] Feature-specific fixtures exist for prompt assembly, task delegation,
  permission decisions, compaction summaries, and extension/built-in state.

## Distribution and first-run onboarding

- [x] V1 has one recommended install path for normal users.
- [x] V1 has one source-build path for contributors.
- [x] The released artifact supports `harness --help` and `harness --version` from
  outside the repository.
- [x] The first-run path explains how to create or copy a minimal `harness.jsonc`.
- [x] The first-run path explains provider/auth setup without assuming the local
  loopback provider already exists.
- [x] The first-run path ends with an exact first prompt and visible success
  signal, so a new user can tell the provider, tools, and session store worked.
- [x] `doctor` is documented as an install/config readiness check.
- [x] A real execution smoke test is documented separately from `doctor`, because
  a green readiness report does not prove the selected provider can complete a
  model call.
- [x] V1 install docs explain where sessions, config, skills, and artifacts live.
- [x] V1 has a troubleshooting page for auth, provider base URL, missing tools,
  session resume, terminal rendering, and permission prompts.

## Vanilla local-coding baseline

### CLI

- [x] The CLI entrypoint exists.
- [x] The interactive TUI can be launched from the CLI.
- [x] Headless prompt execution exists.
- [x] Scenario/headless run flows exist.
- [x] Config validation exists.
- [x] JSON schema output exists.
- [x] Doctor diagnostics exist.
- [x] Session list, inspect, replay, continue, export, tree, fork, and clone
  commands exist.
- [x] Model catalog generation/probing commands exist.
- [x] CLI help text has been reviewed as a complete V1 user surface.
- [x] CLI command names and docs are audited against the README quick start.

### TUI

- [x] Ratatui startup/live/replay/scenario modes exist.
- [x] Live sessions render a transcript-first shell.
- [x] Replay mode is read-only.
- [x] Permission and question overlays exist.
- [x] Diff rendering exists for edit review.
- [x] Operator sidebar and secondary surfaces exist.
- [x] Keybinding override plumbing exists.
- [x] Slash commands exist for model/status/toggles/resume/new/tree/fork/clone.
- [x] Prompt `@` mentions can suggest workspace files, agents, and MCP resources.
- [x] Startup is prompt-first by default, with session browsing secondary rather
  than the initial focus.
- [x] Prompt history is durable across sessions.
- [x] Prompt history navigation preserves drafts and cursor intent.
- [x] Command palette metadata is centralized and reused by slash commands,
  help, and keybinding surfaces.
- [x] Permission overlays show shortcuts, scope, and timeout/countdown state
  clearly.
- [x] Model switching shows provider-grouped search and visible fallback/error
  status.
- [x] Session search supports visible fielded or fuzzy filtering.
- [x] Subagent/background work is keyboard-navigable from the operator surface.
- [x] Diff review supports next/previous hunk navigation.
- [x] Approve/deny, diff review, resume, and replay failure states have visible
  operator flows covered by deterministic PTY or snapshot evidence.
- [x] UI signoff records startup screen, command palette, session picker,
  and diff review coverage through deterministic snapshot or PTY evidence;
  reference-image comparison is not required for this PRD.
- [x] New keybindings are registered through configurable keybinding defaults,
  not hardcoded checks scattered through TUI code.
- [x] Session tree/sidebar navigation has keyboard-first controls before any
  pointer-only polish is considered complete.

### Sessions and replay

- [x] Events are the source of truth.
- [x] Replay derives state from event logs without executing tools or providers.
- [x] Session lineage, tree, fork, and clone surfaces exist.
- [x] Background child-task wakeup events are modeled.
- [x] Tool summaries and artifacts are persisted with redaction/capping policy.
- [x] Model-visible session tools exist: `session_list`, `session_read`,
  `session_search`, and `session_info`.
- [x] Resume behavior has a documented V1 acceptance test covering a realistic
  interrupted session.
- [x] Session resume/list performance is measured against a large enough local
  session corpus before V1 claims fast long-session behavior.
- [x] Crash-resilient session write behavior is documented and tested at the
  event-store boundary.
- [x] Compaction summaries preserve enough file, tool, skill, todo, and plan
  context for a resumed session to continue without guessing.
- [x] Branch/fork/clone session flows document how summaries, artifacts, and
  restored context behave across lineage.
- [x] Session list/resume surfaces show meaningful generated titles, not only
  paths or opaque run ids. Editable titles are tracked by the OpenCode/Pi
  backend hardening PRD until `UpdateSessionTitle` ships.
- [x] A redacted support export or bug-report bundle captures enough session
  events, artifacts, doctor output, and provider/config summary to debug V1 user
  failures without leaking secrets.

### Providers and models

- [x] Mock provider support exists for deterministic runs.
- [x] OpenAI-compatible provider transport exists.
- [x] Config supports provider/model definitions.
- [x] Model variants are supported in config examples.
- [x] Generated provider catalog maintenance exists.
- [x] Provider errors are surfaced with enough context for non-expert users.
- [x] Runtime model fallback policy is defined for V1.
- [x] The V1 provider support statement is explicit: OpenAI-compatible execution
  first, broader catalog metadata as reference unless implemented.
- [x] Provider errors use stable, user-actionable categories such as missing
  credentials, invalid credentials, rate limit, context-window overflow,
  unsupported tool call, malformed stream, and transport failure.

### Config and doctor

- [x] Runtime config uses `harness.json` / `harness.jsonc` as the canonical
  public shape.
- [x] TUI config uses `tui.json` / `tui.jsonc` as the canonical public shape.
- [x] Generated runtime and TUI schemas are checked in.
- [x] `configs/harness.example.jsonc` is the canonical runtime example.
- [x] `configs/tui.example.jsonc` is the canonical TUI example.
- [x] Unsupported upstream product areas are rejected or accepted only when
  inactive: `server`, `command`, `plugin`, `share`, `autoupdate`, and
  `enterprise`.
- [x] Config-backed MCP servers are part of the runtime tool registry.
- [x] Doctor reports the complete resolved agent list, including primary agents,
  subagents, hidden profiles, and category routes.
- [x] Doctor reports stale/missing built-in asset references.
- [x] Doctor reports extension/roadmap readiness separately from runtime health.

## Native tool baseline

- [x] Workspace read/list/glob/grep tools exist.
- [x] Hashline edit tooling exists.
- [x] Bash/shell execution exists behind permission and safety controls.
- [x] Web fetch/search/code search tools exist.
- [x] LSP diagnostics/symbol/reference/rename tools exist.
- [x] `question` exists.
- [x] `skill` exists.
- [x] `todoread` and `todowrite` exist.
- [x] `task` exists as the canonical child-delegation tool.
- [x] `background_output` exists.
- [x] `batch` exists.
- [x] Config-backed MCP tools exist.
- [x] `background_cancel` exists as a dedicated user-facing tool instead of only
  cancellation through `background_output` arguments.
- [x] AST-grep search is a first-class read-only native tool.
- [x] AST-grep replace is a first-class native tool.
- [x] Model-visible session tools are first-class native tools.
- [x] Native tool docs include a concise V1 tool catalog.
- [x] Native tool parity tests cover the full V1 tool catalog.

## Agents and subagents

### Primary agents

- [x] `build` exists as the default implementation lane.
- [x] `plan` exists as a planning lane with codebase-edit restrictions and a
  controlled handoff back to Build.
- [x] Primary-agent picker/docs make clear that primary agents are operator modes,
  not search helpers.
- [x] Primary-agent prompt bodies are reviewed for V1 quality rather than only
  frontmatter/runtime synthesis.

### Subagents

- [x] `general` exists as a subagent profile.
- [x] `explore` exists as a read-only local code-search subagent profile.
- [x] Category routes exist for `visual-engineering`, `artistry`, `ultrabrain`,
  `deep`, `quick`, `unspecified-low`, `unspecified-high`, and `writing`.
- [x] Category profiles are intended for `task(category = "...")` delegation and
  deny recursive task delegation by default.
- [x] `task(subagent_type = ...)` and `task(category = ...)` behavior is documented
  in a user-facing V1 guide.
- [x] `task` has a clear contract for sync vs background execution, cancellation,
  continuation, and skill loading.
- [x] Subagent output is summarized in a way that keeps parent context lean.
- [x] Category route model/variant/fallback resolution is centralized in an
  `AgentCatalog`-style seam.

## Orchestration-inspired V1 release work

These are useful without forcing a full orchestration-style agent OS. The detailed
acceptance criteria live in the sections above; this section is a compact
cross-reference for agents choosing the next implementation slice.

- [x] Hashline editing.
- [x] Simple subagents through `task`.
- [x] Category routing through `task(category = ...)`.
- [x] Markdown skill loading.
- [x] Config-backed MCP registration.
- [x] Stricter delivery remains prompt and tool guidance, not an extra primary agent.
- [x] Built-in `git-master` skill; see Skill depth.
- [x] Built-in `review-work` skill; see Skill depth.
- [x] Built-in `frontend-ui-ux` or equivalent visual-engineering skill; see Skill
  depth.
- [x] AST-grep search; see Native tool baseline.
- [x] AST-grep replace; see Native tool baseline.
- [x] Model-visible session tools; see Sessions and replay plus Native tool
  baseline.
- [x] Dedicated `background_cancel` tool; see Native tool baseline.
- [x] Doctor checks for built-in skills, category routes, and missing assets; see
  Prompt-system evidence plus Config and doctor.
- [x] A small first-party slash-action/native lifecycle-hook seam for built-in
  behavior is documented, distinct from arbitrary executable plugins; see
  Command and hook depth plus Extension and plugin strategy.

## Extension and plugin strategy

V1 should be plugin-ready, not a broad arbitrary plugin host.

- [x] Config-backed MCP gives a safe external-tool integration path today.
- [x] Skills provide a markdown-based instruction extension path today.
- [x] A typed extension manifest seam exists for optional tools, hooks, commands,
  prompts, MCP bundles, diagnostics, and provider decorators.
- [x] Extension tool descriptors declare public permission names, but extension-provided
  tools are not registered or executed in V1 and no runtime permission path
  exists yet.
- [x] Replay support for extension manifests is limited to static descriptor/config
  metadata; it does not render extension tool events or load extension code.
- [x] Current built-in lifecycle behavior is on the native lifecycle hook seam,
  while future extension command-hook migration remains gated on the typed
  manifest seam.
- [x] Hook phases are explicitly modeled for native provider params, tool
  preflight/result, permission, run, agent turn, subagent, and compaction
  phases; context transform and session idle remain unsupported/post-V1.
- [x] External executable/script plugins are deferred until command mediation,
  sandboxing, and replay-safe manifests are proven.
- [x] Active upstream plugin compatibility is explicitly post-V1.
- [x] Built-in extension-like features are disableable by stable ids before any
  external plugin runtime is introduced.
- [x] Extension strategy follows the extension-first and TypeScript baselines:
  keep core changes small, keep authority explicit, and require conformance
  evidence for every new extension surface.


  create/status/message/task/shutdown/delete as primitive event tools, not as
  optional layer.
  release scope.

## Explicitly post-V1 unless re-scoped

- [ ] Ralph loop / ultrawork loop.
- [ ] Todo enforcer or autonomous idle continuation loop.
- [ ] Prometheus / Metis / Momus / Atlas orchestration stack.
- [ ] Full specialist persona catalog.
- [ ] Skill-embedded MCP lifecycle.
- [ ] MCP OAuth lifecycle.
- [ ] Browser automation / Playwright skill bundle.
- [ ] Media analysis / `look_at` / multimodal-looker.
- [ ] Interactive tmux terminal tool.
- [ ] Arbitrary upstream plugin loading.
- [ ] Upstream server/share/auth/account product surfaces.
- [ ] Desktop, web, mobile, or PWA clients.
- [ ] IDE integration, editor-selection sync, or editor-side diff control.
- [ ] GitHub Action, Slack, Discord, Telegram, OpenClaw, or other remote
  collaboration bots.
- [ ] Cloud, enterprise, billing, analytics, telemetry, or hosted API surfaces.
- [ ] JS/QuickJS/WASM extension marketplace or broad extension runtime.
- [ ] Beads, Agent Mail, RCH, swarm, or external issue/validation-broker
  coordination systems.
- [ ] Math-heavy adaptive optimization controllers, shadow dual execution,
  online policy evaluation, or VOI planners.
- [ ] Broad non-OpenAI-compatible provider transports beyond the implemented execution path.
- [ ]   OS-level execution sandbox for build/plan tool execution (Linux
  Landlock+seccomp, macOS Seatbelt), distinct from the operator permission layer;
  Windows remains best-effort/unsupported initially. See
  [`docs/agent_harness_opencode_ui_pi_backend_prd.md`](agent_harness_opencode_ui_pi_backend_prd.md) §5.
- [ ] Native Anthropic transport with explicit `cache_control` ephemeral
  breakpoints and per-model-capability TTL gating.
- [ ] Server-side context reuse (`previous_response_id`) for the Responses API,
  with replay-safety design for server-held state.
- [ ] OAuth/provider integrations beyond Codex and GitHub Copilot, including
  regional/Chinese providers.
- [ ] Standardized external MCP config import (`.mcp.json`-style) for interop.
- [ ] Harness logo / brand redesign (human-owned design task, not an
  autonomous-agent deliverable).

## Explicit V1 non-goals from the inspiration review

- [x] Do not add a second build system such as Bazel beside the Cargo workspace.
- [x] Do not copy another harness architecture mechanically; extract behavior and
  reimplement it through Harness modules and event contracts.
- [x] Do not use source-brand parity claims as release claims without current
  Harness evidence.
- [x] Do not add broad compatibility shims that bypass canonical Harness tool ids,
  permissions, or replay safety.

## V1 polish additions from the hyperplan review

These additions sharpen the release target after comparing the roadmap against
the checked-in inspiration material. They are V1 work only where they make the
vanilla local-coding product trustworthy and polished; broader agent-OS,
desktop/mobile, browser/media, OAuth MCP, and arbitrary plugin work stays
post-V1 unless this roadmap explicitly re-scopes it.

### Release evidence and claim integrity

- [x] A V1 claim-to-evidence matrix maps every release-facing claim to a
  deterministic test, manual artifact, fixture, command output, or explicit
  documented limitation.
- [x] A release blocker taxonomy classifies open work as correctness, safety,
  UX, docs, provider, performance, or evidence work so checked foundations are
  not confused with V1-quality surfaces.
- [x] Release-facing performance, startup, binary-size, provider, and parity
  claims cite current artifacts with run provenance and fail closed when
  evidence is stale or partial.
- [x] The V1 closeout bundle includes an operator-readable summary that ties
  README/config/docs claims back to the evidence matrix.

### Operator happy path and TUI signoff

- [x] A scripted operator happy path covers install, config, provider readiness,
  TUI startup, one tool-enabled prompt, permission approval/denial, edit review,
  diff inspection, resume, replay, doctor, and support export.
- [x] TUI visual signoff artifacts cover at least one normal prompt flow, one
  permission/question flow, one provider/tool failure flow, and one resume flow.
- [x] The TUI has release-quality operator surfaces for model/provider status,
  permission scope, tool progress, diff review, session navigation, and
  recoverable failure states.
- [x] TUI polish inspired by source-reference terminal tools is scoped to terminal-first V1
  surfaces: prompt history, session sidebar/search, diff review, model/status
  clarity, and question/permission overlays; desktop/mobile/PWA polish remains
  post-V1.

### Provider, permission, and privacy readiness

- [x] A V1 provider support matrix states the supported execution path, known
  limits, model fallback policy, credential expectations, and named error
  categories.
- [x] Provider errors have stable user-actionable categories and TUI/headless
  recovery text for missing credentials, invalid credentials, rate limits,
  context overflow, unsupported tool calls, malformed streams, and transport
  failures.
- [x] A permission threat model explains that permissions are an operator
  approval layer rather than a sandbox, names the mutable surfaces, and links
  prompt promises to runtime enforcement fixtures.
- [x] Privacy and local-data notes explain what can leave the machine, where
  sessions and artifacts live, how redaction works, and which cloud/telemetry
  features are absent unless added later.

### Session, resume, and compaction trust

- [x] Resume acceptance evidence covers an interrupted realistic coding session
  with tool artifacts, permission state, summaries, and restored context.
- [x] Large-session list/resume/search behavior is measured before V1 makes fast
  long-session claims.
- [x] Crash-resilient session write behavior is tested at the event-store
  boundary and documented for support/debugging.
- [x] Compaction preservation tests prove file, tool, skill, todo, and plan
  context survive summary generation well enough for a resumed session to
  continue without guessing.
- [x] Session list, search, tree, fork, and clone surfaces expose meaningful
  generated titles instead of only paths or opaque run ids. Editable titles are
  tracked by the OpenCode/Pi backend hardening PRD until `UpdateSessionTitle`
  ships.

### Built-in skills and prompt rigor

- [x] The minimal V1 built-in skill set is named explicitly and each skill has a
  real body, use-when/do-not-use-when guidance, docs, disablement behavior, and
  tests before it is advertised as shipped.
- [x] README and docs do not advertise `git-master`, `review-work`,
  `frontend-ui-ux`, or equivalent skills as release-ready until the evidence
  matrix links them to V1-quality bodies and tests.
- [x] Prompt bodies and category profiles distinguish template scaffolding from
  agent-specific operating guidance, with golden snapshots for the shipped
  prompt set.
- [x] Intent-gate behavior is covered by prompt fixtures for ambiguous requests,
  investigation requests, implementation requests, and planning requests.

### Typed extension seam without plugin sprawl

- [x] A typed extension manifest seam defines optional tools, hooks, commands,
  prompts, MCP bundles, diagnostics, provider decorators, capability ids,
  disablement state, and replay-safe descriptor metadata.
- [x] Extension-provided behavior is not registered, executed, permissioned, or
  replay-rendered in V1; future runtime support must go through coordinator-owned
  permissions, event append authority, artifact redaction, and replay side-effect
  boundaries.
- [x] Built-in extension-like features use the public Harness interfaces and
  stable ids before external plugin compatibility is considered.
- [x] Migration notes explain which source-inspiration, desktop/mobile,
  browser/media, and plugin surfaces are unsupported
  by design for V1.

## V1 enhancement additions (provider auth, caching, prompt parity)

These items extend V1 beyond the checked foundations with high-leverage,
well-scoped capability for a vanilla local-coding harness. They are the scope of
[`docs/auth-model-parity-prd.md`](auth-model-parity-prd.md), which holds the
strict end-state goal, anti-gaming contract, workstreams, and evidence rules.
Heavier items (OS sandbox, native Anthropic transport, server-side response
reuse, additional providers, logo) stay post-V1 in the list above. All boxes
below are unchecked until backed by the PRD's cited evidence.

### Provider authentication (OAuth): Codex and GitHub Copilot first

- [x] A provider credential abstraction supports `apiKey`/`apiKeyEnv` and an
  `oauth` credential kind without adding a new transport protocol;
  OpenAI-compatible execution stays the base path. Evidence:
  `cargo test -p harness-core auth -- --nocapture`; `cargo test -p harness-providers openai_compatible_uses_credential_source_before_static_api_key -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] OAuth credentials persist in a dedicated store outside `harness.json`, in the
  platform data dir, with restrictive permissions, and never appear in
  `events.jsonl`, support bundles, or committed files. Evidence:
  `cargo test -p harness --test replay_sessions_cli_test sessions_export_cli_excludes_stored_credentials_and_scans_for_leaks -- --nocapture`; `cargo check -p harness-core`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] OAuth access tokens refresh automatically from the stored refresh token with
  single-flight behavior; failures map to the existing provider error categories.
  Evidence: `cargo test -p harness-core auth -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Codex (ChatGPT) OAuth login works via PKCE loopback (browser) and
  device-code (headless) flows, with requests decorated by bearer token, account
  id, and the Codex endpoint without leaking secrets. Evidence:
  `cargo test -p harness-core codex -- --nocapture`;
  `cargo test -p harness auth -- --nocapture`;
  `cargo test -p harness-providers codex_auth_profile_rewrites_endpoint_and_adds_context_headers -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] GitHub Copilot OAuth login works via the GitHub device-code flow, with public
  and enterprise deployment options and the required Copilot request headers.
  Evidence: `cargo test -p harness-core copilot -- --nocapture`;
  `cargo test -p harness-providers github_copilot_auth_profile_rewrites_public_and_enterprise_headers -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] `harness auth login/logout/list` CLI commands exist and a skippable TUI
  first-run login flow exists, neither printing secrets. Evidence:
  `cargo test -p harness auth -- --nocapture`;
  `cargo test -p harness-tui onboarding -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Stored Codex and GitHub Copilot credentials activate built-in provider/model
  catalogs for CLI/TUI/prompt use without requiring a project `harness.json`;
  `/model` groups authenticated provider rows, persists valid recent selections,
  and selected provider/model metadata routes the next prompt. Evidence:
  `cargo test -p harness runtime_catalog -- --nocapture`;
  `cargo test -p harness no_config_tui -- --nocapture`;
  `cargo test -p harness prompt::tests::no_config_prompt -- --nocapture`;
  `cargo test -p harness-tui model_switcher -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Doctor reports per-provider auth status (kind, presence, expiry) with
  redacted values, separate from transport health. Evidence:
  `cargo test -p harness auth -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] OAuth flows are proven with deterministic fixture/mock tests; live OAuth is
  env-gated/manual only and never required for deterministic lanes. Evidence:
  `cargo test -p harness-core codex -- --nocapture`;
  `cargo test -p harness-core copilot -- --nocapture`;
  `scripts/test-lanes.sh simulation`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).

### Prompt-cache optimization (OpenAI-compatible path, reference cache implementation parity)

- [x] OpenAI-compatible requests set a stable, clamped, per-session
  `prompt_cache_key` to maximize cache routing and hit rate. Evidence:
  `cargo test -p harness-providers openai_compatible_ -- --nocapture`; source
  citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] The composed system prompt keeps volatile fields (date, git branch) at the
  tail of the stable prefix, covered by a composition-order test. Evidence:
  `cargo test -p harness dynamic_prompt_keeps_volatile_environment_at_stable_prefix_tail -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Cache read/write token telemetry is surfaced in an operator-visible TUI
  status, derived from existing event fields. Evidence:
  `cargo test -p harness-tui cache_read_write_tokens_render_as_separate_status_labels -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).

### Model resolution and prompt parity

- [x] Model-family/capability resolution uses an explicit, tested seam (not only
  `model_id.contains(...)` heuristics) for prompt selection and per-family request
  behavior, with a documented default fallback. Evidence:
  `cargo test -p harness-core --test model_variant_resolution_test -- --nocapture`,
  `cargo test -p harness-core model_resolution -- --nocapture`, and
  `cargo test -p harness provider_prompt_uses_resolved_metadata_family_not_model_substrings -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] The roadmap's substring-heuristic vs explicit-preset claim is reconciled
  honestly with a citation. Evidence:
  [`docs/config.md`](config.md#v1-model-prompt-tuning-stance) plus
  `cargo test -p harness --test config_docs_reference_test -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Non-GPT family prompts (Anthropic, Gemini, and any Copilot-exposed families)
  meet the shared skeleton at reference model implementation-parity quality, branding-stripped, with golden
  snapshots. Evidence:
  `cargo test -p harness family_prompt -- --nocapture` and
  `cargo test -p harness --test bootstrap_profiles_test shipped_v1_family_prompt_assets_match_golden_snapshots -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Model-family prompt bodies are sourced from data assets rather than hardcoded
  Rust string constants, with a drift test for missing/empty assets. Evidence:
  `cargo test -p harness family_prompt -- --nocapture` and
  `cargo test -p harness --test config_schema_cli_test prompt_family_asset -- --nocapture`;
  source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).

### Skill hardening

- [x] Bundled skill references/assets load via progressive disclosure within
  documented caps, instead of remaining deferred-only metadata. Evidence:
  `cargo test -p harness-tools --test skill_load_discovery_test -- --nocapture`;
  sources: `docs/auth-model-parity-progress.md`, `docs/extension-strategy.md`,
  `docs/starter-skills.md`.
- [x] Skill discovery has symlink-escape and path-traversal tests across all
  configured project/global roots. Evidence:
  `cargo test -p harness-tools --test skill_load_discovery_test -- --nocapture`;
  sources: `docs/auth-model-parity-progress.md`,
  `crates/harness-tools/tests/skill_load_discovery/03_v1_skill_contract_test.rs`.

### First-run onboarding and UX

- [x] A skippable first-run onboarding flow (provider/auth selection → first prompt
  → visible success) exists in the TUI, adapted from the reference implementation onboarding UX
  without its branding, and does not block pre-configured users. Evidence:
  `cargo test -p harness-tui onboarding -- --nocapture`; `cargo test -p harness auth -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).
- [x] Skill listing/selection UX in the TUI is aligned with the reference implementation skill
  surface where it improves clarity. Evidence:
  `cargo test -p harness-tui onboarding -- --nocapture`; source citation:
  [`docs/auth-model-parity-progress.md`](auth-model-parity-progress.md).

## Documentation deliverables

- [x] README quick start is accurate and minimal.
- [x] Config guide is accurate for V1.
- [x] Architecture guide is accurate for V1.
- [x] Testing guide is accurate for V1.
- [x] Native tool catalog exists.
- [x] Agent and subagent guide exists.
- [x] Permissions guide exists.
- [x] Sessions and replay guide exists.
- [x] Extension strategy guide exists, clearly marking post-V1 plugin work.
- [x] Privacy and local-data notes explain what leaves the machine, where sessions
  and artifacts are stored, how redaction works, and that telemetry/cloud features
  are absent unless explicitly introduced later.
- [x] Migration notes explain which source-inspiration areas are unsupported by design.

## Suggested implementation order

- [x] Freeze V1 scope and add the release blocker taxonomy plus claim-to-evidence
  matrix before adding new user-facing release claims.
- [x] Clean documentation and asset drift first, because stale prompt/agent
  references make every later checklist item ambiguous.
- [x] Lock install, config, provider, `doctor`, and one prompt smoke from outside
  the repository, because V1 starts with a user being able to run the binary.
- [x] Add the scripted operator happy path and TUI visual signoff checklist before
  treating the TUI as release-ready.
- [x] Make startup prompt-first and improve prompt history, because this is the
  highest-frequency vanilla local-coding interaction.
- [x] Centralize command/keybinding metadata before expanding keybindings, slash
  commands, or help text.
- [x] Improve permission modal clarity before adding more powerful tools, so new
  capabilities inherit a clear approval surface.
- [x] Harden provider error categories, fallback policy, and support-matrix docs
  before claiming provider breadth beyond the implemented execution path.
- [x] Lock resume, large-session, crash-write, and compaction preservation
  evidence before making session durability or long-session performance claims.
- [x] Add the permission threat model and privacy/local-data notes before adding
  more extension-like or externally integrated capabilities.
- [x] Add prompt bodies, prompt snapshots, and task/permission fixtures before
  expanding subagent or skill catalogs.
- [x] Add an `AgentCatalog`-style resolution seam before relying on category
  fallbacks, hidden profiles, or category-specific prompt appends.
- [x] Add model-visible session tools after session/replay acceptance tests are
  documented.
- [x] Add dedicated `background_cancel` after sync/background task contracts are
  documented.
- [x] Add AST-grep search after the native tool catalog shape and
  parity-test harness are stable, then update the catalog and matrix for
  AST-grep.
- [x] Add V1 built-in skills after skill schema, precedence, disablement, and
  progressive disclosure are documented.
- [x] Define the typed extension manifest seam before migrating lifecycle command
  hooks onto it or marking the extension strategy guide complete.
- [x] Add doctor readiness checks once the prompt, skill, tool, and agent catalog
  contracts are stable enough for diagnostics to enforce.
  deliberately re-opened; otherwise document it as a post-V1 optional layer.
