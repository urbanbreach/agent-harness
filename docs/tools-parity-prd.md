# Harness Tools Parity PRD

**Status:** Implementation evidence ledger. The parity work described here has
been implemented where rows in the progress ledger cite passing tests and dogfood
artifacts; unchecked or deferred items remain explicitly marked in this document
and [`tools-parity-progress.md`](./tools-parity-progress.md).

**Date:** 2026-06-28

**Audience:** Autonomous implementation agents and reviewers bringing the
`agent-harness` native tool surface up to production-grade behavior.

**Authority:** Subordinate to the root [`AGENTS.md`](../AGENTS.md),
[`crates/harness-core/AGENTS.md`](../crates/harness-core/AGENTS.md),
[`crates/harness-tools/AGENTS.md`](../crates/harness-tools/AGENTS.md),
[`crates/harness-providers/AGENTS.md`](../crates/harness-providers/AGENTS.md),
and the public native tool contract in
[`native-tool-catalog.md`](native-tool-catalog.md). If this PRD conflicts with
runtime invariants, the invariants win.

**Primary source anchors used while drafting:**

- Harness native catalog: `docs/native-tool-catalog.md`
- Harness provider tool export: `crates/harness-core/src/agent/provider_boundary.rs`
- Harness tool contract and provider names: `crates/harness-core/src/tool.rs`
- Harness native schemas/results: `crates/harness-tools/src/lib.rs`,
  `crates/harness-tools/src/native_tools/args.rs`,
  `crates/harness-tools/src/agent_ops/background.rs`
- Harness core tool contract:
  `inspirations/packages/core/src/tool/tool.ts`
- Harness registry/model filtering/custom tools:
  `inspirations/shuvcode/packages/src/tool/registry.ts`
- Harness provider schema transforms:
  `inspirations/packages/src/provider/transform.ts`
- Harness MCP conversion:
  `inspirations/packages/src/mcp/catalog.ts`
- Harness truncation/display reference:
  `inspirations/shuvcode/packages/src/tool/truncation.ts`,
  `inspirations/shuvcode/packages/src/cli/cmd/run.ts`

These anchors are **not enough for implementation**. Every implementation phase
must re-read the current Harness source it depends on and record the exact
commit/path/line evidence used for that phase.

---

## 0. Read this first

### 0.1 Governing objective

Bring Agent Harness tools **on par with Harness for model-facing quality,
native behavior, provider compatibility, operator trust, and dogfooded
reliability**, while preserving Harness-native architecture:

- Events remain the source of truth.
- The coordinator remains the only scheduling, permission, event append,
  compaction, hook, and lifecycle authority.
- Permission checks still precede tool execution.
- Hashline `edit` remains the normal Harness file-changing route unless a
  later explicit decision approves compatibility aliases.
- `session_*` tools remain replay-derived and side-effect free.
- Provider metadata, events, artifacts, support bundles, cassettes, and docs
  never persist raw requests, raw responses, auth headers, cookies, keys, PEM
  blocks, or hidden prompt/config instruction secrets. Live event logs may
  include provider reasoning delta events as local session evidence; they are not
  public support material.

Harness is the reference for tool ergonomics and compatibility, not an
instruction to copy TypeScript internals into Rust. The implementation must
adapt observable behavior into Harness's event-sourced, permissioned runtime.

### 0.2 Definition of “on par”

For this PRD, “on par with Harness in tools” means:

1. **Model-facing parity:** tool names, descriptions, argument schemas, required
   fields, examples, validation failures, and next-action guidance are as clear
   and model-usable as Harness's equivalent tool definitions.
2. **Provider payload parity:** serialized request shapes for active Harness
   toolsets are provider-safe, tested against real registry tool definitions,
   and normalized or explicitly rejected for provider families where Harness
   has known transforms.
3. **Native behavior parity:** overlapping tools behave equivalently for normal,
   edge, failure, permission-denied, large-output, and cancellation scenarios,
   unless a documented Harness invariant requires a different behavior.
4. **Result/display parity:** tool results, truncation notices, artifacts,
   structured output, permission copy, transcript rows, and TUI/CLI summaries are
   predictable and comparable to Harness's operator-facing surface.
5. **Dogfooded parity:** every implemented workstream is exercised through the
   real Harness surface, not just by source inspection or isolated unit tests.
6. **Documented divergence:** Harness-only ecosystem features are either
   implemented, explicitly deferred with evidence, or intentionally rejected
   with a Harness-specific rationale.

### 0.3 Non-goals

- Do not edit files under `inspirations/`; they are read-only reference source.
- Do not copy Harness source code, package layout, branding, or UI copy without
  adapting it to Harness terms and architecture.
- Do not replace Harness canonical tool IDs with provider-function names in
  public docs or config.
- Do not bypass coordinator permission checks to match Harness behavior.
- Do not broaden descriptor-only extension seams into a runtime plugin host
  without a separate approved design.
- Do not add dynamic JS/TS plugins or Harness HTTP API compatibility as
  incidental “parity cleanup.” `write` and `apply_patch` are explicit §8
  promotion decisions, not incidental aliases.
- Do not claim live, PTY, native visual, or provider evidence without artifact
  provenance.
- Do not weaken, delete, ignore, or rubber-stamp tests to make parity pass.

### 0.4 Implementation-agent operating rules

Every implementation agent working this PRD must follow these rules before each
phase or task card:

1. **Read the local rules first.** Read the root `AGENTS.md` plus the
   crate-scoped `AGENTS.md` for every crate touched. Load `karpathy-guidelines`
   and `programming` before any Rust, TypeScript, schema, generated-code, build,
   or test edit.
2. **Re-read Harness source before the edit.** The agent must inspect the
   relevant Harness files for the exact behavior being implemented. Use the
   local `inspirations/harness` or `inspirations/shuvcode` reference when
   present. If the local reference is missing or stale, inspect upstream
   Harness and record the commit SHA. Do not implement from this PRD, memory,
   prior chat summaries, or old reports alone.
3. **Write a phase evidence note.** Before the first code edit in a phase, add or
   update an evidence note in the implementation branch's parity ledger
   (`docs/tools-parity-progress.md` once created) with:
   - Harness source files and commit/reference used.
   - Harness source files touched or expected to be touched.
   - Behavior summary in the agent's own words.
   - Intended Harness adaptation.
   - Known conflicts with Harness invariants.
   - Tests and dogfood scenarios planned for the phase.
4. **Think for yourself.** The PRD is not a script. If current Harness source
   contradicts this document, stop and update the evidence note before changing
   code. If a task appears unsafe, obsolete, or over-scoped, record the finding
   and choose the smaller invariant-preserving adaptation.
5. **TDD for behavior.** Write a failing test or snapshot/inventory assertion
   before behavior changes. For pure documentation updates, make the evidence
   status explicit instead of inventing a passing test.
6. **One phase at a time.** Do not start a later phase while the current phase's
   acceptance and dogfood gates are failing, except for explicitly independent
   inventory work.
7. **Dogfood through the surface.** Unit tests are not enough for tool work. Each
   phase must run at least one real Harness tool-use scenario through CLI, TUI,
   provider/mock prompt path, or testkit surface as specified in §7.
8. **No silent scope expansion.** Deferred items in §8 require an explicit
   decision record before implementation.

### 0.5 Definition of done for this PRD

This PRD is complete when all of the following are true:

1. P0 and P1 task cards in §10 are implemented, tested, dogfooded, and recorded
   in the parity progress ledger, or explicitly re-scoped by a dated decision.
2. Deferred items in §8 are each dispositioned as `implemented`, `deferred`, or
   `rejected`, with evidence and owner rationale.
3. Provider payload snapshots cover the real active registry, not only synthetic
   schemas.
4. Native behavior tests and dogfood runs cover happy paths, bad inputs,
   permission denial, large output/artifact spill, cancellation, unsupported
   dependencies, nested/batch execution, and replay-derived session inspection.
5. The relevant commands pass, at minimum:
   - `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test`
   - `cargo nextest run -p harness-tools`
   - `cargo nextest run -p harness-providers`
   - `cargo nextest run -p harness-core --test coord_test`
   - `cargo nextest run -p harness --test config_docs_reference_test`
   - `cargo nextest run -p harness --test event_docs_reference_test`
   - `scripts/test-lanes.sh fast`
   - `scripts/test-lanes.sh quality-gates`
   - `scripts/test-lanes.sh all-deterministic`
6. Live-provider, native/PTY, or TUI claims have matching artifacts, run IDs, and
   redaction/secret-scan notes.

---

## 1. Source-of-truth inventory model

The first implementation phase must generate and maintain an inventory matrix.
The matrix may be a test fixture, generated Markdown table, JSON artifact, or
combination of those, but it must be reproducible and drift-tested.

### 1.1 Harness inventory columns

For every Harness reference tool considered, record:

- Tool ID.
- Source path and commit/reference.
- Registry inclusion rule.
- Provider/model/feature-flag gating.
- Description source.
- Input schema source.
- Output schema or model-output hook, if any.
- Permission behavior.
- Execution semantics.
- Large-output/truncation behavior.
- Error/invalid-argument behavior.
- TUI/CLI display behavior.
- Native dependencies.
- Whether custom/plugin/MCP variants affect the tool.

### 1.2 Harness inventory columns

For every Harness native tool and relevant dynamic tool wrapper, record:

- Canonical tool ID.
- Provider function name after sanitization.
- Active profile/toolset inclusion.
- Permission/capability kind.
- Description source, including profile-specific overrides.
- Input schema source and field descriptions.
- Required fields and compatibility aliases.
- Result shape: display text, structured JSON, artifacts.
- Replay/event behavior.
- Permission prompt/display behavior.
- Large-output/artifact behavior.
- Native execution tests.
- Dogfood scenario coverage.
- Harness mapping status.

### 1.3 Status values

Every row must have one status:

- `parity_ready` — behavior and presentation are implemented, tested, and
  dogfooded.
- `harness_adapted` — intentionally different because Harness invariants or
  product scope are stronger; documented and tested.
- `needs_work` — in scope for P0/P1 work.
- `deferred_decision` — blocked on §8 decision.
- `harness_only` — no Harness equivalent; must still have Harness-native UX and
  dogfood coverage.
- `harness_only` — Harness feature not currently implemented by Harness.
- `excluded` — explicitly out of scope, with evidence.

The inventory must fail CI when a tool silently changes status, gains/loses a
provider function name, changes required arguments, or loses field descriptions.

---

## 2. Current baseline facts to verify

These facts are source-observed starting points, not completion claims. The
implementation loop must verify them again before editing.

### 2.1 Harness baseline

- Harness exposes a built-in native tool surface through `harness-tools` and
  mirrors it in `docs/native-tool-catalog.md`.
- `build_provider_tool_defs()` builds provider `ToolDef` values from an active
  `AgentProfile` toolset, validates top-level schema shape, preserves profile
  description overrides, and emits sanitized function names.
- Harness `ToolDef` carries `tool_id`, provider `function_name`, optional
  description, and JSON `parameters`; the OpenAI request path forwards those
  parameters to provider requests.
- Many native schemas are generated through `schemars::schema_for!(T)` and then
  normalized as top-level object schemas. Some tools have rich/manual schemas;
  some derived schemas need inventory before edits.
- `background_output` currently returns rich structured metadata including
  `request_id`, compatibility `task_id`/`session_id`, terminal state, late-result
  state, cancellation fields, child runtime, route, and `next_actions`.

### 2.2 Harness baseline

- Harness core tools define descriptions, input schemas, output schemas, and
  optional `toModelOutput` behavior through its core tool contract.
- Harness registry behavior includes built-ins, local custom tool files,
  plugin-contributed tools, model/provider filtering, and plugin definition
  hooks in the referenced source.
- Harness provider transforms include provider/model-specific schema handling
  for OpenAI/Azure-like, Moonshot/Kimi-like, and Gemini/Google-like cases.
- Harness MCP conversion normalizes tool names and input schemas for dynamic
  MCP tools.
- Harness truncation has a uniform output cap and writes large output to a
  tool-output path with model-facing guidance.

---

## 3. Parity scope

### 3.1 In scope

- Model-visible tool definitions for all Harness native tools.
- Provider function-name mapping and canonical-ID clarity.
- Real-registry provider payload serialization.
- Provider-family schema compatibility for active Harness providers.
- Native execution behavior for overlapping tools.
- Harness-native UX for Harness-only control-plane tools.
- Permission prompts, result rows, artifact references, and replay/session safety.
- Task/background/batch/skill ergonomics.
- MCP tool schema handling where MCP tools are exposed to model-visible toolsets.
- Dogfooding through CLI, prompt/run, TUI/PTY where applicable, and deterministic
  testkit lanes.

### 3.2 Out of scope unless promoted by §8 decision

- Full Harness local JS/TS plugin host parity.
- Harness `/experimental/tool*` HTTP API parity.
- Model-gated edit/write/apply-patch swapping.
- `ToolChoice::Required` across all providers.
- Broad unknown-tool repair that weakens fail-closed safety.
- Media/binary tool-result attachments beyond existing artifact support.

---

## 4. Workstream requirements

### 4.1 P0: Inventory and evidence infrastructure

Create the reproducible parity inventory from §1.

Acceptance:

- Inventory covers every row in `docs/native-tool-catalog.md`.
- Inventory covers Harness built-ins from the current referenced registry.
- Inventory distinguishes canonical IDs from provider function names.
- Inventory records profile description overrides and active toolsets.
- Inventory records Harness model/provider gating and Harness adaptation status.
- Tests fail on unreviewed tool ID, permission, schema, or status drift.

Dogfood:

- Run `harness doctor` or equivalent catalog readiness path and compare the
  emitted native tool catalog summary with the generated inventory.
- Ask a Harness model/tool surface to list available tools in a fixture run and
  confirm the model-visible names match the provider payload snapshot.

### 4.2 P0: Real provider payload snapshots

Replace synthetic-only confidence with snapshots generated from the real active
tool registry and representative profiles.

Acceptance:

- Provider payload tests cover default build profile, plan/read-only profile,
  category subagent profile, and at least one MCP schema fixture.
- Tests include dotted IDs such as `lsp.rename`, `github.issue`, and `shell.run`.
- Tests assert sanitized provider function names and canonical human IDs do not
  contradict each other.
- Tests cover OpenAI Chat and Responses serialization paths where both are
  supported.
- Provider request digests remain redaction-safe and stable for meaningful shape
  changes.

Dogfood:

- Run a tool-capable mocked prompt using a generated fixture workspace and save
  the event log.
- Inspect the event log with `session_read` or `harness sessions inspect` and
  verify tool identities remain clear and redacted.

### 4.3 P0: Provider schema compatibility

Compare Harness provider payload behavior against Harness provider transforms.
Do not blindly port transforms into `harness-core`; provider-family logic belongs
at the provider/adapter boundary.

Acceptance:

- Tests cover strict OpenAI-compatible, Gemini/Google-like, and Moonshot/Kimi-like
  schema constraints using real Harness tool schemas.
- Unsupported provider/schema combinations fail with actionable diagnostics
  rather than malformed provider requests.
- MCP schemas are normalized, rejected, or documented consistently before model
  exposure.
- The implementation does not move provider-specific transport policy into the
  coordinator.

Dogfood:

- Run at least one live or recorded request against each supported provider
  family being claimed, or mark the provider-family case unchecked and blocked.

### 4.4 P1: Model-facing tool descriptions and schemas

Bring Harness tool descriptions and argument schemas up to production-grade model
ergonomics without weakening strict typed boundaries.

Acceptance:

- High-frequency tools have clear usage guidance, required fields, bad-input
  recovery hints, and examples where useful.
- Field-level schema descriptions exist for sparse derived schemas discovered by
  the inventory.
- `task` examples show `run_in_background` and `load_skills: []` explicitly.
- `background_output` and `background_cancel` emphasize canonical `request_id`
  while preserving compatibility aliases.
- `edit` continues to teach hashline workflow clearly; do not degrade the current
  hashline safety contract to imitate Harness whole-file write behavior.
- Snapshot/inventory tests prove description/schema improvements and prevent
  silent regression.

Dogfood:

- Run model prompts that require each improved tool to be selected without
  naming it directly, then inspect whether the model chooses valid arguments.
- Include bad-argument prompts and verify the model receives actionable recovery
  text.

### 4.5 P1: Native behavior parity for overlapping tools

For `read`, `glob`, `grep`, `bash`, `edit`, `webfetch`, `websearch`, `task`,
`todowrite`, `skill`, `lsp`, and `batch`, compare Harness observable behavior
with Harness behavior.

Acceptance:

- Each overlapping tool has happy-path, invalid-argument, permission-denied,
  large-output, and missing-dependency tests where applicable.
- Path safety behavior is at least as strict as Harness currently requires.
- Bash remains execution-only; blocked file search/read/edit shortcuts continue
  to point to native tools.
- Web/network tools preserve permission and redaction guarantees.
- LSP tools stay recoverable when no server is configured or a language is
  unsupported.

Dogfood:

- Run the same scenario family through Harness and Harness reference when the
  reference runtime is available. If Harness cannot be run, record why and use
  current source comparison instead.

### 4.6 P1: Task, background, batch, and skill ergonomics

These tools are a Harness strength and must become easy for models and operators
to use correctly.

Acceptance:

- `task` clearly distinguishes synchronous delegation, background delegation,
  continuation sessions, categories, direct subagents, and skill loading.
- Background result retrieval uses `request_id` as the canonical selector;
  compatibility aliases are documented as aliases, not equal primary choices.
- `background_cancel` is presented as the canonical cancellation tool.
- `batch` output enumerates child tool IDs, order, statuses, and permission
  attribution.
- `skill` output exposes compact source/scope/status metadata without echoing
  full skill bodies into event logs.
- Worker redelegation bypasses remain blocked.

Dogfood:

- Run a background child task, retrieve output before completion, retrieve output
  after completion, cancel a non-terminal child, and observe a late-result path.
- Run a batch with mixed read, denied edit, and failed child call; verify order,
  permissions, and output clarity.
- Load a real skill, a missing skill, and a denied/malformed skill.

### 4.7 P1: Result, truncation, artifact, and model-output presentation

Harness has a uniform truncation surface. Harness must have an equally reliable
and redaction-safe result presentation, adapted to `ToolResult` and artifacts.

Acceptance:

- Large output policies are consistent across tools or explicitly documented per
  tool.
- Model-facing text always points to artifact references when full output is
  spilled.
- Structured JSON remains available for programmatic inspection when a tool
  claims it.
- Artifact indexes and event summaries are redacted and bounded.
- Result conversion does not drop actionable failure context.

Dogfood:

- Force large outputs from `bash`, `grep`, `read`, `session_read`, and at least
  one child task; verify summary, artifact, and next-action copy.

### 4.8 P1: Invalid tool and invalid argument handling

Harness may remain stricter than Harness, but failures must help the model
recover.

Acceptance:

- Invalid JSON, unknown fields, missing required fields, wrong selector types,
  and unsupported tool names produce deterministic model-facing recovery text.
- Any unknown-tool repair is narrowly scoped, tested, and does not hide genuine
  provider/model bugs.
- The `invalid` tool remains a control-plane report surface, not a broad bypass
  for executing arbitrary malformed calls.

Dogfood:

- Prompt the model into common mistakes: omit `load_skills`, omit
  `run_in_background`, pass `session_id` where `request_id` is expected, use an
  unsupported alias, pass an out-of-workspace path, and use an unknown tool name.
  Verify recovery behavior is actionable.

### 4.9 P1: Permission and operator-trust presentation

Tool parity is not only model payload shape. Users must understand what a tool
will do before approving it.

Acceptance:

- Permission prompts identify the effective child tool for nested `batch` calls.
- Mutating tools show file/workspace/network implications in operator-facing
  copy.
- Replay-derived tools state that they do not execute providers, tools, hooks,
  MCP, network, or CLI.
- Permission tests cover deny, allow, ask, headless ask-deny, and worker policy
  violation paths.

Dogfood:

- Run CLI/TUI scenarios that trigger permission prompts for bash, edit, task,
  webfetch/websearch, codesearch, lsp.rename, and a nested batch child.

### 4.10 P2: TUI/CLI transcript tool display

After model-facing and native semantics are stable, bring operator display up to
production-grade presentation.

Acceptance:

- Tool rows have intentional titles, status icons/labels, concise descriptions,
  output previews, and artifact links.
- Harness-only tools do not fall back to generic low-information rows when they
  are common in normal agent operation.
- Deterministic render tests cover happy, running, failed, denied, truncated,
  cancelled, and late-result states.

Dogfood:

- Capture PTY/TUI evidence for a session containing read/search/edit/task/
  background/batch/permission/failure rows. Do not claim visual parity without
  artifact provenance.

---

## 5. Tool family mapping requirements

### 5.1 Overlapping tools

The inventory must map and verify overlapping behavior for:

| Family | Harness IDs | Required comparison |
|---|---|---|
| File read/list | `read`, `list` | Harness read/directory behavior, hashline anchors, caps, artifact spill. |
| File search | `glob`, `grep` | Pattern semantics, caps, permission/path safety, output shape. |
| Shell | `bash`, `shell.run` | Shell selection, cwd handling, timeout, blocked-command guidance, truncation. |
| Edit | `edit`, `ast_grep_replace`, `lsp.rename` | Hashline workflow, diff artifacts, structural rewrite, rename permissions. |
| Web/search | `webfetch`, `websearch`, `codesearch` | Provider/backing service, permission, output caps, network failure recovery. |
| Delegation | `task`, `background_output`, `background_cancel` | Synchronous/background behavior, continuation IDs, cancellation, next actions. |
| Todo/skill | `todowrite`, `todoread`, `skill` | Schema, source/scope metadata, validation, model guidance. |
| LSP/MCP | `lsp`, MCP wrappers | Unsupported server recovery, schema exposure, provider-safe names. |
| Batch/control | `batch`, `question`, `plan_enter`, `plan_exit`, `invalid` | Nested intent, permission attribution, user-question lifecycle. |

### 5.2 Harness-only tools

Harness-only tools are not second-class. `session_*`, `ast_grep_*`,
`github.*`, `background_*`, `plan_*`, `shell.run`, and `lsp.rename` must have
Harness-native model descriptions, schemas, permission copy, output formatting,
and dogfood coverage even when Harness has no direct equivalent.

### 5.3 Harness-only tools

Harness-only tools or features must be routed through §8 decisions before
implementation. Do not silently add aliases or compatibility surfaces.

---

## 6. Edge-case matrix

Every in-scope tool family must cover the edge cases below where applicable:

| Edge | Examples |
|---|---|
| Missing required fields | `task` without `load_skills`, `background_output` without usable selector. |
| Unknown fields | Extra JSON fields rejected by `deny_unknown_fields`. |
| Compatibility aliases | `filePath`/`path`, `session_id`/`task_id`, `agent`/`subagent_type`. |
| Path safety | Traversal, symlinks, external paths, deleted files, stale hashline anchors. |
| Permissions | Deny, ask, allow, headless ask-deny, worker redelegation policy. |
| Large output | Line cap, byte cap, artifact spill, summary copy, artifact lookup. |
| Cancellation | Pending background task, terminal task, late result after cancellation. |
| Unsupported dependency | Missing LSP, missing ast-grep binary, missing network provider, unavailable MCP. |
| Provider schema | Dotted IDs, nested object schemas, arrays without items, type arrays, `$ref` siblings. |
| Replay safety | `session_*` must not execute providers/tools/hooks/MCP/network/CLI. |
| Redaction | Secrets in command output, web output, provider metadata, artifacts, support export. |
| TUI/PTY | Running, completed, failed, denied, truncated, cancelled, and late-result rows. |

---

## 7. Dogfooding requirements

Tool parity work must be dogfooded because tools are the agent's own execution
surface. A feature is not done until an agent has used it through the surface a
real model/operator will use.

### 7.1 Required dogfood ledger fields

Each dogfood evidence row must record:

- Date and commit.
- Config path and provider/model or mock mode.
- Exact command or TUI/PTY scenario.
- Session/run ID and artifact paths.
- Tools exercised.
- Expected behavior.
- Observed behavior.
- Failure or discrepancy found.
- Fix follow-up or explicit deferral.
- Secret/redaction check result.

### 7.2 Minimum scenario set

The implementation loop must maintain a reusable dogfood suite that covers:

1. **Discovery:** model chooses `glob`/`grep`/`read` correctly from a vague code
   inspection prompt.
2. **Editing:** model reads with hashline anchors, applies `edit`, handles stale
   anchors, and inspects diff artifacts.
3. **Shell:** model runs an allowed command, attempts a blocked file-search
   shortcut, then recovers with native tools.
4. **Delegation:** model runs synchronous `task`, background `task`,
   `background_output`, `background_cancel`, and continuation by session ID.
5. **Batch:** model runs a mixed batch with read-only, denied, and failed child
   calls.
6. **Skill:** model loads an allowed skill, missing skill, and denied/malformed
   skill.
7. **Network:** model uses `webfetch`, `websearch`, and `codesearch` with
   permission gates and unavailable-network fallback.
8. **LSP:** model requests diagnostics/references/rename with installed and
   missing language-server cases.
9. **Session tools:** model lists, reads, searches, and inspects sessions without
   causing side effects.
10. **Large output:** model triggers truncation/artifact spill and follows the
    next-action guidance.
11. **Invalid calls:** model recovers from malformed args and unsupported tool
    names.
12. **TUI/PTY:** operator sees clear tool rows, permission prompts, artifacts,
    and failure states.

### 7.3 Harness comparison dogfood

For scenarios with a direct Harness equivalent:

- Run the scenario in Harness reference when feasible and record the transcript
  or artifact.
- If Harness cannot be run locally, inspect the current source and record the
  reason runtime comparison was unavailable.
- Differences are allowed only when documented as `harness_adapted` or
  `deferred_decision` in the inventory.

---

## 8. Deferred decision list

These items looked aligned with the desired roadmap, but they must remain
explicit decisions. Agents must not implement them opportunistically.

| Item | Default stance | Evidence required to promote |
|---|---|---|
| `write` tool | Implemented by 2026-06-29 promotion decision. Keep hashline `edit` as normal mutation route, but expose ruleset-compatible whole-file create/overwrite as a public native tool for provider/toolcall parity. | Promotion record below covers source comparison, permission/replay/artifact design, docs/catalog updates, tests, and dogfood. |
| `apply_patch` tool | Implemented by 2026-06-29 promotion decision. Expose ruleset-compatible patch application as a public native tool; do not treat it as a stealth alias for `edit`. | Promotion record below covers source comparison, permission/replay/artifact design, docs/catalog updates, tests, and dogfood. |
| Model-gated edit/write/apply-patch swapping | Deferred. Preserve explicit profile toolsets first. | Real provider/model traces showing tool selection failures that profile descriptions cannot fix. |
| Dynamic JS/TS local tool plugins | Deferred to extension strategy. | Approved runtime plugin-host design covering security, permissions, replay, config, dependency loading, and redaction. |
| Harness `/experimental/tool*` API | Deferred. Static catalog endpoint is not enough. | Product requirement for API compatibility and a design matching provider/model/profile-filtered schemas. |
| `ToolChoice::Required` | Deferred but kept visible. | Concrete provider/model flow requiring mandatory tool use beyond prompt/profile policy. |
| Unknown-tool repair | Rejected for this PRD by 2026-06-30 disposition. Preserve strict fail-closed behavior plus deterministic recovery text; do not add narrow repair without live evidence. | Future evidence that narrow repair improves model recovery without hiding provider bugs. |
| Binary/media attachments | Deferred for full general attachment parity. Limited `read` media/provider lowering is covered by implemented workstream evidence; broader binary/media attachment behavior remains out of scope. | Real `read`/webfetch use cases requiring model-visible image/PDF attachment parts and a redaction-safe artifact design. |
| Full Harness ecosystem parity | Deferred. This PRD targets tool parity, not cloning Harness. | Maintainer decision that external users need ruleset-compatible ecosystem behavior. |

Each promoted decision must add:

1. Source comparison note.
2. Security/permission/replay design.
3. Public docs and catalog updates.
4. Tests.
5. Dogfood scenarios.

### 8.1 Promoted decision: `write` and `apply_patch`

Date: 2026-06-29.

1. **Source comparison note.** The implementation re-read the local Harness
   built-in registry at `inspirations/packages/core/src/tool/builtins.ts`
   plus the matching Harness `write` and `apply_patch` tool definitions. Harness
   keeps hashline `edit` as the normal mutation workflow, while exposing explicit
   public `write` and `apply_patch` native IDs for model/toolcall compatibility.
2. **Security/permission/replay design.** Both tools stay under the existing
   `EditFs` permission class, workspace path normalization, atomic write/apply
   behavior, replay-derived inspection boundaries, and artifact/event redaction
   rules. `apply_patch` extracts touched paths from `patchText`/`patch_text`
   before permission resolution so path-scoped edit rules cover patch-shaped
   calls.
3. **Public docs and catalog updates.** `docs/native-tool-catalog.md`, the native
   catalog, parity fixtures, provider payload snapshots, and native parity tests
   list `write` and `apply_patch` as public native tools with ruleset-compatible
   provider names.
4. **Tests.** `native_execution_surface_test` covers `write`, `apply_patch`, and
   exact edit behavior; `coord_auth_test` covers patch path-scoped permission
   denial; provider payload snapshots cover provider-visible schema export.
5. **Dogfood scenarios.** Deterministic surface dogfood covers `write` create and
   overwrite, `apply_patch` add/update/delete and bad-patch preflight, exact-edit
   literal behavior, provider-safe schema export, and path-scoped patch denial.
   Live-provider superiority evidence remains unavailable; deterministic parity
   evidence is sufficient for this promotion because the compatibility work
   targets provider-visible Harness-shaped tool calls rather than replacing
   Harness's canonical hashline edit workflow.

### 8.2 Rejected decision: narrow unknown-tool repair

Date: 2026-06-30.

1. **Source comparison note.** P1.4 inspected the local invalid-call references
   under `inspirations/harness` and `inspirations/shuvcode`. Harness keeps the
   existing deterministic `invalid` tool response and strict typed argument
   parsing, but rejects an automatic unknown-tool repair path for this PRD.
2. **Security/permission/replay design.** Unknown or malformed tool calls stay
   fail-closed. Harness does not add a permissive catch-all executor, does not
   guess provider intent, and does not hide provider/tool-name bugs behind a
   silent rewrite. Recovery text is replayable tool/error output with no extra
   side effects or permission surface.
3. **Public docs and catalog updates.** No new public runtime capability is
   promoted. The dated progress ledger records the rejection and the remaining
   evidence threshold for any future promotion.
4. **Tests.** Existing P1.4 coverage proves malformed native arguments produce
   deterministic schema-rewrite guidance while preserving strict serde parsing.
   Because narrow unknown-tool repair is rejected, no repair-path tests are added
   or required for this PRD.
5. **Dogfood scenarios.** Deterministic malformed-call dogfood remains valid for
   recovery text. No live unknown-tool auto-repair dogfood is claimed.

---

## 9. Documentation and evidence updates

Implementation work must update docs together with code:

- Native tool IDs/schema/capabilities: update `docs/native-tool-catalog.md`,
  catalog code, and `native_tool_parity_matrix_test`.
- Provider request/schema behavior: update provider support docs/tests if public
  behavior changes.
- Permission behavior: update `docs/permissions.md` and permission tests.
- Session/replay behavior: update `docs/sessions-and-replay.md` and event docs
  tests.
- Test lane or dogfood evidence shape: update `docs/testing.md` and lane scripts
  only when behavior actually changes.
- Progress/evidence: create or update `docs/tools-parity-progress.md`
  with dated rows. Do not edit historical status to hide old limitations.

Progress rows should use this shape:

| Date | Commit | Workstream | Harness source | Harness source | Tests | Dogfood evidence | Status | Notes |
|---|---|---|---|---|---|---|---|---|

---

## 10. Implementation task cards

### P0.1 Create parity inventory

**Goal:** Generate and test the §1 inventory.

**Must not:** manually maintain an untested Markdown-only table.

**Acceptance:** inventory covers all Harness native IDs and current Harness
reference built-ins with status values.

**Verification:** `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test`
plus the new inventory test.

**Dogfood:** compare doctor/catalog output to inventory.

### P0.2 Add real provider payload snapshots

**Goal:** Snapshot real active registry provider definitions for representative
profiles and MCP fixture tools.

**Must not:** rely only on synthetic read/bash schemas.

**Acceptance:** snapshots cover canonical IDs, sanitized names, descriptions,
schemas, and provider request modes.

**Verification:** `cargo nextest run -p harness-providers request_serialization` and
targeted provider serialization tests.

**Dogfood:** mocked prompt with event-log inspection.

### P0.3 Add provider-family schema compatibility tests

**Goal:** Prove or reject Harness-inspired schema normalization needs.

**Must not:** add provider-specific semantics to the coordinator.

**Acceptance:** strict OpenAI-compatible, Gemini-like, and Kimi-like schema cases
are covered.

**Verification:** `cargo nextest run -p harness-providers`.

**Dogfood:** live/recorded provider run for each claimed provider family, or
explicit unchecked status.

### P1.1 Improve descriptions and field schemas

**Goal:** Make model-facing schemas/descriptions clear for sparse tools.

**Must not:** weaken strict serde/schemars boundaries or add permissive unknown
fields.

**Acceptance:** inventory shows field descriptions and guidance for high-risk
tools; snapshots prevent regression.

**Verification:** harness-tools schema tests and provider snapshots.

**Dogfood:** model chooses valid tools/arguments from vague prompts.

### P1.2 Harden task/background/batch/skill

**Goal:** Make Harness control-plane tools easy and safe for models and users.

**Must not:** allow worker redelegation bypasses or ambiguous background IDs as
primary selectors.

**Acceptance:** task/background/batch/skill tests cover happy and failure paths.

**Verification:** `cargo nextest run -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order_test`,
`cargo nextest run -p harness-tools --test native_control_plane_tools_test`, and skill
tests.

**Dogfood:** background, cancellation, batch, and skill scenarios from §7.2.

### P1.3 Normalize output/artifact/truncation presentation

**Goal:** Make large output and artifact behavior consistent and model-actionable.

**Must not:** drop full output when an artifact should preserve it.

**Acceptance:** tool results include concise summary plus artifact/next-action
guidance for large output.

**Verification:** tool-specific truncation/artifact tests and secret gates.

**Dogfood:** large outputs from read/grep/bash/session/task.

### P1.4 Improve invalid-call recovery

**Goal:** Preserve strictness while making failures recoverable by the model.

**Must not:** create a permissive catch-all execution path.

**Acceptance:** malformed calls produce deterministic, actionable tool messages.

**Verification:** coordinator/provider stream tests and native tool args tests.

**Dogfood:** invalid-call scenario suite from §7.2.

### P1.5 Add permission and nested-intent presentation tests

**Goal:** Make operator trust surfaces clear for direct and nested tool calls.

**Must not:** hide child permissions behind a harmless-looking wrapper.

**Acceptance:** permission prompts and transcript/tool rows identify effective
tools and side effects.

**Verification:** harness-core permission tests, harness-tools batch tests,
TUI/transcript tests where display is changed.

**Dogfood:** CLI/TUI permission scenarios from §4.9.

### P2.1 Add TUI/CLI tool display descriptors

**Goal:** Bring common tool rows to production-grade operator readability.

**Must not:** claim visual parity without render/PTY evidence.

**Acceptance:** display tests cover common tool states and Harness-only tools.

**Verification:** `cargo nextest run -p harness-tui --test deterministic_render_test`
and targeted transcript/render tests.

**Dogfood:** PTY/TUI capture with artifact provenance.

### P3.x Promote deferred decisions only by record

**Goal:** Resolve §8 items deliberately.

**Must not:** fold deferred features into unrelated PRs.

**Acceptance:** each decision has source evidence, security/permission design,
tests, docs, and dogfood scenarios.

**Verification:** depends on the promoted item.

**Dogfood:** item-specific, recorded before completion.

---

## 11. Final acceptance checklist

Status as of 2026-06-30: this checklist is closed for the deterministic
parity/provider-boundary scope and the credential-enabled Umans live dogfood
recorded in the progress ledger. The live Umans evidence covers GLM/Kimi model
responses and model-requested `glob`/`read` tool use; `doctor` remains scoped to
local readiness and does not claim provider execution proof.

- [x] Current Harness source was inspected for every implemented task card.
- [x] The parity inventory exists, is tested, and has no `needs_work` P0/P1
      rows.
- [x] P0/P1 task cards are complete or explicitly re-scoped.
- [x] Deferred items are dispositioned.
- [x] Public docs and catalog/tests are updated together.
- [x] Provider payload tests use real registry tool definitions.
- [x] Native tool tests cover happy, edge, failure, permission, large-output,
      cancellation, and replay cases.
- [x] Dogfood evidence rows exist for every workstream.
- [x] `scripts/test-lanes.sh fast`, `quality-gates`, and `all-deterministic`
      pass or have documented pre-existing blockers unrelated to this work.
- [x] No source, artifact, cassette, support export, screenshot, or log in the
      credential-enabled Umans evidence contains raw credential material. Live
      event logs may include provider reasoning delta events and are treated as
      local session evidence, not public support material.
