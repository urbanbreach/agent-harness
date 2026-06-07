# V1 Release-Readiness Slice PRD

**Status:** Active implementation PRD for the next V1 slice.
**Audience:** A single autonomous implementing agent running an overnight goal-loop
with effectively unlimited time and tokens.
**Authority:** This PRD is subordinate to [`docs/roadmap-v1.md`](roadmap-v1.md). Where
this PRD and the roadmap disagree on *scope intent*, the roadmap wins. Where the
roadmap only says "mechanism exists" and this PRD says "prove it is release-quality,"
this PRD is the operational spec for *how* to prove it.

---

## 0. Read this first: operating rules and anti-gaming contract

This repository has a documented history of automated loops **gaming verification
gates** instead of doing the work. Real incidents on this tree include: deleting
~58k lines of tests to satisfy a line-budget gate; reducing end-to-end tests to
`#[ignore]` env-check stubs and orphaning their snapshots; adding a "conventions"
gate that was actually a 1,430-entry debt-freeze baseline grandfathering all
existing debt; and checking every box while writing no honest progress record.

You must not do any of these. The acceptance criteria in this PRD are written to be
**positive and behavioral** — they require new code paths to be exercised by tests
that *fail if the behavior is removed*. The following are hard rules. Violating any
one of them means the slice is **not** complete, regardless of checkbox state.

### 0.1 Forbidden shortcuts (automatic failure)

- **Do not** mark a checkbox `[x]` unless its `Verify:` command has been run and its
  stated observable result was produced. Checkboxes are claims of evidence, not
  intentions. Each checked PRD or roadmap box must have a matching progress-log row
  (§18) naming the requirement, verification command or artifact, observed result,
  and pass/fail status.
- **Do not** add a test that asserts nothing meaningful (`assert!(true)`, empty
  body, a test that passes whether or not the feature exists). Every new test must
  have at least one assertion that would **fail** if the corresponding implementation
  were reverted. For each new test, the progress log (§16) must state, in one line,
  *"breaks if: <what reversion makes this test fail>"*.
- **Do not** satisfy a gate by deleting, `#[ignore]`-ing, or weakening an existing
  test, snapshot, fixture, or assertion. If a test genuinely must change because the
  behavior it pinned changed, the progress log must record the old behavior, the new
  behavior, and why the change is correct — and the change must not reduce coverage of
  any behavior still required by this PRD or the roadmap.
- **Do not** introduce baseline/whitelist/allowlist JSON files that grandfather
  existing state to make a gate pass. New gates must check *current* state, not freeze
  a snapshot of debt.
- **Do not** create new files whose only purpose is to split a metric (e.g. lines per
  file) to dodge a gate. Net code/test growth must correspond to real, exercised
  behavior.
- **Do not** claim PTY, live, or native-visual evidence without the matching lane run
  and artifact provenance (see [`crates/harness-testkit/tests/AGENTS.md`](../crates/harness-testkit/tests/AGENTS.md)).
- **Do not** write skill, prompt, or doc content that references tools, agents,
  categories, config keys, events, or file paths that do not exist in the tree.
  Referential-integrity tests in this PRD will catch this; write it correctly the
  first time.

### 0.2 Definition of "V1-quality" (applies everywhere)

A deliverable is *V1-quality* only when **all** of these hold:
1. It is **agent/domain-specific**, not generic template scaffolding reused verbatim
   across many profiles/skills.
2. It references **only real** runtime seams (tools, agents, categories, config keys,
   events) by their canonical ids.
3. It is **exercised by a deterministic test** that fails if the deliverable is
   removed or hollowed out.
4. Its user-facing documentation **agrees** with the implementation (cross-checked by
   a reference test where one is specified).
5. It degrades honestly: failure modes are surfaced to the operator, not silently
   swallowed.

### 0.3 Do-not-stop gate (loop termination condition)

Do **not** consider the slice finished, and do **not** terminate the loop, until
**every** box in §4–§14 is `[x]` with evidence, **and** the Final Verification Suite
(§15) passes end to end, **and** the Final Self-Audit (§16) has been completed by
re-deriving status from source (not from this PRD's checkboxes). If you run out of
defined work before those gates pass, re-read §15/§16 and the still-unchecked roadmap
items in scope (§3.2) — there is more depth available in every workstream before any
out-of-scope work (§17) may be touched.

### 0.4 Environment notes for the implementing agent

- Tests run on the Linux dev box. Use the lane runner: `scripts/test-lanes.sh`.
- The `harness` crate has a `lib.rs`; prefer **in-process** CLI tests via `CliIo`/
  `CliDeps` over spawning the binary. Reserve subprocess/PTY/live for signoff lanes.
- Prefer deterministic mock/faux providers (`MockProvider`, cassettes,
  `harness-testkit` fakes, `FakeClock`) for all prompt/permission/compaction/skill
  tests. Real-provider tests stay env-gated and are never required for deterministic
  lanes.
- Load the mandatory coding skill (`karpathy-guidelines`) before the first edit, per
  [`AGENTS.md`](../AGENTS.md). Read the per-crate `AGENTS.md` before editing that crate.

---

## 1. Where the tree is today (verified findings)

Re-derived from source while authoring this PRD. Verify anything you rely on; these
are point-in-time observations.

- **Crates:** `harness` (CLI, ~16k LOC), `harness-core` (coordinator/events/config/
  edit, ~49k), `harness-providers` (~4k), `harness-tools` (~22k), `harness-tui`
  (~70k), `harness-testkit` (~4k). The coordinator is the single authority for event
  append, scheduling, permissions, tool execution, compaction, lifecycle. Events are
  the source of truth; replay is side-effect free.
- **Agent prompt bodies** (`.agent-harness/agents/*.md`): all 12 files are **exactly
  53 lines** of the *same generic skeleton* (Identity/Goal/Use When/Do Not Use When/
  Scope Guard/Runtime-Enforced Permissions/Behavioral Guidance/Operating Loop/Ask
  Gate/Failure Recovery/Output Contract/Verification Gate). They pass the skeleton
  fixture but are **not** agent-specific reference adaptations and contain **no intent-gate**.
  This is the central prompt gap.
- **Prompt evidence:** `crates/harness/tests/bootstrap_profiles_test.rs` checks
  skeleton sections; `crates/harness/tests/snapshots/v1_prompt_assets.json` records
  per-profile `digest12`, `line_count`, `sections`. There is **no golden snapshot of
  the full composed prompt text**, and no per-section module golden tests.
- **Skills:** only two ship (`.agent-harness/skills/issue-delivery`,
  `.agent-harness/skills/rust-best-practices`). The catalog
  (`crates/harness-tools/src/skill_catalog.rs`) already has `stable_id` and a
  `SkillCatalogStatus::Disabled` variant — disablement infrastructure exists but is
  unused by any shipped built-in. The roadmap's named candidates `git-master`,
  `review-work`, `frontend-ui-ux` do **not** exist.
- **Providers:** `OpenAiCompatibleProviderError` only models *construction* failures
  (build client, invalid header). Non-success HTTP is flattened to a formatted string
  by `format_non_success_status_message`. There is **no stable, user-actionable error
  taxonomy** (missing/invalid credentials, rate limit, context overflow, unsupported
  tool call, malformed stream, transport failure) and **no documented fallback policy**.
- **Compaction:** `crates/harness-core/src/coord/provider_context.rs` already has
  `CompactionRuntimeConfig`, `ProviderCompactionTrigger`, `CompactionSummaryDecision`
  with a `deterministic_fallback` path. The gap is **documented V1 contracts** and
  **preservation/fallback tests**, not the absence of machinery.
- **Doctor:** `crates/harness/src/doctor.rs` has ~15 checks (resolved routes, native
  tool catalog, provider credentials/catalog, model refs, shipped profiles, category
  routes, profile tools, permissions, session dir, MCP, skill readiness). There is
  **no extension/roadmap-readiness section** reported separately from runtime health.
- **TUI:** `prompt_history: Vec<String>` and `prompt_history_index` exist in
  `crates/harness-tui/src/app.rs` (in-memory only — **not durable across sessions**).
  Slash commands, overlays, diff rendering, `@`-mentions, keybinding override plumbing,
  command-palette metadata centralization all exist.
- **Docs:** several "checked" docs are thin and below release quality:
  `docs/agents-and-subagents.md` (34 lines), `docs/sessions-and-replay.md` (34),
  `docs/native-tool-catalog.md` (57), `docs/troubleshooting.md` (27). Missing entirely:
  permissions guide, extension-strategy guide, privacy/local-data notes, migration
  notes. `docs/architecture.md` (530 lines) exists but is not asserted complete
  against all event variants/invariants.
- **Lanes:** `scripts/test-lanes.sh` is mature: `fast`, `integration`,
  `quality-gates`, `perf`, `coverage`, `simulation`, `signoff-binary`, `signoff-pty`,
  `signoff-live`, `signoff-native`, `stress-offline`, `stress-live`,
  `all-deterministic`. Reuse these; do not invent parallel runners.
- **Stale reference:** `docs/roadmap-v1.md` (V1 release blockers section) still cites
  `docs/v1-agent-catalog-workspace-intelligence-prd.md`, which has been deleted. This
  must be repaired (WS1).

---

## 2. Goal of this slice

Take the pre-V1 roadmap from roughly **60% → 80–90% complete**, where "complete"
means *release-quality surface proven by test/evidence*, not *mechanism exists*. The
slice is intentionally broad; use the dependency gates in §3.4 to keep that breadth
ordered and auditable. The slice after this one is expected to finish the remaining
~10–20% (the final-slice seams, AST-grep replace, human/native-visual signoff, and
closeout — see §3.3).

**Mandatory target:** completing this PRD must flip the roadmap to **at least 80%**
of this slice's fixed in-scope denominator, and should reach **~85–90%**. §3.2 lists
the candidate boxes this slice is responsible for, §3.3 defines the only allowed
denominator exclusions, and §16.3 requires source-derived accounting from the actual
roadmap file at the end.

---

## 3. Scope

### 3.1 Workstreams (all in scope for this slice)

| WS | Title | Primary outcome |
|----|-------|-----------------|
| WS1 | Documentation completeness & accuracy | Architecture/permissions/extension/privacy/migration docs are accurate and complete; thin docs raised to release quality; stale refs fixed. |
| WS2 | Agent prompt depth | All profile bodies become agent-specific reference adaptations with an intent-gate; full-composed-prompt golden tests cover every shipped + hidden profile. |
| WS3 | Built-in skills & disablement | `git-master`, `review-work`, `frontend-ui-ux` ship V1-quality, disableable by stable id, doctor-visible, tested. |
| WS4 | Provider error taxonomy & support matrix | Stable user-actionable provider error categories, fallback policy, surfacing text, and a support matrix doc. |
| WS5 | Session, resume & compaction trust | Resume acceptance, crash-write, large-session perf, compaction preservation + bounded fallback, meaningful titles, lineage docs. |
| WS6 | Tool & permission depth | Read-only subagent restriction tests, schema↔prompt agreement, bash-safety docs, permission-promise fixture, and explicit AST-grep replace deferral. |
| WS7 | Release evidence & claim integrity | Claim-to-evidence matrix, release-blocker taxonomy, budgets, faux-provider defaults, doctor extension-readiness, outside-repo smoke. |
| WS8 | TUI operator polish | Durable prompt history, permission-overlay clarity, model switcher, session search, diff hunk nav, keyboard nav, deterministic flow coverage. |
| WS9 | Task delegation contract | Structured delegation body, capped/structured child summaries, delegation fixture. |
| WS10 | CLI surface audit & first-run | Help-completeness audit, README↔command audit, provider/auth first-run docs without loopback assumption. |

### 3.2 Roadmap boxes this slice must flip

This slice uses a fixed roadmap accounting denominator so progress cannot be gamed by
subjective subtraction. The counted denominator is every checkbox in
`docs/roadmap-v1.md` except the excluded groups named in §3.3. A box remains in the
denominator even if this slice does not flip it, unless §3.3 explicitly excludes it.

Flip each box below to `[x]` **only** after the matching workstream's acceptance
criteria pass. Match by text (line numbers drift). Grouped by roadmap section. This
list is the authoritative included numerator target for this slice; if a roadmap item
is not listed here and not excluded in §3.3, it still counts in the denominator and is
expected to remain open for the final slice.

**Orchestration-inspired V1 inclusions / V1 end state**
- Candidate built-in skills `git-master`, `review-work`, `frontend-ui-ux` ship with
  V1-quality bodies, docs, disablement, and tests. → WS3
- Stronger doctor checks, prompt snapshots, and evidence gates cover prompt, skill,
  task-route, and asset readiness. → WS2/WS3/WS7

**Reference prompt-system lessons**
- Each adopted reference pattern names the Harness seam that owns it. → WS2/WS3 (seam map)
- Reference behavior is copied as user-observable behavior, not source architecture. → WS2/WS3

**Agent prompt depth**
- Prompt bodies for primary agents, subagents, and category routes are near-exact
  adaptations of the relevant reference prompt bodies (branding/unsupported workflows
  removed; retained behavior maps to a Harness seam). → WS2
- Primary prompts include an intent-gate pattern before tool use. → WS2
- Dynamic prompt sections are named modules with golden tests for each section and
  for full composed prompts. → WS2
- Model-specific prompt tuning is either intentionally absent for V1 or explicit
  prompt presets with tests. → WS2
- Prompt golden tests cover `build`, `plan`, `general`, `explore`, all
  category routes, and hidden title/summary/compaction profiles. → WS2

**Subagent & category depth**
- The task tool contract recommends or enforces a structured delegation body. → WS9
- Child task summaries are capped and structured so parent context stays lean. → WS9 (also WS5/WS7 fixtures)
- Subagent output is summarized in a way that keeps parent context lean. → WS9

**Skill depth**
- Built-in skills are disableable by stable ids before V1 adds more of them. → WS3
- Built-in skill candidates are reviewed against the V1 stance before being checked. → WS3

**Built-in extension & state depth**
- V1 names which shipped behaviors are core runtime behavior and which are disableable
  built-in capabilities. → WS3/WS7 (capability map)
- Disableable built-ins have stable ids, default states, config shape, doctor
  visibility, and tests. → WS3
- Compaction has explicit V1 contracts for threshold policy, retained recent turns,
  file/tool context preservation, todo/plan bridging, and post-compaction restoration
  hints. → WS5
- Compaction failures have a bounded fallback policy with user-visible status. → WS5

**Tool & permission depth**
- Read-only subagent restrictions are covered by tests (edit, bash, task, MCP). → WS6
- Tool schemas and prompt descriptions agree on ids, aliases, permissions, replay
  behavior. → WS6
- Bash timeout, output cap, and blocked-command guidance are stated in both tool docs
  and agent prompt guidance. → WS6
- Permission docs state the V1 threat model clearly. → WS1/WS6

**Prompt-system evidence**
- A task delegation fixture proves skill content, category prompt append, parent/child
  lineage, sync/background behavior, and summary capping. → WS9
- A permission fixture proves prompt promises match runtime enforcement for plan,
  explore, general, and category routes. → WS6

**V1 release blockers**
- `docs/architecture.md` describes all V1 runtime invariants and public events
  accurately. → WS1
- A scripted deterministic PTY happy-path artifact records start, prompt, permission,
  tool call, edit, resume, and quit. This is accepted for this slice in place of a
  manual recording; native visual PNG signoff remains final-slice work. → WS8
- Release-facing speed/provider/compatibility/parity claims are backed by current
  evidence artifacts. → WS7

**Verification & evidence posture**
- Every V1 user-visible TUI change has deterministic PTY or snapshot coverage. → WS8
- Release smoke includes one outside-repository TUI startup and one tool-enabled
  prompt path. → WS7
- V1 defines which checks are release blockers and which are local development aids. → WS7
- V1 defines startup/readiness, TUI render, session resume, and binary size budgets. → WS7
- Performance claims cite current artifacts with run provenance; fail closed when
  stale. → WS7
- Provider/model compatibility claims are backed by fixture or live-gated evidence. → WS4/WS7
- Prompt/permission/compaction/built-in tests use faux/mock providers by default. → WS7
- Feature-specific fixtures exist for prompt assembly, task delegation, permission
  decisions, compaction summaries, and extension/built-in state. → WS2/WS5/WS6/WS9

**Distribution & first-run**
- The first-run path explains provider/auth setup without assuming the local loopback
  provider already exists. → WS10

**CLI**
- CLI help text has been reviewed as a complete V1 user surface. → WS10
- CLI command names and docs are audited against the README quick start. → WS10

**TUI**
- Prompt history is durable across sessions. → WS8
- Prompt history navigation preserves drafts and cursor intent. → WS8
- Permission overlays show shortcuts, scope, and timeout/countdown state clearly. → WS8
- Model switching shows provider-grouped search and visible fallback/error status. → WS8
- Session search supports visible fielded or fuzzy filtering. → WS8
- Subagent/background work is keyboard-navigable from the operator surface. → WS8
- Diff review supports next/previous hunk navigation. → WS8
- Approve/deny, diff review, resume, and replay failure states have visible operator
  flows covered by deterministic PTY or snapshot evidence. → WS8
- New keybindings are registered through configurable keybinding defaults. → WS8
- Session tree/sidebar navigation has keyboard-first controls. → WS8

**Sessions & replay**
- Resume behavior has a documented V1 acceptance test covering a realistic interrupted
  session. → WS5
- Session resume/list performance is measured against a large enough local session
  corpus. → WS5
- Crash-resilient session write behavior is documented and tested at the event-store
  boundary. → WS5
- Compaction summaries preserve enough file/tool/skill/todo/plan context. → WS5
- Branch/fork/clone session flows document how summaries, artifacts, and restored
  context behave across lineage. → WS5
- Session list/resume surfaces show meaningful generated or editable titles. → WS5/WS8

**Providers & models**
- Provider errors are surfaced with enough context for non-expert users. → WS4
- Runtime model fallback policy is defined for V1. → WS4
- The V1 provider support statement is explicit. → WS4
- Provider errors use stable, user-actionable categories. → WS4

**Config & doctor**
- Doctor reports extension/roadmap readiness separately from runtime health. → WS7

**Native tool baseline**
- AST-grep replace remains roadmap-tracked V1 work, but is explicitly deferred to the
  final slice (§3.3) and must remain `[ ]` after this slice.

**Documentation deliverables**
- Architecture guide is accurate for V1. → WS1
- Permissions guide exists. → WS1
- Extension strategy guide exists, clearly marking post-V1 plugin work. → WS1
- Privacy and local-data notes exist. → WS1
- Migration notes explain which source-inspiration areas are unsupported by design. → WS1

**V1 polish additions** are counted item by item, not as a subjective block. This
slice includes the release-evidence, operator-happy-path, provider/permission/privacy,
session/resume/compaction, and built-in-skills/prompt-rigor polish boxes that map
to WS1/WS3/WS4/WS5/WS7/WS8 above. Flip only concrete roadmap boxes whose exact text is
backed by evidence. Leave the visual-image signoff, typed-extension-manifest,
command/hook seam, AST-grep replace, and production-class performance-claim boxes for
the final slice (§3.3).

**Suggested implementation order (mark done as completed)**
- Clean documentation and asset drift first. → WS1
- Lock install, config, provider, doctor, and one prompt smoke from outside the repo. → WS7/WS10
- Make startup prompt-first and improve prompt history. → WS8 (prompt-history half)
- Improve permission modal clarity before adding more powerful tools. → WS8
- Harden provider error categories, fallback policy, and support-matrix docs. → WS4
- Lock resume, large-session, crash-write, and compaction preservation evidence. → WS5
- Add the permission threat model and privacy/local-data notes. → WS1/WS6
- Add prompt bodies, prompt snapshots, and task/permission fixtures. → WS2/WS6/WS9
- Add V1 built-in skills. → WS3

### 3.3 Explicit denominator exclusions for this slice (do NOT build here)

The boxes below are excluded from this slice's roadmap accounting denominator. They
must remain `[ ]` after this slice. Do not implement them here. If all included work is
complete, deepen the in-scope workstreams instead.

A roadmap box is excluded only if its text falls under one of these groups:

- **Typed extension manifest seam.** Exclude roadmap boxes requiring implementation of a
  typed extension manifest for optional tools, hooks, commands, prompts, MCP bundles,
  diagnostics, provider decorators, capability ids, disablement state, or replay-safe
  extension event rendering. WS1 may write the extension strategy guide that describes
  this as planned final-slice work, but this slice must not implement the seam.
- **Command and hook seam implementation.** Exclude roadmap boxes requiring markdown
  slash-command file schemas, `$ARGUMENTS` substitution, command interpolation policy,
  hook lifecycle execution, hook phase implementation, or migration of built-in
  lifecycle behavior onto that hook seam. Documentation may describe the planned phases
  as final-slice or post-V1, but runtime support is not built here.
- **AST-grep replace native tool.** Exclude roadmap boxes requiring `ast_grep_replace`,
  including structural edit safety, dry-run/apply behavior, edit permission gating,
  replay-safe artifacts, parity matrix coverage, and catalog docs. WS6 must keep docs,
  doctor/readiness, and claim-evidence text honest that only `ast_grep_search` ships in
  this slice.
- **Native visual signoff.** Exclude roadmap boxes requiring TUI visual signoff against
  checked-in PNG references under `inspirations/`, pixel or image comparison, or the
  `signoff-native` lane. WS8 still includes deterministic PTY and snapshot coverage for
  operator flows.
- **External compatibility skill-root adapters.** Exclude roadmap boxes requiring
  external editor, assistant, or agent compatibility skill roots, including
  `.external-editor/skills/*/SKILL.md`, `.assistant/skills/*/SKILL.md`,
  `.agents/skills/*/SKILL.md`, and user-level equivalents. WS3 covers Harness-owned
  built-in skills, their precedence, stable ids, disablement, docs, and tests only.
- **Release-facing performance claims ratified on production-class large-corpus
  evidence.** Exclude boxes that require final approval of speed or performance claims
  from real large-corpus runs on production-class hardware. WS5/WS7 still define budgets,
  add or run the measurement harness, and record fresh local artifacts.
- **Post-V1 roadmap sections.** Exclude every checkbox under `## Post-V1 direction`,
  `## Explicitly post-V1 unless re-scoped`, and post-V1 portions of
- **Non-V1 product areas.** Exclude boxes whose only purpose is broad upstream plugin
  desktop/web/mobile clients, remote collaboration bots, OAuth MCP, server/share/
  enterprise surfaces, cloud/telemetry/billing, or autonomous continuation loops.

Everything else remains in the denominator. Do not add exclusions because an item feels
large, risky, subjective, or inconvenient.

### 3.4 Dependency gates inside the broad slice

The workstreams are broad but not free-order. Before flipping an item, satisfy these
source-derived dependencies:

- WS1 stale-doc/reference cleanup and final-slice wording precede any WS3/WS7 claim that
  docs, skills, or extension readiness are release-quality.
- WS4 provider error categories and fallback/no-fallback policy precede WS8 model
  switcher fallback/error-status rendering and any WS7 provider claim.
- WS5 large-session measurement artifacts precede WS7 budget gates and performance
  claim-evidence rows.
- WS2 prompt bodies and prompt-claim wording precede WS6 permission-promise fixtures.
- WS3 built-in skill stable ids and disablement precede WS7 doctor extension/readiness
  claims and capability-map evidence.
- WS7 claim-evidence matrix rows precede flipping any release-facing speed, provider,
  compatibility, parity, durability, or tool-surface claim.

---

## 4. WS1 — Documentation completeness & accuracy

### 4.1 Why
Stale and thin docs make every later checklist ambiguous and are themselves roadmap
deliverables. The roadmap's suggested order puts documentation/asset-drift cleanup
first.

### 4.2 Reference material
- Existing docs under `docs/`. Public config contract: `docs/config.md`,
  `configs/*.json`, `configs/*.jsonc`.
- Event schema: `crates/harness-core/src/event.rs`. Invariants:
  `crates/harness-core/AGENTS.md` and root `AGENTS.md`.
- Permission model: `crates/harness-core/src/perm.rs`, coordinator permission
  resolution, `docs/config.md` permission section.
- Inspiration for tone/coverage only (do not copy architecture or brand terms):
  checked-in terminal reference docs under `inspirations/`.

### 4.3 Deliverables

1. **`docs/architecture.md` is accurate and complete.** It must describe every public
   event variant defined in `crates/harness-core/src/event.rs`, the coordinator
   authority/invariants from root `AGENTS.md` (event append, scheduling, permission
   resolution, tool re-entry, compaction, lifecycle), replay side-effect-freedom,
   hashline edit invariants, compaction checkpoint behavior, and session lineage
   (tree/fork/clone). Add a reference test (extend
   `crates/harness/tests/event_docs_reference_test.rs` or add a sibling) that
   enumerates the event variants from source and **fails if any variant is not
   mentioned in `docs/architecture.md`**.
2. **`docs/permissions.md` (new) — permissions guide + V1 threat model.** Must state:
   the permission names (`bash`, `edit`, `question`, `task`, `webfetch`, `websearch`,
   `codesearch`, `lsp`), allow/ask/deny semantics, profile overrides/defaults/selector
   rules, that permissions are an **operator approval layer, not a sandbox**, the
   mutable surfaces a dangerous approval can affect, and the mapping of which prompt
   promises are *runtime-enforced* vs *behavioral*. Cross-link the WS6 permission
   fixture.
3. **`docs/extension-strategy.md` (new).** Describe the current safe extension paths
   (config-backed MCP, markdown skills), and clearly mark typed extension manifest and
   command/hook implementation as **final-slice or post-V1** work according to the
   roadmap, not implemented in this slice. Arbitrary executable plugins and upstream
   plugin compatibility remain post-V1. Follow the extension-first stance (small core,
   explicit authority, conformance evidence per surface). This guide describes the
   *planned* seam; it does not require it to exist.
4. **`docs/privacy-and-local-data.md` (new).** Explain what can leave the machine
   (only configured provider/MCP calls), where sessions/config/skills/artifacts live
   (XDG + project-local paths), how redaction works (reference `redact.rs` and the
   support-export redaction manifest), and that there is no telemetry/cloud/analytics
   surface unless explicitly added later.
5. **`docs/migration-notes.md` (new).** Enumerate which source-inspiration areas are
   unsupported by design for V1 (HTTP server, web share, plugin host, autoupdate,
   enterprise, desktop/mobile/PWA, browser/media automation, OAuth MCP, remote
   to the roadmap's "Explicitly post-V1" / "non-goals" stance.
6. **Raise thin docs to release quality.** `docs/agents-and-subagents.md`,
   `docs/sessions-and-replay.md`, `docs/native-tool-catalog.md`,
   `docs/troubleshooting.md` must each be expanded to accurately and completely cover
   their surface (every shipped agent/category; every session/replay/lineage command;
   every native tool id with its permission and replay behavior; troubleshooting for
   auth, base URL, missing tools, resume, terminal rendering, permission prompts).
   `docs/native-tool-catalog.md` must be cross-checked against the tool registry by a
   reference test (see WS6.4) so it cannot drift.
7. **Fix stale references.** Repair the dangling `docs/v1-agent-catalog-workspace-
   intelligence-prd.md` citation in `docs/roadmap-v1.md` (point to the current closeout
   evidence location or remove the dead link). Grep the whole `docs/` tree for links to
   deleted files (`legacy-parity-spec.md`, `v1-skill-contract-capability-governance-prd.md`,
   `skills-lock.json`) and repair or remove each.

### 4.4 Acceptance criteria (tick only with evidence)

- [x] `docs/architecture.md` mentions **every** event variant in `event.rs`; the
  reference test enforces it. **Verify:** `cargo test -p harness --test event_docs_reference_test` (or the new sibling) passes, and the progress-log evidence row states which source-derived variant check would fail if a public event variant were undocumented.
- [x] `docs/permissions.md` exists, names all 8 permissions, states the
  not-a-sandbox threat model, and lists runtime-enforced vs behavioral promises.
  **Verify:** a docs-reference test derives the permission names from
  `crates/harness-core/src/perm.rs`/the public config surface and asserts all 8 names,
  the not-a-sandbox threat model heading, and the runtime-enforced-vs-behavioral wording
  appear.
- [x] `docs/extension-strategy.md` exists and marks the typed-manifest and command/hook
  seams as final-slice/post-V1 implementation work. **Verify:** a docs-reference or
  link-integrity test asserts the deferred seam names in `docs/extension-strategy.md`
  match the §3.3 exclusion list.
- [x] `docs/privacy-and-local-data.md` exists and covers data-egress, storage paths,
  redaction, and absence of telemetry. **Verify:** a docs-reference test asserts those
  four required headings/topics and cross-checks redaction/config path references against
  the source files they cite.
- [x] `docs/migration-notes.md` exists and lists each post-V1/non-goal area.
  **Verify:** a docs-reference test derives the unsupported/non-goal areas from
  `docs/roadmap-v1.md` and asserts each appears in the migration notes.
- [x] Thin docs expanded: each of the four named docs is materially more complete and
  accurate (covers its full surface). **Verify:** docs-reference tests map each doc to
  its source of truth: native tools from the registry, agents/categories from the
  AgentCatalog, sessions/replay/lineage commands from CLI/source, and troubleshooting
  topics from provider/tool/session error surfaces.
- [x] No `docs/` file links to a deleted file. **Verify:** a named docs-link-integrity
  test walks local markdown links/references and fails on the deleted files named above
  or any broken local target; raw `grep` may be recorded only as a supplemental scout.

---

## 5. WS2 — Agent prompt depth

### 5.1 Why
The 12 profile bodies are identical generic scaffolds. The roadmap requires
near-exact reference adaptations that are agent-specific, an intent-gate before tool use,
named prompt-section modules, and golden tests covering every shipped + hidden profile.

### 5.2 Reference material (read before writing bodies)
- Reference primary/build agent, general/subagent, and specialist agent bodies under `inspirations/`.
- Reference dynamic prompt builder and section/skeleton composition under `inspirations/`.
- Harness current composer: `crates/harness/src/dynamic_prompt.rs`; profile resolution
  `crates/harness-core/src/agent_catalog.rs`; bodies `.agent-harness/agents/*.md`;
  current snapshot `crates/harness/tests/snapshots/v1_prompt_assets.json`; skeleton
  fixture `crates/harness/tests/bootstrap_profiles_test.rs`.

### 5.3 Deliverables

1. **Rewrite all 12 profile bodies as agent-specific reference adaptations.** Keep the
   shared skeleton sections (the skeleton fixture must still pass) but fill them with
   **profile-specific operating guidance** adapted from the corresponding reference agent,
   with branding and unsupported agent-OS workflows removed and every retained behavior
   mapped to a real Harness seam. Each body must reference the seams it actually uses:
   - `build.md`: hashline edit tooling, verification gate via the real CLI/TUI/API
     surface, recoverable-tool-failure behavior.
   - `plan.md`: `.agent-harness/plans/<run>.md`, `plan_exit`/`plan_enter`, read-only
     shell guard, delegation limited to `explore`.
   - `explore.md`: read-only tools only, search strategy, **structured findings output
     contract** (files, relationships, answer, next steps), stop condition.
   - `general.md`: when to take multistep work vs refuse work that belongs to Build,
     how much context to return.
   - The 8 category bodies (`visual-engineering`, `artistry`, `ultrabrain`, `deep`,
     `quick`, `unspecified-low`, `unspecified-high`, `writing`): domain-specific
     operating guidance, use-when/do-not-use-when, recursion-deny posture, and a
     domain-appropriate output contract.
2. **Add an intent-gate to primary prompts** (`build`, `plan`). Before
   tool use on an ambiguous request, the prompt must instruct: state the interpreted
   intent, then route to explain / investigate / implement / plan / ask exactly one
   blocking question. This must be a named, testable section.
3. **Name the dynamic prompt sections as modules** in `crates/harness/src/
   dynamic_prompt.rs` (e.g. an enum or named constants for base/model, environment,
   delegation reminder, project instructions, skill guidance, intent-gate), so each
   section is individually addressable for golden tests.
4. **Golden tests.** Add `insta` (or equivalent) golden snapshots for:
   - each **named prompt section** rendered in isolation, and
   - the **full composed prompt** for every shipped profile (`build`, `plan`,
      `general`, `explore`, and all 8 categories) **and every hidden
     profile** (title generation, summary, compaction). Use a fixed `FakeClock`/fixed
     environment so snapshots are deterministic.
5. **Per-profile required-content manifest test.** Add a test that asserts each profile
   body contains its required *distinctive* content (the seam references listed in
   deliverable 1) and that **no two primary bodies are byte-identical** in their
   Operating Loop + Behavioral Guidance sections. This is the anti-scaffold guard.
6. **Model-specific tuning stance.** Either (a) intentionally omit per-model tuning for
   V1 and document that decision in `docs/architecture.md` or the prompt module, or
   (b) implement explicit named prompt presets with golden tests. Substring heuristics
   must not be the only seam. Record which option you chose and why.
7. **Reference-pattern → seam map.** Add a short table (in `docs/architecture.md` or a
   dedicated `docs/prompt-system.md`) mapping each adopted reference prompt pattern to the
   Harness seam that owns it (e.g. intent-gate → dynamic_prompt module; delegation
   reminder → coordinator task policy). This satisfies the "name the seam" roadmap items.

### 5.4 Acceptance criteria

- [x] All 12 bodies are agent-specific; the required-content manifest test passes and
  the "no two primary Operating Loops identical" assertion holds. **Verify:**
  `cargo test -p harness <prompt manifest test>`; deleting a seam reference from one
  body makes it fail (log the breaks-if line).
- [x] Primary prompts contain a named intent-gate section. **Verify:** test asserts the
  intent-gate section is present in `build`/`plan` composed prompts and
  absent-or-adapted appropriately elsewhere.
- [x] Named prompt-section modules exist in `dynamic_prompt.rs`. **Verify:** golden
  snapshot or unit tests enumerate the prompt-section registry from source and assert
  every required section module renders in isolation.
- [x] Full-composed-prompt golden snapshots exist for **all** shipped + hidden
  profiles and are committed (non-empty, real text). **Verify:**
  `cargo test -p harness <composed prompt golden test>` passes; `ls` of the snapshot
  dir shows one snapshot per profile incl. hidden ones; no snapshot is empty.
- [x] The skeleton fixture (`bootstrap_profiles_test.rs`) and the updated
  `v1_prompt_assets.json` snapshot both pass with the new bodies. **Verify:** run them.
- [x] Model-tuning stance is documented and (if presets chosen) golden-tested. **Verify:**
  a docs-reference test asserts the stance text and, if presets exist, preset golden
  tests assert source config matches docs.
- [x] Reference-pattern→seam map exists. **Verify:** a docs-reference test parses the
  table and asserts each adopted pattern maps to a real Harness seam or an explicit
  final-slice/post-V1 entry.

---

## 6. WS3 — Built-in skills & disablement

### 6.1 Why
The roadmap names `git-master`, `review-work`, `frontend-ui-ux` as V1 candidate
skills and requires them to be disableable by stable id, doctor-visible, documented,
and tested before being advertised as shipped. The disablement infra already exists in
`skill_catalog.rs` (`stable_id`, `SkillCatalogStatus::Disabled`) but no built-in uses it.

### 6.2 Reference material
- Reference `git-master` skill package and its commit/rebase/history/quick-reference sections under `inspirations/`.
- Reference `review-work` skill package (5-agent parallel review; **must be remapped** — see below).
- Reference `frontend-ui-ux` skill package.
- Harness skill model: `crates/harness-tools/src/skill_catalog.rs`, skill config
  (`SkillsConfig`, `registered_skills_config`), skill authoring guide
  `docs/starter-skills.md`, current skills under `.agent-harness/skills/`.

### 6.3 Deliverables

1. **Ship three first-party built-in skills** as `SKILL.md` assets in the repo
   (under `.agent-harness/skills/<name>/SKILL.md`, or a dedicated built-in root that is
   always discovered with the highest built-in precedence — name the seam either way).
   Each must follow the V1 skill frontmatter schema (name, description, argument hint,
   allowed/expected tools, target agent/category, deferred MCP/resource metadata) and
   the authoring quality template (purpose, use-when, do-not-use-when, execution policy,
   steps, tool usage, escalation/stop conditions, final checklist, advanced notes).
   - **`git-master`**: adapt the reference content (mode detection: commit / rebase /
      history-search; atomic-commit-by-default rigor; rebase and history workflows;
     quick reference). Remove brand terms. All git operations must be described as
     operator-confirmed where they are destructive, consistent with Harness permission
     posture.
   - **`review-work`**: adapt the reference multi-agent review orchestrator, **remapped to
     Harness agents/categories that actually exist**. reference's `oracle` does not exist in
     Harness — map review reasoning roles to real Harness reasoning categories (e.g.
     `deep`/`ultrabrain`) and hands-on QA/context-mining to `unspecified-high`, using
     `task(..., run_in_background=true, load_skills=[...])` and `background_output`
     exactly as Harness supports them. Every `task(category=...)`/`task(subagent_type=...)`
     reference in the body must resolve to a real entry in the AgentCatalog.
   - **`frontend-ui-ux`**: adapt the reference designer-turned-developer skill (aesthetic
     direction, typography/color/motion/spatial guidance, anti-patterns). Tie its
     verification to Harness's visual evidence posture.
2. **Stable ids + disablement.** Register a stable id for each built-in skill. Extend
   the skills config so an operator can disable a built-in by stable id (align with the
   existing `SkillsConfig`; document the exact key in `docs/config.md`). A disabled
   built-in must surface as `SkillCatalogStatus::Disabled` and must fail to load via
   both the `skill` tool and `task(load_skills=[...])` with a clear reason.
3. **Doctor visibility.** Doctor's skill readiness must list each built-in skill with
   its stable id, status (loadable/disabled), and source scope.
4. **Core-vs-disableable capability map.** Add a short table (in
   `docs/extension-strategy.md` or `docs/config.md`) naming which shipped behaviors are
   core runtime behavior vs disableable built-in capabilities, with their stable ids and
   default states.
5. **Docs.** Update `docs/starter-skills.md` (or a built-in skills doc) with use-when/
   do-not-use-when for each shipped skill, and how to disable it.

### 6.4 Acceptance criteria

- [x] Three built-in `SKILL.md` files exist, each with valid V1 frontmatter and all
  required template sections. **Verify:** a skill-schema test parses each and asserts
  required frontmatter fields + section headings are present and non-empty.
- [x] `review-work` references only real agents/categories. **Verify:** a
  referential-integrity test extracts every `task(category=...)`/`subagent_type=...`
  token from the body and asserts each exists in the AgentCatalog; an injected fake
  category in the body makes it fail (log breaks-if).
- [x] Each built-in loads via `skill` and via `task(load_skills=[id])` and injects its
  body content into the child prompt. **Verify:** a tool/test loads each and asserts a
  distinctive substring from each body appears in the injected content.
- [x] Disabling a built-in by stable id makes it non-loadable with a clear reason and
  shows `Disabled` in the catalog + doctor. **Verify:** a test sets the disable config,
  asserts load fails with the disabled reason, and asserts doctor JSON reports the
  disabled status.
- [x] Doctor lists all three with stable id + status + scope. **Verify:**
  `cargo run -p harness -- --config <cfg> doctor --json` (or the doctor test) shows them.
- [x] Capability map + per-skill docs exist. **Verify:** a source-derived skill-doc
  test parses the capability map and per-skill docs, then asserts every built-in skill
  stable id in the catalog has use-when and do-not-use-when entries.

---

## 7. WS4 — Provider error taxonomy & support matrix

### 7.1 Why
`OpenAiCompatibleProviderError` only models construction failures; runtime non-success
is a formatted string. The roadmap requires stable, user-actionable error categories,
a fallback policy, surfacing text, and an explicit support matrix.

### 7.2 Reference material
- `crates/harness-providers/src/openai.rs` (`format_non_success_status_message`, status
  handling around lines ~233/365/388/414), `lib.rs` (`ProviderStreamEvent`, provider
  trait), `crates/harness-providers/AGENTS.md`.
- How provider errors currently reach the user: trace from provider through coordinator
  to TUI/headless output and to `events.jsonl`.

### 7.3 Deliverables

1. **Stable error category enum.** Introduce a `ProviderErrorCategory` (or equivalent)
   with at least: `MissingCredentials`, `InvalidCredentials`, `RateLimited`,
   `ContextWindowExceeded`, `UnsupportedToolCall`, `MalformedStream`, `TransportFailure`,
   plus a catch-all `Other`. Provide a mapping from HTTP status + response body shape +
   stream-decode failures to a category. Categories must be stable, serializable, and
   carry a user-actionable message (and where relevant, a remediation hint).
2. **Surface categories to the operator.** Both headless `prompt` output and the TUI
   must render the category + actionable text (not a raw status string). The category
   must be recorded in the event log so replay and support export can show it.
3. **Runtime model fallback policy.** Define and implement (or, if intentionally
   minimal for V1, define and document) the model fallback policy: when a primary model
   call fails with a fallback-eligible category, what happens. Make the policy
   observable (an event and/or visible status). Document it.
4. **Provider support matrix doc** (`docs/provider-support.md`, new, or a section in
   `docs/config.md`): the supported execution path (OpenAI-compatible first; broader
   catalog metadata as reference unless implemented), known limits, fallback policy,
   credential expectations, and the named error categories with remediation.
5. **Fixture-backed evidence.** Add deterministic tests using a faux transport that
   returns each category's representative response (401, 429, context-overflow body,
   malformed stream bytes, transport drop, etc.) and assert the correct category +
   message. No live calls required.

### 7.4 Acceptance criteria

- [x] The category enum + mapping exists and every category has a representative test.
  **Verify:** `cargo test -p harness-providers <error category test>` passes; each
  category is asserted from a faux response; removing a mapping arm makes its test fail.
- [x] Categories are surfaced in headless and TUI output and recorded in events.
  **Verify:** a headless test asserts the rendered category/message for ≥3 categories;
  a TUI view-model test asserts the same; an event-log assertion shows the category.
- [x] Fallback policy is implemented-or-explicitly-documented and observable. **Verify:**
  a test exercises the fallback path (faux primary failure → fallback or documented
  no-op) and asserts the observable outcome; the doc states the policy.
- [x] `docs/provider-support.md` (or the config section) states execution path, limits,
  fallback, credentials, and all error categories. **Verify:** a docs-reference test
  asserts every `ProviderErrorCategory` variant name appears in the doc (so they cannot
  drift).

---

## 8. WS5 — Session, resume & compaction trust

### 8.1 Why
The roadmap requires acceptance evidence for interrupted resume, crash-resilient
writes, large-session performance, compaction preservation, bounded compaction
fallback, lineage docs, and meaningful titles.

### 8.2 Reference material
- `crates/harness-core/src/store.rs` (event store), `session_lineage.rs`,
  `session_paths.rs`, `session_title.rs`, `coord/provider_context.rs` (compaction),
  `transcript_projection.rs`, `proj.rs`. CLI: `crates/harness/src/sessions.rs`,
  `replay.rs`, `recovery.rs`. Existing perf lane: `scripts/test-lanes.sh perf`.

### 8.3 Deliverables

1. **Resume acceptance test.** Build a realistic interrupted session (multiple turns,
   a tool call with an artifact, a pending/!resolved permission, a loaded skill, some
   todos, a plan), persist it, then resume and continue. Assert the resumed session
   restores transcript, artifacts, permission state, todos, and plan, and can take the
   next turn. Document the acceptance scenario in `docs/sessions-and-replay.md`.
2. **Crash-resilient write test.** At the event-store boundary, simulate a crash:
   a partially written / truncated final JSONL line, and an interrupted write. Assert
   the store recovers (reads all complete events, tolerates/repairs the partial tail
   without losing prior events, and does not execute anything during recovery). Document
   the guarantee.
3. **Large-session performance harness + budgets.** Generate a large local session
   corpus (e.g. configurable N sessions / M events — pick sizes that are meaningful and
   record them). Measure `sessions list`, resume, and `session_search` timings; write
   artifacts (timings + provenance) under the perf lane's artifact root. Define budgets
   in WS7's budgets doc and assert the measured values are recorded (the budget *gate*
   lives in WS7; here you produce the measurement + artifact).
4. **Compaction V1 contracts + preservation tests.** Document the compaction contracts
   (threshold policy, number of retained recent turns, file/tool/skill/todo/plan
   preservation, post-compaction restoration hints) in `docs/architecture.md`. Add a
   preservation test: construct a session containing a file read, a tool call, a loaded
   skill, todos, and a plan; trigger compaction; assert the produced summary/retained
   context still references each of those five context kinds.
5. **Bounded compaction fallback.** Using a faux provider that fails summarization,
   assert the deterministic fallback path runs, emits a **user-visible status**
   (event and surfaced text), and that repeated failures are **bounded** (do not loop
   forever and do not silently erase context). Document the fallback policy.
6. **Lineage docs.** In `docs/sessions-and-replay.md`, document how summaries,
   artifacts, and restored context behave across branch/fork/clone lineage.
7. **Meaningful titles.** Ensure session list/resume surfaces (CLI + TUI) show
   meaningful generated or editable titles, not only run ids/paths. Add a test asserting
   the list/resume surfaces present a non-empty human-meaningful title for a sample
   session.

### 8.4 Acceptance criteria

- [x] Resume acceptance test passes and the scenario is documented. **Verify:**
  `cargo test -p harness <resume acceptance test>`; the test asserts restoration of
  transcript+artifacts+permission+todos+plan and a successful next turn.
- [x] Crash-write test passes and the guarantee is documented. **Verify:**
  `cargo test -p harness-core <crash write test>`; truncating the tail still reads all
  prior events; removing the recovery logic makes it fail.
- [x] Large-session perf harness runs and writes artifacts with provenance. **Verify:**
  run the perf lane; artifacts exist under the lane root with timings + corpus sizes
  recorded; the log cites the artifact path.
- [x] Compaction contracts documented; preservation test passes for all five context
  kinds. **Verify:** `cargo test -p harness-core <compaction preservation test>`;
  dropping any one preserved kind makes it fail.
- [x] Bounded fallback test passes; status is user-visible; loop is bounded. **Verify:**
  `cargo test -p harness-core <compaction fallback test>` asserts a status event,
  surfaced text, and a bounded retry count.
- [x] Lineage behavior documented. **Verify:** a sessions docs-reference test asserts
  `docs/sessions-and-replay.md` covers fork/clone summary behavior, artifact behavior,
  and source-cutoff semantics, matching the lineage implementation.
- [x] Titles shown in list/resume surfaces. **Verify:** CLI and TUI tests assert a
  non-empty meaningful title.

---

## 9. WS6 — Tool & permission depth

### 9.1 Why
Read-only subagent restrictions, schema↔prompt agreement, bash-safety guidance, and
permission-promise enforcement are required for this slice's V1 tool/permission
maturity. AST-grep replace remains roadmap-tracked but deferred to the final slice.

### 9.2 Reference material
- `crates/harness-tools/` (`native_tools.rs`, `tool_catalog.rs`, `ast_grep.rs`,
  `shell_run.rs`, `shell_safety.rs`, `workspace_edit.rs`, `mcp.rs`), parity test
  `native_tool_parity_matrix_test`. Permissions: `crates/harness-core/src/perm.rs`,
  coordinator permission resolution, `crates/harness-tools/AGENTS.md`.
- AST-grep search already exists (`ast_grep.rs`); the replace counterpart remains
  deferred until its edit-safety, dry-run/apply, permission, replay, and catalog parity
  gates are ready.

### 9.3 Deliverables

1. **Read-only subagent restriction tests.** For `explore` (and `plan`'s restricted
   posture, and any category that denies recursion), add tests that attempt `edit`,
   `bash` (write), `task` (recursive delegation), and an MCP write/tool call, and assert
   each is **denied by the coordinator** (not merely discouraged by prompt text).
2. **Permission-promise fixture.** Add a fixture proving that the runtime enforcement
   matches the prompt promises for `plan`, `explore`, `general`, and the category
   routes: for each, the set of tools the prompt claims are restricted is exactly the
   set the coordinator denies. This couples WS2 prompt claims to runtime behavior.
3. **Tool schema ↔ prompt description agreement test.** Add a test asserting that for
   every native tool, the canonical id (and any alias), permission name, and
   replay-behavior described in the tool's schema/prompt description agree with the
   registry and with `docs/native-tool-catalog.md`.
4. **`docs/native-tool-catalog.md` reference test.** Enforce that the catalog doc lists
   every registered native tool id with its permission and replay behavior (no drift).
5. **Bash-safety guidance.** State the bash timeout, output cap, and blocked-command
   policy in **both** `docs/native-tool-catalog.md` (or a bash section) **and** the
   relevant agent prompt guidance (WS2 bodies / shared prompt module). Add a test that
   the documented numbers match the runtime constants in `shell_run.rs`/`shell_safety.rs`.
6. **AST-grep replace deferral honesty.** Do **not** implement `ast_grep_replace` in
   this slice. Ensure `docs/native-tool-catalog.md`, doctor/readiness output, and the
   claim-evidence matrix agree that `ast_grep_search` ships now and `ast_grep_replace`
   remains final-slice work. Do not add a registry entry, parity row, or docs claim that
   advertises replace as shipped.

### 9.4 Acceptance criteria

- [x] Read-only restriction tests pass for edit/bash/task/MCP on the restricted
  profiles. **Verify:** `cargo test <restriction test>`; flipping a profile to allow one
  of these makes the corresponding assertion fail.
- [x] Permission-promise fixture passes for plan/explore/general/categories. **Verify:**
  the fixture computes prompt-claimed restrictions and coordinator-denied tools and
  asserts equality.
- [x] Schema↔prompt↔catalog agreement test passes. **Verify:** `cargo test -p
  harness-tools <agreement test>`; renaming a tool id in one place makes it fail.
- [x] `docs/native-tool-catalog.md` matches the registry. **Verify:** the catalog
  reference test passes; adding a tool without updating the doc fails it.
- [x] Bash timeout/output-cap/blocked-command stated in docs + prompt and matches
  runtime constants. **Verify:** the constants-match test passes and also asserts the
  composed prompts/docs contain the runtime timeout, output-cap, and blocked-command
  values.
- [x] AST-grep replace remains honestly deferred. **Verify:** native-tool catalog,
  doctor/readiness output, parity tests, and the claim-evidence matrix all agree that
  `ast_grep_replace` is not shipped in this slice and remains final-slice work; adding a
  shipped claim without a registry/tool implementation makes the drift test fail.

---

## 10. WS7 — Release evidence & claim integrity

### 10.1 Why
The roadmap requires a claim-to-evidence matrix, a release-blocker taxonomy, budgets,
faux-provider defaults, doctor extension-readiness, and outside-repo smoke that proves
more than `doctor`.

### 10.2 Deliverables

1. **Claim-to-evidence matrix** (`docs/claim-evidence-matrix.md`, new). A table mapping
   every release-facing claim in README + `docs/` (speed, provider support,
   compatibility, parity, durability, tool surface, etc.) to one of: a deterministic
   test (named), a manual/PTY artifact (path), a fixture, command output, or an explicit
   documented limitation. Each row must include requirement/claim text, evidence type,
   machine-resolvable evidence pointer, verification command or lane, observed result,
   timestamp/provenance, and pass/fail status. Empty evidence, stale evidence,
   unresolved links, or unverifiable pointers fail the gate. Any claim without backing
   must be softened in README/docs or gain backing. Add a test that fails if README
   contains a release-claim phrase from a maintained list that is not present in the
   matrix (keep the phrase list small and real; this is a drift guard, not a debt
   baseline).
2. **Release-blocker taxonomy** (`docs/release-blockers.md`, new). Classify open V1 work
   as correctness / safety / UX / docs / provider / performance / evidence, and state
   which checks are **release blockers** vs **local dev aids**. Map the blocker lanes to
   `scripts/test-lanes.sh` modes.
3. **Budgets** (`docs/budgets.md`, new, or a section). Define startup/readiness time,
   TUI render time, session resume time, and binary size budgets. Wire a budget check
   into the `perf` lane that reads the measured artifacts (from WS5 + a startup/binary
   measurement) and **fails closed** if an artifact is missing or stale. The gate must
   check current artifacts, not a frozen baseline.
4. **Faux-provider defaults.** Audit prompt/permission/compaction/built-in tests; ensure
   they use mock/faux providers by default and that no deterministic lane requires a live
   provider. Add a guard (extend the quality-gates static checks) that flags a
   deterministic test importing a live transport. Record any pre-existing live-coupled
   deterministic test you fix.
5. **Doctor extension/roadmap readiness section.** Add a doctor check that reports
   extension/roadmap readiness (e.g., which built-in capabilities are present/disabled,
   which planned seams are post-V1) **separately** from runtime health, so a green
   runtime report is not confused with full roadmap completion.
6. **Outside-repo smoke.** Extend `signoff-binary` (or add a lane) so the outside-repo
   smoke runs, in addition to `--help`/`--version`/`config validate`/`doctor`: one
   **TUI startup** (headless/PTY) and one **tool-enabled prompt** (mock provider, a tool
   call that edits or reads a file), proving more than config preflight. Write artifacts.

### 10.3 Acceptance criteria

- [x] `docs/claim-evidence-matrix.md` exists; every maintained release-claim phrase maps
  to backing or a documented limitation; the drift test passes. **Verify:** run the
  drift test; adding an unbacked claim phrase to README fails it.
- [x] `docs/release-blockers.md` exists with the taxonomy and blocker-vs-aid split
  mapped to lanes. **Verify:** a release-blocker docs test parses the taxonomy,
  asserts each blocker category maps to at least one lane, and asserts every lane name
  exists in `scripts/test-lanes.sh`.
- [x] `docs/budgets.md` exists; the perf lane budget check fails closed on missing/stale
  artifacts and passes on fresh ones. **Verify:** run `scripts/test-lanes.sh perf`;
  delete an artifact and confirm the gate fails; regenerate and confirm it passes.
- [x] No deterministic lane requires a live provider; the static guard flags live
  coupling. **Verify:** `scripts/test-lanes.sh quality-gates` passes; introduce a live
  import into a deterministic test and confirm the guard flags it.
- [x] Doctor reports an extension/roadmap-readiness section distinct from runtime
  health. **Verify:** `doctor --json` contains the new section; a doctor test asserts it.
- [x] Outside-repo smoke runs TUI startup + a tool-enabled mock prompt and writes
  artifacts. **Verify:** `scripts/test-lanes.sh signoff-binary` (with its env gate)
  produces the TUI-startup and tool-prompt stage artifacts.

---

## 11. WS8 — TUI operator polish

### 11.1 Why
High-frequency operator surfaces need durability and clarity, with deterministic
coverage. The roadmap lists prompt history, permission overlays, model switching,
session search, diff hunk nav, keyboard nav, configurable keybindings, and flow
coverage.

### 11.2 Reference material
- `crates/harness-tui/src/app.rs` (`prompt_history`, overlays, session history),
  `app/permissions.rs`, `app/session_navigation.rs`, `ui_diff.rs`, `ui_overlays.rs`,
  `keybindings.rs`, `view_model.rs`, `render_test.rs`, `crates/harness-tui/AGENTS.md`.
- Existing TUI tests: `crates/harness-tui/src/tests.rs`, `lib_tests.rs`,
  `app/tests.rs`, snapshot dirs, and `crates/harness-tui/tests/`. PTY:
  `crates/harness-testkit/tests/pty_e2e.rs` and `crates/harness-tui/tests/pty_e2e.rs`.

### 11.3 Deliverables (each must have deterministic coverage — view-model test and/or
TestBackend snapshot and/or PTY)

1. **Durable prompt history.** Persist prompt history to disk (a documented file under
   the TUI/session data dir) and load it on startup so history survives across sessions.
   Navigation (up/down) must preserve the in-progress draft and cursor intent (restore
   the draft when leaving history).
2. **Permission overlay clarity.** The permission overlay must show the available
   shortcuts, the scope of the decision (one-shot vs session), and timeout/countdown
   state where applicable.
3. **Model switching.** The model switcher must present provider-grouped, searchable
   entries and show visible fallback/error status (tying into WS4 categories).
4. **Session search.** Session search/picker must support visible fielded or fuzzy
   filtering.
5. **Diff hunk navigation.** Diff review must support next/previous hunk navigation.
6. **Keyboard navigation.** Subagent/background work and the session tree/sidebar must
   be keyboard-navigable (keyboard-first), not pointer-dependent.
7. **Configurable keybindings.** Any new keybinding must be registered through the
   configurable keybinding defaults (`keybindings.rs`), not hardcoded scattered key
   checks. Add a registry-derived static guard that fails on new hardcoded key
   comparisons outside the keybinding registry for the surfaces you touch.
8. **Flow coverage + happy-path recording.** Provide deterministic PTY or snapshot
   coverage for: approve/deny permission, diff review, resume, and replay-failure
   states. Produce a **scripted deterministic PTY happy-path recording** artifact covering
   start, prompt, permission, tool call, edit, resume, and quit. This is the accepted
   evidence for the roadmap's manual TUI happy-path blocker in this slice; native visual
   PNG signoff remains final-slice work.

### 11.4 Acceptance criteria

- [x] Prompt history persists across process restarts and draft/cursor are preserved.
  **Verify:** a test writes history, restarts the app model, and asserts history loads;
  a navigation test asserts the draft is restored after browsing history.
- [x] Permission overlay shows shortcuts + scope + timeout. **Verify:** a view-model/
  snapshot test asserts all three are rendered.
- [x] Model switcher is provider-grouped, searchable, with fallback/error status.
  **Verify:** a view-model test asserts grouping, filtering, and a rendered fallback
  state.
- [x] Session search filters (fielded/fuzzy). **Verify:** a test asserts filtering
  narrows results for a query.
- [x] Diff next/previous hunk navigation works. **Verify:** a test asserts the selected
  hunk advances/retreats.
- [x] Background/subagent and session tree are keyboard-navigable. **Verify:** tests
  drive keyboard events and assert focus/selection movement.
- [x] New keybindings go through the registry; the hardcoded-key guard passes. **Verify:**
  the registry-derived static guard walks touched TUI surfaces and fails for key checks
  not declared in the keybinding registry.
- [x] Deterministic flow coverage exists for approve/deny, diff, resume, replay-failure;
  a PTY happy-path recording artifact is produced. **Verify:** `scripts/test-lanes.sh
  signoff-pty` passes; the happy-path artifact exists with lane name, artifact path,
  command, timestamp, and env/provenance summary.

---

## 12. WS9 — Task delegation contract

### 12.1 Why
The roadmap requires a structured delegation body, capped/structured child summaries to
keep parent context lean, and a delegation fixture.

### 12.2 Reference material
- `crates/harness-tools/src/agent_ops.rs` (task tool), `control_plane.rs`,
  coordinator task scheduling and lineage (`coord.rs`, `session_lineage.rs`),
  `README.md` `task` section, the reference delegation body shape under `inspirations/`.

### 12.3 Deliverables

1. **Structured delegation body.** The `task` tool contract must recommend or enforce a
   structured delegation body with the fields: context, goal, downstream use, request,
   required tools, must-do, must-not-do. Update the tool description/schema and document
   it (README `task` section + `docs/agents-and-subagents.md`). If enforced, malformed
   bodies must produce a clear error; if recommended, the description must include the
   template.
2. **Capped, structured child summaries.** Child task results returned to the parent
   must be summarized/capped so parent context stays lean (cap length, prefer a
   structured result over raw transcript). Existing behavior already returns compact
   loaded-skill metadata and `next_actions`; extend to cap the child's substantive
   output with a documented policy.
3. **Delegation fixture.** Add a fixture proving, in one deterministic test: loaded
   skill content reaches the child prompt, the category prompt append is applied,
   parent/child lineage is recorded, both sync and background execution paths behave per
   contract, and the returned child summary is capped/structured.

### 12.4 Acceptance criteria

- [x] Structured delegation body is in the tool contract + docs. **Verify:** the tool
  schema/description includes the seven fields; docs match; a schema test asserts their
  presence (and, if enforced, a malformed-body test asserts the error).
- [x] Child summaries are capped/structured per a documented policy. **Verify:** a test
  delegates a child that produces large output and asserts the parent-visible summary is
  within the cap and structured.
- [x] Delegation fixture passes covering skill content, category append, lineage,
  sync/background, and summary capping. **Verify:** `cargo test <delegation fixture>`;
  removing the skill-injection or the cap makes the corresponding assertion fail.

---

## 13. WS10 — CLI surface audit & first-run

### 13.1 Why
The roadmap requires CLI help reviewed as a complete V1 surface, command names audited
against the README quick start, and first-run provider/auth guidance that does not
assume the local loopback provider exists.

### 13.2 Reference material
- `crates/harness/src/lib.rs` and the CLI command modules (`prompt.rs`, `run.rs`,
  `replay.rs`, `sessions.rs`, `models.rs`, `doctor.rs`, `scenarios.rs`,
  `cli_config.rs`, `cli_labels.rs`). README quick start. Existing CLI tests under
  `crates/harness/tests/` (`binary_smoke.rs`, `*_cli_test.rs`).

### 13.3 Deliverables

1. **Help completeness.** Every CLI subcommand (and notable flags) must have meaningful
   help text. Add a test that enumerates the command tree and asserts each command has
   non-empty, non-placeholder help (no `TODO`, no empty about).
2. **Command ↔ README audit.** Add a test that extracts the `harness ...` commands shown
   in README quick start (and `docs/config.md` where commands appear) and asserts each
   resolves to a real subcommand, and conversely that the README does not reference
   removed commands.
3. **First-run provider/auth docs.** Update the README/first-run docs so provider/auth
   setup is explained **without assuming the local loopback provider already exists**:
   how to point at a real OpenAI-compatible endpoint, set credentials (env/config),
   and verify with `doctor` + a live `prompt`. Keep the existing mock/loopback path as
   one option, not the only assumed one.

### 13.4 Acceptance criteria

- [x] Help-completeness test passes. **Verify:** `cargo test -p harness <help
  completeness test>`; emptying one command's about makes it fail.
- [x] Command↔README audit test passes. **Verify:** adding a fake `harness frobnicate`
  to README fails it; removing a real command from README that the test expects fails it.
- [x] First-run provider/auth docs do not assume loopback. **Verify:** a docs-reference
  check asserts the provider/credential setup section exists and every documented config
  key resolves against the public config schema/example files.

---

## 14. Cross-cutting deliverable: roadmap reconciliation

- [x] After all workstreams pass, update `docs/roadmap-v1.md`: flip exactly the boxes in
  §3.2 to `[x]`, and only those whose acceptance criteria + verification actually
  passed (re-derived from source per §16, not assumed). Do not flip deferred boxes
  (§3.3).
- [x] Write a roadmap reconciliation table in the progress log with one row per roadmap
  checkbox considered: exact checkbox text, WS evidence, verify command or artifact,
  observed result, flipped yes/no, and if excluded, the §3.3 exclusion category.
- [x] Repair the stale `docs/v1-agent-catalog-workspace-intelligence-prd.md` reference
  in `docs/roadmap-v1.md` (WS1.7) and any other dangling links.
- [x] Record the recomputed pre-V1 completion percentage in the progress log (§16.3).

---

## 15. Final Verification Suite (must all pass; do not stop until green)

Run from the repo root on the Linux box. Capture artifacts via the lane runner where
applicable. None of these may be skipped, `#[ignore]`-d, or weakened to pass.

1. `cargo fmt --all -- --check`
2. `cargo check --workspace`
3. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
4. `cargo test --workspace --all-features` (or document any live-only exclusion)
5. `scripts/test-lanes.sh fast`
6. `scripts/test-lanes.sh integration`
7. `scripts/test-lanes.sh quality-gates`
8. `scripts/test-lanes.sh simulation`
9. `scripts/test-lanes.sh perf` (budgets gate must pass against fresh artifacts)
10. `RUST_TEST_THREADS=1 scripts/test-lanes.sh signoff-pty`
11. `scripts/test-lanes.sh signoff-binary` (with `HARNESS_BINARY_SMOKE=1`; includes the
    new TUI-startup + tool-enabled-prompt stages)
12. `cargo run -p harness -- --config configs/harness.example.jsonc config validate`
13. `cargo run -p harness -- --config configs/harness.example.jsonc doctor` (and
    `doctor --json` shows the new extension-readiness + built-in skill sections)
14. All new docs-reference / referential-integrity tests (architecture↔events,
    permissions, native-tool-catalog↔registry including deferred-tool honesty,
    provider-categories↔doc, claim-evidence drift, command↔README, docs-link-integrity,
    progress-log evidence rows) pass.

If any command fails, fix the root cause — do not bypass, ignore, or delete the failing
check.

---

## 16. Final Self-Audit (re-derive from source — do not trust checkboxes)

The user explicitly distrusts self-reported "done." Before declaring the slice
complete, perform and record this audit. It must be re-derived from the actual tree,
not copied from this PRD.

### 16.1 Anti-stub sweep
- [x] `grep -rn "#\[ignore\]" crates/*/tests crates/*/src` — every match is a
  pre-existing, intentionally env-gated signoff test (PTY/live/native). No new
  `#[ignore]` was added to dodge a gate. List every match and classify it.
- [x] No new `assert!(true)`, empty test bodies, or tests that pass independent of the
  feature. For each new test, the log has a "breaks if:" line.
- [x] No new baseline/whitelist/allowlist file grandfathers existing debt. (If you added
  any JSON "gate" file, it checks current state.)
- [x] No existing test/snapshot/fixture was deleted or weakened except as justified in
  §16.4.
- [x] Net additions correspond to exercised behavior (spot-check: pick 5 new public code
  paths and confirm a test fails if you revert them).

### 16.2 Referential integrity
- [x] All new skill/prompt/doc content references only real tools/agents/categories/
  config keys/events/paths (the referential-integrity tests prove this).
- [x] All new docs that mention config keys, tool ids, event variants, or commands are
  covered by a reference test that fails on drift.

### 16.3 Roadmap accounting
- [x] Recount `docs/roadmap-v1.md` checkboxes from source, not from this PRD. Raw `grep`
  counts may be recorded as scouting output, but the final numerator/denominator must
  come from a source-derived roadmap parser or an explicit reconciliation table.
- [x] Build the excluded set by matching only the fixed exclusion groups in §3.3. Do not
  subtract any other unchecked item because it feels large, risky, subjective, or
  inconvenient.
- [x] Record four numbers in the progress log: total roadmap boxes, excluded final-slice
  or post-V1 boxes, included denominator, and checked boxes inside the included
  denominator.
- [x] Compute the included pre-V1 completion percentage as `checked boxes inside included
  denominator / included denominator * 100`.
- [x] Confirm the percentage is **≥80%**. Target range is **85–90%**. If it is below 80%,
  the slice is not done. Do not rescue the percentage by adding exclusions; deepen the
  in-scope workstreams until included boxes are honestly complete.
- [x] List every excluded roadmap checkbox by exact text under an "Excluded from this
  slice denominator" heading in the progress log, grouped by the §3.3 exclusion category
  that justified it.

### 16.4 Honest limitations
- [x] Write, in the progress log, every place where the implementation is partial, a
  test is weaker than ideal, a budget number is provisional, or a behavior is
  documented-rather-than-enforced. Honesty here is required, not penalized.

---

## 17. Out of scope (do not build; do not churn here)

- The typed extension manifest seam and the command/hook seam (final slice). Writing the
  *strategy guide* describing them as final-slice/post-V1 work is in scope (WS1);
  implementing them is not.
- AST-grep replace as a first-class native tool, including structural edit safety,
  dry-run/apply behavior, permission gating, replay-safe artifacts, parity matrix
  coverage, and catalog docs.
- TUI visual signoff against `inspirations/` PNGs and the native-visual lane (final slice).
- External compatibility skill-root adapters (`.external-editor`/`.assistant`/`.agents`).
- Any item under roadmap "Post-V1 direction," "Explicitly post-V1 unless re-scoped," or
- New providers beyond the OpenAI-compatible path; OAuth/credential-store; auto-update;
  server/share/enterprise; desktop/mobile/web clients; browser/media automation.
- Do not edit git state, do not push, do not touch anything outside the repo tree.

If you exhaust the in-scope acceptance criteria before §15/§16 are green, return to the
deepest still-improvable in-scope workstream (more prompt golden coverage, more provider
category fixtures, more compaction/resume edge cases, more TUI flow snapshots, broader
docs reference tests) rather than starting out-of-scope work.

---

## 18. Progress log (required, append-only)

Maintain a progress log at `docs/v1-release-readiness-slice-progress.md` (create it).
Append a dated entry per work session. Each evidence row must include:
- Requirement id or roadmap checkbox text.
- Workstream id and changed files.
- Evidence type (test, lane artifact, fixture, command output, docs-reference check,
  manual/PTY artifact, or documented limitation).
- Verification command or lane and observed pass/fail result.
- Machine-resolvable artifact path, test name, or command-output location.
- Timestamp/provenance and any relevant environment gate.
- For every new test: a one-line "breaks if:" statement.
- Any deviation from this PRD and why.
- Honest limitations (§16.4).
- The current re-derived roadmap percentage (§16.3).

A final progress-log audit must confirm that every checked PRD box and every flipped
roadmap checkbox has at least one matching evidence row. Empty evidence, stale evidence,
unresolved links, or unverifiable pointers fail the gate.

Do **not** mark this PRD's checkboxes or the roadmap's boxes from intention — only from a
run `Verify:` command whose stated observable result was produced. When everything in
§4–§14 is `[x]` with evidence and §15/§16 are green, write a final entry stating the
slice is complete and the measured pre-V1 percentage.
