# Inspiration gap analysis

This document ranks worthwhile gaps between Harness and the local references under
`inspirations/`. It is not a parity checklist for cloning every upstream feature.
Harness has its own event-sourced runtime contract: the coordinator owns event
append, task scheduling, permission resolution, tool execution re-entry,
compaction, and replay purity. A worthwhile gap is therefore a reference feature
that can improve Harness without weakening those invariants.

## Scope and evidence

Inspected Harness evidence:

- `README.md` for the public quick start, Plan workflow, stress harness, and
  session lineage commands.
- `docs/architecture.md` for crate boundaries, event schema, coordinator
  invariants, permission model, tool surface policy, hashline editing,
  compaction, and replay contract.
- `docs/config.md` for the public config contract, provider/model catalog
  generation, agent profiles, task/background output metadata, permissions,
  Plan operator workflow, and compaction knobs.
- `docs/testing.md` for deterministic, integration, PTY, live, native visual,
  and stress lanes.
- `crates/harness-*/AGENTS.md` for local crate ownership and implementation
  constraints.

Inspected inspiration evidence:

- `inspirations/opencode/README.md` for distribution, desktop app,
  provider-agnostic positioning, TUI focus, and client/server architecture.
- `inspirations/codex/README.md` for local CLI distribution, IDE and desktop
  entry points, and ChatGPT-plan authentication.
- `inspirations/oh-my-openagent/README.md`,
  `inspirations/oh-my-openagent/docs/reference/features.md`, and
  `inspirations/oh-my-openagent/AGENTS.md` for discipline agents, categories,
  hooks, skills, MCPs, Team Mode, IntentGate, Ralph/ultrawork loops, tmux,
  diagnostics, and compatibility surfaces.
- `inspirations/pi_agent_rust/README.md`, `AGENTS.md`, `docs/`, `examples/`,
  `fuzz/`, and `benches/` for provider breadth, RPC, extension runtime,
  capability policy, performance governance, session indexing, conformance,
  fuzzing, installer, diagnostics, and large-session storage.
- `inspirations/pi-mono/README.md` and `AGENTS.md` for package layering,
  provider onboarding discipline, generated model registry rules, and TUI test
  expectations.
- `inspirations/senpi/README.md` and `AGENTS.md` for extension-first fork
  strategy, builtin extensions, dynamic prompts, prompt presets, permission
  system, parallel tool routing, compaction extensions, and differential TUI
  budgets.
- `inspirations/opencode-ui-images/` and
  `inspirations/screenshots opencode ui parity/` for visual affordances around
  start screen, command palette, session views, session picker, and diff review.

## Ranking rubric

Ranks use these factors, in order:

1. **Invariant impact:** whether the gap affects replay, permissions,
   scheduling, tool execution, provider boundaries, or durable session state.
2. **Leverage:** whether one implementation seam enables many later features.
3. **Operator value:** whether users get a materially better daily workflow.
4. **Testability:** whether the gap can be locked by deterministic lanes,
   schema drift tests, PTY snapshots, or live opt-in gates.
5. **Fit:** whether the feature fits Harness's Rust, event-sourced design.

Classifications:

- **Architecture gap:** needs a new or deepened module seam.
- **Parity gap:** a reference feature is missing or materially narrower.
- **Operational gap:** affects packaging, diagnostics, support, or evidence.
- **UX gap:** affects TUI/operator workflow.
- **Intentional divergence:** Harness is different for a good reason, but should
  document or preserve the divergence.

## Ranked gaps

| Rank | Gap | Classification | Importance | Suggested first test lane |
| ---: | --- | --- | --- | --- |
| 1 | Coordinator-owned extension seam | Architecture gap | Unlocks safe plugins, builtin extensions, provider-native tools, hooks, and package-like distribution without bypassing permissions or replay. | `integration` |
| 2 | Provider and credential breadth | Parity gap | Harness currently executes only OpenAI-compatible transports, while references treat provider breadth and auth diagnostics as core product value. | `integration`, `signoff-live` |
| 3 | Performance and large-session evidence governance | Operational gap | Pi Rust treats startup, resume, memory, session storage, and perf claims as gated artifacts; Harness has stress lanes but not equivalent budgets or claim gates. | `stress-offline`, new perf lane |
| 4 | Session index and large-history storage fast path | Architecture gap | Harness has JSONL replay and lineage, but references add SQLite/sidecar indexes, stale reindexing, and migration validation for fast resume at scale. | `integration`, new storage stress |
| 5 | RPC, client/server, and external frontend API | Architecture gap | OpenCode and Pi expose remote/client integration paths; Harness is primarily CLI/TUI/prompt oriented. | `integration`, protocol tests |
| 6 | Autonomous discipline-agent and category routing system | Parity gap | Harness has build/plan/explore/general; OMO/Senpi add specialist agents, category-to-model routing, fallback chains, and plan reviewers. | `integration`, config drift |
| 7 | Persistent todo and continuation loop | Parity gap | OMO/Senpi use todos, Ralph/ultrawork loops, and idle recovery to keep long tasks moving; Harness has task lifecycle but no first-class goal/todo continuation surface. | `integration`, coordinator tests |
| 8 | Skill-embedded MCP and scoped tool bundles | Architecture gap | Harness has skills and config-backed MCP separately; references let skills bring on-demand MCP servers and permissions. | `integration`, MCP tests |
| 9 | Dynamic prompt builder and per-model prompt presets | Parity gap | Harness supports prompt assets and model profiles, but references adapt prompt content by intent, model family, category, and tool protocol. | config/prompt tests |
| 10 | Text tool-call middleware and provider-native tool adapters | Architecture gap | References support XML/Hermes/YAML/Gemma text tools and provider-native web/code tools; Harness mainly normalizes OpenAI-compatible function/tool streams. | provider tests, live gates |
| 11 | Stronger shell and extension execution safety | Architecture gap | Harness has permissions and Plan shell guards; Pi/Senpi add command mediation, dangerous-pattern classification, risk ledgers, trust lifecycle, and sandbox backends. | `integration`, security tests |
| 12 | Diagnostics, doctor, and self-healing config checks | Operational gap | OMO/Pi expose `doctor` commands for config, models, auth, registration, sessions, and extensions; Harness has config validation but no holistic health check. | CLI tests |
| 13 | Install, release, and migration packaging | Operational gap | OpenCode/Codex/Pi ship install scripts, package-manager paths, binary releases, completions, migration helpers, and uninstall flows; Harness is mostly Cargo-run oriented. | packaging smoke tests |
| 14 | Session export, share, and dataset workflow | Operational gap | Pi-mono emphasizes publishing OSS sessions; Pi Rust supports HTML export. Harness has artifacts and lineage, but no share/export workflow framed for review or datasets. | replay/session CLI tests |
| 15 | Interactive terminal/tmux lane | UX gap | OMO and pi-mono document tmux-driven live terminal interaction; Harness has bash and PTY signoff but no user-facing persistent interactive terminal sessions. | PTY lane |
| 16 | Autocomplete and inline context attachment | UX gap | Pi offers `@file` references, fuzzy slash completions, skills/templates/files in editor completions, and background refresh. Harness has slash commands and read tools, but no comparable composer indexing contract. | TUI tests, PTY lane |
| 17 | Dedicated command and diff review surfaces | UX gap | OpenCode screenshots show dedicated command windows and diff review. Harness has command palette snapshots and inline diff rendering, but not a dedicated review workflow. | TUI snapshots, PTY lane |
| 18 | Visual/perf TUI budgets | Operational/UX gap | Senpi enforces differential-rendering/flicker budgets and Pi tracks render performance. Harness has PTY/native visual signoff, but not explicit redraw or frame-budget contracts. | TUI tests, native visual |
| 19 | Multi-agent team coordination and file reservations | Architecture gap | OMO Team Mode and Pi Agent Mail include shared task lists, mailboxes, file claims, and optional worktrees. Harness supports child tasks, but not cross-agent conflict management. | integration tests |
| 20 | Gap ledger and parity evidence database | Operational gap | Pi Rust maintains machine-readable parity, evidence, and certification ledgers. Harness has docs and drift tests, but no durable ranked gap ledger tied to owners/status. | docs drift tests |

## Detailed gaps

### 1. Coordinator-owned extension seam

**Classification:** Architecture gap.

**Inspiration evidence:**

- OMO has plugin initialization phases for config, managers, tools, hooks, and
  plugin interface in `inspirations/oh-my-openagent/AGENTS.md`.
- OMO documents 52 hooks, 26 tools, built-in MCPs, skill MCPs, background
  agents, and feature modules in `inspirations/oh-my-openagent/AGENTS.md`.
- Pi Rust describes a QuickJS/native extension runtime with capability-gated
  hostcalls in `inspirations/pi_agent_rust/README.md`.
- Senpi keeps upstream changes small by landing features as builtin extensions in
  `inspirations/senpi/README.md`.

**Harness evidence:**

- `docs/config.md` accepts `plugin` only when empty and explicitly says plugins
  are not loaded.
- `docs/architecture.md` requires coordinator-owned event append, scheduling,
  permission resolution, tool execution, artifacts, and replay.
- `crates/harness-tools/AGENTS.md` requires stable native tool IDs, typed
  schemas, and coordinator policy enforcement.

**Why it matters:**

Most large inspiration features are extension-like: provider-native tools,
commands, hooks, skill MCPs, sandbox policies, prompt presets, and diagnostics.
Adding them ad hoc to core would make modules shallow and widen the coordinator.
The deeper seam is a coordinator-owned extension runtime where adapters can
register tools, commands, prompts, hooks, and provider decorators, but every
side effect still re-enters the coordinator.

**Recommended shape:**

- Start with a manifest-only extension seam for in-process Rust adapters before
  embedding JS/QuickJS.
- Extension registration should be declarative: tool schemas, permissions,
  event hooks, prompt additions, config schema fragments, and artifact contracts.
- Runtime hook callbacks must not append events directly. They should return
  coordinator commands or advisory data.
- Keep config support explicit. Do not silently enable OpenCode `plugin` fields;
  introduce Harness-owned extension config with migration text.

**Acceptance criteria:**

- Extension tools appear in the registry only through coordinator-owned
  registration and permission gates.
- Replay never executes extension code.
- Extension events, artifacts, and denials are redacted and durable.
- Disabling an extension removes its tools/commands without corrupting old logs.
- Executable extensions cannot ship until the shell/extension safety work in gap
  11 has command mediation, redaction, and incident evidence coverage.
- Extension adapters have conformance fixtures that prove registration,
  permission denial, artifact persistence, disable/unload behavior, and replay
  safety.

### 2. Provider and credential breadth

**Classification:** Parity gap.

**Inspiration evidence:**

- OpenCode highlights provider-agnostic use with Claude, OpenAI, Google, or local
  models in `inspirations/opencode/README.md`.
- Pi Rust lists native provider modules for Anthropic, OpenAI Chat, OpenAI
  Responses, Gemini, Cohere, Azure, Bedrock, Vertex, GitHub Copilot, and GitLab
  Duo in `inspirations/pi_agent_rust/README.md` and `AGENTS.md`.
- Pi Rust documents API key, OAuth, AWS, service-key, bearer-token, and provider
  diagnostic flows in `inspirations/pi_agent_rust/README.md`.
- Pi-mono has a provider onboarding checklist across core types, provider
  implementation, generated models, tests, coding-agent docs, and changelog in
  `inspirations/pi-mono/AGENTS.md`.

**Harness evidence:**

- `crates/harness-providers/AGENTS.md` documents deterministic mock plus
  OpenAI-compatible Chat/Responses transports.
- `docs/config.md` says models.dev describes many providers, while Harness
  currently executes only OpenAI-compatible transports.
- `docs/testing.md` has live-provider lanes, but they are centered on the
  configured OpenAI-compatible provider path.

**Why it matters:**

Provider breadth is not just model selection. It affects authentication,
streaming semantics, tool-call formats, token accounting, native provider tools,
fallback policy, and live signoff. Harness's current OpenAI-compatible adapter is
a strong seam, but references show provider diversity as a first-class operator
need.

**Recommended order:**

1. Add provider onboarding docs and a provider test-obligation matrix.
2. Add credential status diagnostics without storing secrets in events.
3. Add the next native transport that most differs from OpenAI-compatible
   semantics, likely Anthropic or Gemini.
4. Add provider fallback chains only after per-provider error classification is
   durable and testable.

**Acceptance criteria:**

- Each native provider has streaming fixture tests, tool-call tests, auth error
  diagnostics, and live opt-in coverage.
- Provider-specific fields remain inside `harness-providers`.
- Event metadata stores only redacted IDs, digests, and advisory usage.
- Provider adapters include malformed-stream and tool-call conformance cases so
  parser failures become contextual errors instead of panics.

### 3. Performance and large-session evidence governance

**Classification:** Operational gap.

**Inspiration evidence:**

- Pi Rust documents cited startup, memory, resume, large-session, and extension
  workload measurements with artifact paths and correlation IDs in
  `inspirations/pi_agent_rust/README.md`.
- Pi Rust includes `benches/`, `examples/session_workload_bench.rs`, extension
  workload examples, perf reports, claim-integrity gates, and evidence bundles.
- Pi Rust requires README performance claims to cite artifact paths and run IDs.

**Harness evidence:**

- `docs/testing.md` defines stress lanes and signoff artifacts.
- `README.md` documents `scripts/stress-harness.sh` and per-stage artifacts.
- Harness docs do not define performance budgets for startup, resume, memory,
  TUI redraws, provider streaming, or large-session replay.

**Why it matters:**

Harness is event-sourced, which makes it a good candidate for rigorous perf
evidence. Without budgets, regressions in replay, transcript projection,
compaction, tool artifacts, and session lineage can accumulate invisibly.

**Recommended shape:**

- Add a `perf` or `stress-perf` lane with machine-readable artifacts.
- Track startup/readiness, replay/resume time by event count, session tree/fork
  time, compaction checkpoint time, JSONL append latency, TUI render cost, and
  peak RSS for large fixture sessions.
- Require docs that make numeric claims to cite an artifact and run ID.

**Acceptance criteria:**

- Performance claims fail docs/checks if they lack evidence references.
- Perf artifacts include command, commit, config, platform, run ID, and status.
- Budgets can be relaxed only by editing a reviewed contract file.

### 4. Session index and large-history storage fast path

**Classification:** Architecture gap.

**Inspiration evidence:**

- Pi Rust documents a SQLite session index, WAL, lock file coordination,
  staleness reindexing, and a Session Store V2 sidecar with segments, offsets,
  checksums, checkpoints, rollback, and migration commands in
  `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `docs/architecture.md` uses append-only JSONL as the source of truth and pure
  replay projections.
- `README.md` documents sessions tree, fork, and clone commands.
- Harness has session catalog projections and compaction artifacts, but no
  documented SQLite/index sidecar or segmented storage fast path.

**Why it matters:**

Append-only JSONL is the right semantic source of truth, but large histories need
fast catalog, resume, branch, transcript, and artifact lookup. A sidecar can
improve locality without changing replay semantics if it is always rebuildable
from events.

**Recommended shape:**

- Add a rebuildable session index sidecar for catalog and latest-stable-prefix
  lookups.
- Keep `events.jsonl` authoritative. The sidecar is a cache with validation, not
  a second truth.
- Consider segmented artifact indexes before segmenting event storage.

**Acceptance criteria:**

- Deleting the sidecar only causes reindexing, not data loss.
- Replay equivalence tests compare indexed and full-scan results.
- Corrupt sidecars fail closed and rebuild from JSONL.

### 5. RPC, client/server, and external frontend API

**Classification:** Architecture gap.

**Inspiration evidence:**

- OpenCode describes a client/server architecture that allows remote driving from
  non-TUI clients in `inspirations/opencode/README.md`.
- Pi Rust documents `--mode rpc` with line-delimited JSON commands, streaming
  events, abort, state, compact, and extension UI requests in
  `inspirations/pi_agent_rust/README.md`.
- Codex documents IDE and desktop app experiences in `inspirations/codex/README.md`.

**Harness evidence:**

- `README.md` documents CLI `prompt`, TUI, replay, sessions tree/fork/clone, and
  stress harness flows.
- `docs/config.md` rejects active `server` config because server commands are
  outside the current runtime config.

**Why it matters:**

Harness already has event streams, replay, and a TUI. A protocol API would let
IDEs, desktop shells, test harnesses, and remote controllers consume the same
coordinator events without scraping terminal output.

**Recommended shape:**

- Start with a local stdio RPC mode before an HTTP server.
- Expose stable event notifications and command requests that map to existing
  coordinator commands.
- Keep protocol schema generation and drift tests close to event schema tests.

**Acceptance criteria:**

- RPC clients can submit prompts, resolve permissions, abort turns, request
  compact, list sessions, and follow event streams.
- Replay and RPC event payloads share projection types where possible.
- No RPC command bypasses existing coordinator permission checks.

### 6. Autonomous discipline-agent and category routing system

**Classification:** Parity gap.

**Inspiration evidence:**

- OMO documents 11 specialized agents with tool restrictions in
  `inspirations/oh-my-openagent/docs/reference/features.md`.
- OMO categories map work type to model, variant, prompt append, tools,
  temperature, and reasoning settings in the same file.
- Senpi documents dynamic prompts, background task builtin, prompt presets,
  todo tools, and model controls in `inspirations/senpi/README.md`.

**Harness evidence:**

- `README.md` and `docs/config.md` document shipped `build`, `plan`, `explore`,
  `general`, `title`, `summary`, and `compaction` profiles.
- `docs/config.md` task results include child metadata such as profile,
  category, model ref, toolset, and redelegation capability, but public config
  does not define a first-class category map like OMO.
- `docs/plan-agent-gap-spec.md` focuses on Plan parity, not the broader OMO
  discipline-agent system.

**Why it matters:**

Harness has the foundation for agent profiles and delegation. OMO's higher
leverage is routing by work domain rather than manually choosing an agent/model.
That can improve operator outcomes while keeping agent permissions explicit.

**Recommended shape:**

- Add first-class category config after the provider/model profile contract is
  stable.
- Treat categories as presets that choose a subagent profile, model profile,
  prompt append, max iterations, and tool policy.
- Add read-only consultant profiles only if their permission restrictions are
  enforced at runtime, not just in prompts.

**Acceptance criteria:**

- Category resolution is visible in task events and child metadata.
- Config/schema tests lock default categories and overrides.
- Category-spawned agents cannot recursively redelegate unless explicitly
  allowed by policy.

### 7. Persistent todo and continuation loop

**Classification:** Parity gap.

**Inspiration evidence:**

- OMO documents Ralph Loop, ultrawork loop, and Todo Enforcer in
  `inspirations/oh-my-openagent/README.md` and
  `inspirations/oh-my-openagent/docs/reference/features.md`.
- Senpi includes `todowrite`/`todoread` tools with branch-aware persistence,
  sidebar widget, and continuation loop in `inspirations/senpi/README.md`.

**Harness evidence:**

- `docs/architecture.md` has task lifecycle events, cancellation, stale
  detection, provider loop continuation after tool results, and compaction.
- `crates/harness-tools/AGENTS.md` names todos under control-plane ownership,
  but public docs do not define a persistent todo event contract or continuation
  loop.

**Why it matters:**

Long-running coding tasks fail when the model stops after partial progress. A
durable todo/goal surface would let the coordinator decide whether a turn is
actually complete, expose progress to TUI/replay, and resume interrupted work.

**Recommended shape:**

- Add todo events and projections before adding autonomous loops.
- Bind todos to session branch/run and agent id.
- Add continuation guards: max loops, user interrupt, permission prompt pause,
  stale detection, and explicit done criteria.

**Acceptance criteria:**

- Todos replay deterministically and appear in TUI/replay.
- Continuation loops append normal task/provider/tool events, not special hidden
  state.
- A cancelled loop records why it stopped and never mutates after cancellation.

### 8. Skill-embedded MCP and scoped tool bundles

**Classification:** Architecture gap.

**Inspiration evidence:**

- OMO documents a three-tier MCP system with built-in MCPs, `.mcp.json`, and
  skill-embedded MCPs in `inspirations/oh-my-openagent/AGENTS.md`.
- OMO's features doc describes skills as domain instructions plus MCP tools and
  scoped permissions.
- Senpi points to extension packages for LSP, AST-grep, sandboxing, rules,
  goal tracking, webfetch, and websearch in `inspirations/senpi/README.md`.

**Harness evidence:**

- `docs/config.md` says config-backed MCP servers are first-class and skills are
  separate shared discovery roots.
- `crates/harness-tools/AGENTS.md` documents config-backed MCP server
  registration and generic `mcp.<server>.tool.call` flows.

**Why it matters:**

Harness can already load skills and MCPs. The missing leverage is a single
task-scoped bundle: load this skill, expose these MCP tools, apply these
permission overrides, then tear them down or hide them when the task ends.

**Recommended shape:**

- Extend `SKILL.md` metadata with optional MCP declarations and permission
  requirements.
- Keep MCP lifecycle session-scoped and coordinator-visible.
- Store loaded-skill/MCP decisions in events so replay can explain tool
  availability without reconnecting.

**Acceptance criteria:**

- Skill MCPs are unavailable unless the skill is loaded for that agent/session.
- MCP tool calls still pass through native permission and artifact policy.
- Skill unload/session end cleans up server processes where applicable.

### 9. Dynamic prompt builder and per-model prompt presets

**Classification:** Parity gap.

**Inspiration evidence:**

- Senpi documents an adaptive dynamic prompt builder and per-model prompt presets
  in `inspirations/senpi/README.md`.
- OMO documents file-based prompts, category `prompt_append`, fallback model
  settings, and IntentGate-driven routing in
  `inspirations/oh-my-openagent/docs/reference/features.md`.

**Harness evidence:**

- `docs/architecture.md` says built-in agent prompts can be dynamic, prompt
  assets live in `.agent-harness/agents/*.md`, inline config prompts remain
  compatibility overrides, and `AGENTS.md` is composed separately.
- `docs/config.md` documents prompt and instruction discovery, model profiles,
  and agent metadata.

**Why it matters:**

Harness already separates structured config from prompt bodies. The next gap is a
typed prompt assembly pipeline that can vary by model family, category, active
tools, loaded skills, intent, and workspace state without making config prose
monolithic.

**Recommended shape:**

- Add a prompt assembly trace that records which layers contributed text.
- Keep safety reminders coordinator-injected and non-overridable.
- Add per-model prompt presets only for concrete transport/tool differences.

**Acceptance criteria:**

- Tests assert prompt layer ordering and safety reminder presence.
- Operators can inspect prompt assembly without exposing hidden reasoning.
- Model-specific prompt changes are schema/documented and drift-tested.

### 10. Text tool-call middleware and provider-native tool adapters

**Classification:** Architecture gap.

**Inspiration evidence:**

- Senpi documents XML/Hermes/YAML+XML/Gemma4 text-tool protocols and stream-error
  preservation in `inspirations/senpi/README.md`.
- Senpi ships provider-native tools such as Anthropic web search/code execution,
  OpenAI web search/code interpreter, and Google grounding/code execution.
- Pi Rust supports provider routing, compat config, and provider-specific feature
  flags in `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `crates/harness-providers/AGENTS.md` focuses on OpenAI-compatible Chat and
  Responses transports with normalized provider stream events.
- `docs/architecture.md` says provider tool-call deltas/completions are
  normalized before coordinator execution.

**Why it matters:**

Not all useful models support identical function-calling semantics. A middleware
seam lets Harness support text-tool protocols and provider-native tools without
teaching the coordinator provider-specific parsing.

**Recommended shape:**

- Keep tool-intent parsing inside provider adapters or provider middleware.
- Normalize all results into the existing coordinator tool preflight path.
- Distinguish Harness-native tools from provider-hosted tools in permissions and
  audit metadata.

**Acceptance criteria:**

- Malformed text-tool calls become provider/tool errors, not parser panics.
- Provider-native tool use has clear permission names and artifacts.
- Tool-result ordering still follows assistant source order.
- Text-tool parsers and provider-native adapters have conformance fixtures and
  fuzz or property-style coverage for malformed tags, partial streams, orphan
  tool results, and source-order preservation.

### 11. Stronger shell and extension execution safety

**Classification:** Architecture gap.

**Inspiration evidence:**

- Pi Rust documents capability-gated hostcalls, two-stage `exec` enforcement,
  command mediation, trust lifecycle, kill switches, runtime risk ledger,
  secret-aware env filtering, and sandbox-oriented policies in
  `inspirations/pi_agent_rust/README.md`.
- Senpi points to `pi-sandbox` and parser-aware permission patterns in
  `inspirations/senpi/README.md`.

**Harness evidence:**

- `docs/architecture.md` documents permission kinds, policy resolution,
  allow-always grants, Plan shell guards, and artifacts.
- `docs/config.md` says permission decisions improve UX but are not a sandbox or
  security boundary.

**Why it matters:**

Harness permissions are explicit and replayable, but references go further for
extension/shell execution: they classify dangerous intent, mediate multiline
wrappers, filter secrets, and maintain incident evidence. This becomes essential
if Harness adds extensions or long-lived interactive terminals.

This gap is a dependency for any future executable extension runtime,
provider-native tool that runs code, or persistent PTY/tmux session. It can stay
below the extension seam in this ranking only if the first extension iteration is
manifest-only or otherwise non-executable.

**Recommended shape:**

- Add command-risk classification as advisory/deny policy before OS-level
  sandboxing.
- Keep results redacted and durable as policy events/artifacts.
- Treat true sandboxing as separate from permission UX.

**Acceptance criteria:**

- Dangerous command classes are denied before process spawn under strict policy.
- Denials include stable machine codes and redacted explanations.
- Secret-bearing environment variables are never persisted unredacted.

### 12. Diagnostics, doctor, and self-healing config checks

**Classification:** Operational gap.

**Inspiration evidence:**

- OMO documents `doctor` checks for plugin registration, config, models, and
  environment in `inspirations/oh-my-openagent/README.md` and `AGENTS.md`.
- Pi Rust documents `pi doctor`, auth troubleshooting, session diagnostics,
  extension compatibility checks, and optional safe fixes in
  `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `README.md` and `docs/config.md` document config validation.
- `docs/testing.md` documents lanes but not a user-facing holistic diagnostic.

**Why it matters:**

As providers, MCPs, skills, Plan, sessions, and TUI surfaces grow, users need a
single command that explains why a setup will or will not work.

**Recommended shape:**

- Add `harness doctor` with text and JSON output.
- Check config discovery, schema, provider credentials, model refs, MCP servers,
  writable state/artifact dirs, sessions index health, LSP availability, and TUI
  prerequisites.
- Keep `--fix` limited to safe directory creation or cache rebuilds.

**Acceptance criteria:**

- Machine-readable diagnostic codes are stable.
- Missing optional integrations are warnings, not failures.
- No diagnostic command leaks secrets.

### 13. Install, release, and migration packaging

**Classification:** Operational gap.

**Inspiration evidence:**

- OpenCode documents curl, npm, Scoop, Chocolatey, Homebrew, Arch, mise, Nix, and
  desktop app installs in `inspirations/opencode/README.md`.
- Codex documents npm, Homebrew, GitHub release binaries, IDE, desktop, and
  ChatGPT-plan sign-in in `inspirations/codex/README.md`.
- Pi Rust documents an idempotent curl installer, checksum/signature options,
  completions, migration from TypeScript Pi, uninstall, and release packaging in
  `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `README.md` uses `cargo run` and shipped example configs.
- No public Harness doc describes release binaries, installer behavior,
  completions, uninstall, or migration between installed versions.

**Why it matters:**

Harness can be excellent technically and still hard to adopt. Packaging also
forces clarity around config locations, migration, generated schemas, and runtime
assets.

**Recommended shape:**

- Start with documented release artifacts and shell completions.
- Add installer only after config/state migration rules are stable.
- Keep installer tests deterministic and non-destructive.

**Acceptance criteria:**

- `harness --version`, `harness --help`, config validation, and smoke prompt work
  from installed artifacts.
- Installer records state sufficient for uninstall or upgrade.
- Release docs identify supported platforms and checksums.

### 14. Session export, share, and dataset workflow

**Classification:** Operational gap.

**Inspiration evidence:**

- Pi-mono encourages sharing OSS coding-agent sessions as datasets in
  `inspirations/pi-mono/README.md`.
- Pi Rust supports session export to HTML and evidence bundles in
  `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `README.md` documents session lineage commands.
- `docs/architecture.md` supports replay, transcript projection, artifacts, and
  redaction.

**Why it matters:**

Harness has unusually strong provenance. Export/share workflows would turn that
into review artifacts, bug reports, demos, and eventually training/evaluation
datasets without handing users raw JSONL and artifact directories.

**Recommended shape:**

- Add redacted Markdown/HTML export from replay projections.
- Include event summary, prompt/model/tool timeline, diffs, artifacts index,
  compaction checkpoints, and permission decisions.
- Add explicit share policy: local export first, remote publishing only with
  opt-in.

**Acceptance criteria:**

- Export is replay-only and side-effect free.
- Secrets scanner runs or redaction status is included.
- Exported sessions identify source run, cutoff, and artifact provenance.

### 15. Interactive terminal/tmux lane

**Classification:** UX gap.

**Inspiration evidence:**

- OMO documents tmux panes for background agents and full interactive terminal
  sessions in `inspirations/oh-my-openagent/docs/reference/features.md`.
- Pi-mono documents tmux-driven TUI testing in `inspirations/pi-mono/AGENTS.md`.

**Harness evidence:**

- `docs/architecture.md` exposes a `bash` tool and task/background workflows.
- `docs/testing.md` has deterministic PTY signoff, but that is a test lane, not
  a user-facing persistent interactive terminal tool.

**Why it matters:**

Some workflows require ongoing terminal state: REPLs, debuggers, local servers,
editors, and watching background agents. A persistent terminal lane could reduce
misuse of one-shot bash.

**Recommended shape:**

- Treat tmux/PTY sessions as tools with explicit lifecycle events and permission
  policy.
- Keep output capped with artifact capture.
- Start with user-invoked sessions, not automatic background panes.

**Acceptance criteria:**

- Sessions can spawn, send keys, capture output, resize, and terminate.
- Replay shows transcript/artifact summaries without re-running terminal apps.
- Orphan process cleanup is tested.

### 16. Autocomplete and inline context attachment

**Classification:** UX gap.

**Inspiration evidence:**

- Pi Rust documents `@file` references, slash command completions, skills,
  prompt templates, fuzzy scoring, and background project-file refresh in
  `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `README.md` documents slash commands such as `/model`, `/status`, `/resume`,
  `/new`, `/tree`, `/fork`, and `/clone`.
- `crates/harness-tui/AGENTS.md` documents keybindings and a compose-first shell,
  but public docs do not describe file-reference autocomplete or context
  attachment.

**Why it matters:**

The faster users can attach relevant files and discover commands, the less they
need to spend turns asking the model to search. This is high daily UX value and
can be tested deterministically.

**Recommended shape:**

- Add composer completions for slash commands, sessions, agents/models, skills,
  and `@` file references.
- Respect `.gitignore` and workspace safety.
- Treat attached files as explicit read/context events for replay.

**Acceptance criteria:**

- Completion order is deterministic and snapshot-tested.
- Attached file context is visible in transcript/replay.
- Large/binary files follow existing read/image limits.

### 17. Dedicated command and diff review surfaces

**Classification:** UX gap.

**Inspiration evidence:**

- OpenCode UI images include `commands-window.png`, `commands-window2.png`,
  `session-diff.png`, `session-select.png`, `session.png`, and `start-screen.png`.
- The screenshot parity folder includes OpenCode command menu, slash command
  menu, session chat examples, and comparable Harness screenshots.

**Harness evidence:**

- `crates/harness-tui/AGENTS.md` documents compose-first home, transcript-first
  sessions, operator sidebar, overlays, snapshots, and diff visualization.
- TUI snapshots cover slash command popups, status dialog, and inline diffs.

**Why it matters:**

Harness has the primitives, but a dedicated review workflow can make file
changes and command discovery safer than inline transcript-only rendering.

**Recommended shape:**

- Add a full-screen or side-panel diff review mode sourced from `EditApplied`
  artifacts.
- Add command palette sections with descriptions, keybindings, and availability
  status.
- Keep inline transcript diffs as the quick path.

**Acceptance criteria:**

- Diff review is replay-safe and reads only persisted artifacts.
- Command palette availability reflects current mode and permissions.
- PTY/native snapshots capture representative layouts.

### 18. Visual/perf TUI budgets

**Classification:** Operational/UX gap.

**Inspiration evidence:**

- Senpi documents differential rendering fast paths and flicker-budget regression
  tests in `inspirations/senpi/README.md` and `AGENTS.md`.
- Pi Rust documents TUI render caches, frame timing telemetry, and memory pressure
  behavior in `inspirations/pi_agent_rust/README.md`.

**Harness evidence:**

- `docs/testing.md` has deterministic PTY and native visual signoff.
- `crates/harness-tui/AGENTS.md` notes animation deadlines and redraw cadence
  constraints.

**Why it matters:**

Snapshot correctness does not catch flicker, excessive redraws, frame latency, or
memory pressure. TUI performance budgets make visual quality regressions visible.

**Recommended shape:**

- Add render counters and optional frame timing telemetry under deterministic
  tests.
- Define no-full-clear-after-init or redraw-count budgets if compatible with the
  Ratatui backend.
- Report active context estimate and TUI memory caps distinctly.

**Acceptance criteria:**

- UI perf tests fail on avoidable full clears or excessive redraws for stable
  scenarios.
- Budgets are documented and adjustable through reviewed fixtures.
- Native visual signoff remains provenance evidence, not a portable hash oracle.

### 19. Multi-agent team coordination and file reservations

**Classification:** Architecture gap.

**Inspiration evidence:**

- OMO Team Mode includes shared mailbox, shared task list, file-locked claims,
  optional worktrees, and tmux layout in
  `inspirations/oh-my-openagent/docs/reference/features.md`.
- Pi Rust `AGENTS.md` describes MCP Agent Mail with identities, inbox/outbox,
  threads, and advisory file reservations.

**Harness evidence:**

- `README.md` and `docs/config.md` document `task`, `background_output`, child
  runtime metadata, cancellation, and follow-up actions.
- `docs/architecture.md` prevents worker redelegation bypasses and keeps
  scheduling coordinator-owned.

**Why it matters:**

Harness can already run child tasks. The missing layer is coordination between
concurrent workers so they do not edit the same files, duplicate work, or lose
status when context is compacted.

**Recommended shape:**

- Add advisory file reservations as coordinator-owned events before adding full
  Team Mode.
- Add child-to-parent status messages and thread IDs without allowing arbitrary
  worker-to-worker side effects.
- Worktrees should be optional and explicit because they complicate artifact and
  path provenance.

**Acceptance criteria:**

- Reservations are visible in replay and expire deterministically.
- Edits can warn or deny on active conflicting reservations.
- Child status messages are durable but do not bypass parent permissions.

### 20. Gap ledger and parity evidence database

**Classification:** Operational gap.

**Inspiration evidence:**

- Pi Rust has machine-readable parity, certification, evidence, provider, and
  extension ledgers under `inspirations/pi_agent_rust/docs/`.
- Pi Rust documents reconciliation checks that fail when high-severity gaps lack
  active owners in `inspirations/pi_agent_rust/AGENTS.md`.

**Harness evidence:**

- Harness has docs drift checks, event docs reference tests, and this ranked doc,
  but no machine-readable gap ledger with status, owner, severity, evidence, and
  test lane.

**Why it matters:**

As this list becomes implementation work, a ledger prevents completion illusion:
high-value gaps should either have owners, be intentionally rejected, or be
re-ranked with evidence.

**Recommended shape:**

- Add a future inspiration-gap ledger JSON file under `docs/` only if the team
  wants active tracking.
- Keep this Markdown file for human context and the JSON ledger for status.
- Add a drift check that every critical/high ledger item appears in a tracking
  issue or project-local task system.

**Acceptance criteria:**

- Each ledger item has id, status, rank, severity, owner/ref, evidence paths,
  acceptance criteria, and test lane.
- Closing a gap requires linked verification evidence.
- Rejected gaps record the invariant or product reason.

## Intentional divergences and already-covered areas

Do not reopen these as generic gaps without new evidence:

- **Hashline editing basics are already present.** Harness docs and tool-crate
  guidance state that `read` emits hashline anchors and `edit` consumes anchored
  views. Future work should improve UX or cross-tool coverage, not re-add the
  concept.
- **Plan mode is already stable and stricter than OpenCode.** The remaining Plan
  work belongs in `docs/plan-agent-gap-spec.md`. Do not weaken Plan to allow
  broad edits or write-capable subagents by default.
- **Background task basics exist.** Harness has `task`, `background_output`,
  child metadata, cancellation, and coordinator scheduling. The gap is richer
  orchestration, categories, reservations, and continuation, not basic child
  spawning.
- **Config-backed MCP exists.** The gap is skill-embedded and scoped MCP
  lifecycle, not generic MCP calls.
- **LSP exists.** Harness already has diagnostics, symbols, references, and
  rename surfaces. Gaps should focus on UX, skill/category exposure, and optional
  provider/tool integration.
- **Replay purity is a strength.** Extension, RPC, terminal, and team-mode work
  must preserve the rule that replay never executes tools, network calls,
  providers, hooks, or extension code.
- **Side-effectful OpenCode product areas are intentionally rejected today.**
  `server`, `command`, `plugin`, sharing, autoupdate, and enterprise fields
  should not silently become active compatibility behavior. Add Harness-native
  surfaces with explicit migration and tests instead.

## Recommended implementation sequence

1. **Define extension and provider seams first:** extension manifest prototype,
   provider onboarding/test obligations, and credential diagnostics.
2. **Add evidence governance:** performance/large-session budgets plus optional
   gap ledger before marketing any perf or parity claims.
3. **Deepen session infrastructure:** rebuildable session index and storage stress
   tests.
4. **Expose protocol/API surfaces:** stdio RPC, then optional local server.
5. **Improve autonomous workflows:** categories, specialist read-only profiles,
   persistent todos, and controlled continuation loops.
6. **Scope tools by task:** skill-embedded MCPs, provider-native tools, and
   text-tool middleware.
7. **Polish operator experience:** autocomplete, diff review, command windows,
   tmux/PTY interactive sessions, and TUI performance budgets.
