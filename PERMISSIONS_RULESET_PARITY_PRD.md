# Harness Permissions, Agents, and Tool-Surface Parity PRD

**Status:** Complete (2026-07-16). Evidence and §12 certificate live in
`docs/permissions-ruleset-parity-progress.md`.

**Date:** 2026-07-16

**Audience:** An implementer agent that will run in a loop until completion.
Human operators reviewing evidence.

**Authority (highest wins on conflict):**

1. Root [`AGENTS.md`](./AGENTS.md) and crate-scoped `AGENTS.md` files
2. Runtime invariants in `crates/harness-core` (events, coordinator, permissions,
   replay, redaction)
3. This PRD
4. Related docs (`docs/permissions.md`, `docs/native-tool-catalog.md`,
   `docs/tools-parity-prd.md`) — update them when behavior changes;
   do not use them to claim work is done without code + tests

**Primary Harness source tree (read-only):**

- `inspirations/` — authoritative reference for this PRD
- Secondary snapshot if needed: `inspirations/shuvcode/` (prefer
  `inspirations/harness` when both exist)

**Progress ledger (create on first phase):**

- `docs/permissions-ruleset-parity-progress.md`

---

## 0. Governing objective

Make Harness **tools, agents, and permissions behave like Harness** for every
implemented native tool and shipped agent profile, so that models trained on
Harness tool-use patterns do not thrash, call hidden tools, or hit opaque
shell bans.

### 0.1 Exact meaning of “exactly like Harness”

For this PRD, **exactly like Harness** means observable parity of:

| Surface | Must match Harness |
|---------|---------------------|
| **Config shape** | `permission` allow/ask/deny scalars, per-tool keys, pattern maps; agent-level permission overlays; legacy `tools: { name: false }` maps to deny |
| **Evaluation** | Last-matching rule wins; default when no rule matches is **ask** (unless Harness source for that path says otherwise — verify) |
| **Model-visible tools** | Tools with catch-all `deny` (`pattern: "*"` + `action: "deny"` for that permission) are **omitted** from the provider tool list |
| **Partial denies** | Path/command-specific allow under a broader deny still **shows** the tool (e.g. Plan `edit`) |
| **Agent defaults** | Shipped `build`, `plan`, `explore`, `general`, compaction/title/summary-style agents match Harness default permission rulesets and tool visibility |
| **Task tool** | Denied subagent types are removed from the task tool description; task permission patterns apply |
| **Bash** | Real-shell ergonomics: globs, `/dev/null` redirects, pipes, common git/du/find idioms work under permission rules — not hard-blocked as “path escapes workspace” / “use the glob tool” unless Harness also blocks them |
| **External paths** | Out-of-project paths use an **external_directory**-style ask/deny flow (or documented equivalent), not a silent hard fail that models cannot learn |
| **Tool schemas & descriptions** | Names, required fields, and description guidance for overlapping tools match Harness closely enough that ruleset-style tool calls succeed |
| **Child agents** | Subagent permission derivation matches Harness (`deriveSubagentSessionPermission` + child tool denies for task/todowrite where applicable) |

“Exactly like” does **not** mean:

- Copy TypeScript into Rust
- Copy Harness branding, UI package layout, or SQLite schema wholesale
- Break Harness event-sourcing, coordinator authority, or redaction invariants

When Harness behavior and a Harness invariant conflict, implement the Harness
**observable** behavior in the Harness architecture, record the adaptation in
the progress ledger, and keep the invariant.

### 0.2 Non-goals

- Do not edit anything under `inspirations/` (read-only).
- Do not claim completion by updating docs alone.
- Do not bypass coordinator permission checks.
- Do not delete, skip, or weaken tests to make parity “pass”.
- Do not implement Harness HTTP API, plugins, or desktop product features
  unless required for permission/tool-surface parity.
- Do not rename Harness canonical tool IDs in public config without aliases
  that preserve model-facing provider function names where needed.
- Do not expand scope into full tool-behavior parity already covered by
  `docs/tools-parity-prd.md` except where permissions, visibility,
  schemas, bash safety, or agent defaults require it.

---

## 1. Why this exists (problem statement)

Current Harness diverges from Harness in ways that cause agents to misbehave:

1. **Deny does not hide tools.** Harness `Permission.disabled` /
   `visibleTools` strips catch-all denied tools from the model request.
   Harness only filters `AgentProfile.toolset` and still enforces deny at
   execution — models call forbidden tools and thrash.

2. **Dual config surfaces.** Harness splits `tools: [...]` and
   `permission: {...}`. Harness uses permission (plus legacy tools map) as
   the primary control plane.

3. **Bash hard-safety ≠ Harness.** Harness rejects shell globs and
   `/dev/null` as workspace escapes. Harness runs a real shell and asks
   `bash` / `external_directory` permissions instead. Agents retry until
   timeout.

4. **Agent defaults differ.** e.g. Harness explore allows bash/webfetch under
   a `*`:deny + allow-list; Harness explore is stricter and/or inconsistent
   between toolset and permission posture.

5. **Task description does not filter denied agents** the way Harness’s
   `ToolRegistry.describeTask` does.

This PRD exists to close those gaps with mechanical, test-backed proof.

---

## 2. Harness source anchors (must re-read every phase)

Before implementing a phase, re-read the live files under
`inspirations/` (paths may shift; search if moved). Minimum anchors:

| Topic | Expected location (verify) |
|-------|----------------------------|
| Permission evaluate / ask / disabled / visibleTools | `packages/src/permission/index.ts` |
| Permission config schema | `packages/core/src/v1/config/permission.ts` |
| Permission V1 types | `packages/core/src/v1/permission.ts` |
| Agent defaults (build/plan/explore/general/…) | `packages/src/agent/agent.ts` |
| Subagent permission derivation | `packages/src/agent/subagent-permissions.ts` |
| LLM tool filtering | `packages/src/session/llm/request.ts` (`resolveTools`) |
| Tool registry + task description filter | `packages/src/tool/registry.ts` |
| Bash/shell tool + permission patterns | `packages/src/tool/shell.ts`, `.../shell/prompt.ts` |
| Task tool + child denies | `packages/src/tool/task.ts` |
| Public docs (secondary) | https://harness.ai/docs/permissions/ |

**Rule:** If this PRD disagrees with current `inspirations/harness` source,
**Harness source wins**. Update the progress ledger and this PRD’s evidence
note before coding.

Record in the progress ledger for each phase:

```text
Harness reference:
  tree: inspirations/harness
  git: <sha or "untracked snapshot" + date>
  files: <paths>
  summary: <what behavior was observed>
```

---

## 3. Harness targets (primary edit surface)

| Area | Paths |
|------|--------|
| Permission policy | `crates/harness-core/src/perm.rs`, `perm/shell.rs` |
| Coordinator gates | `crates/harness-core/src/coord/permission.rs`, `tool_execution.rs` |
| Provider tool list | `crates/harness-core/src/agent/provider_boundary.rs` |
| Agent profiles / defaults | `crates/harness-core/src/config/public/agents.rs`, config public types |
| Shell safety | `crates/harness-tools/src/shell_safety.rs`, `shell_safety/path_validation.rs`, shell runner |
| Native tools / schemas / descriptions | `crates/harness-tools/src/**` |
| Catalog / doctor | `crates/harness-tools/src/tool_catalog.rs`, harness doctor if needed |
| Docs | `docs/permissions.md`, `docs/config.md`, `docs/native-tool-catalog.md`, `docs/agents-and-subagents.md` |
| Tests | `crates/harness-core/tests/**`, `crates/harness-tools/tests/**`, bootstrap profile tests |

---

## 4. Operating rules for the implementer loop

### 4.1 Loop protocol (mandatory)

You are in a **completion loop**. After every unit of work:

1. Update `docs/permissions-ruleset-parity-progress.md`
2. Run the phase’s acceptance commands
3. If any acceptance item fails → fix or reduce scope with a **dated decision
   record**, never mark the phase complete
4. Only then advance to the next phase
5. Only when **all** §11 global gates and §12 certificate requirements pass
   may you stop

**Never** stop because:

- “Most of it works”
- “Docs are updated”
- “Tests that used to assert the old behavior were removed”
- “Parity is close enough”
- Context is long / you are tired of the task

### 4.2 Skills and project rules

Before any code edit:

1. Read root + relevant crate `AGENTS.md`
2. Load skills: `karpathy-guidelines`, `programming` (and Rust skills if
   editing Rust)
3. Prefer surgical changes; no drive-by refactors

### 4.3 TDD

For every behavioral change:

1. Write a **failing** test that encodes Harness-observed behavior
2. Implement the minimal change
3. Prove the test passes
4. Run the broader lane commands for that phase

Forbidden: changing tests to match incorrect Harness behavior and calling it
parity.

### 4.4 Evidence discipline

Each phase entry in the progress ledger must include:

| Field | Required |
|-------|----------|
| Phase id | e.g. `P0-visibility` |
| Status | `not_started` \| `in_progress` \| `blocked` \| `complete` |
| Harness files + sha | yes |
| Harness files changed | yes |
| Tests added/updated | exact `cargo nextest` filters |
| Commands run + exit codes | paste or path to log |
| Dogfood scenario | concrete command + outcome |
| Known divergences | explicit list or `none` |
| Completion claim | only if all phase gates green |

### 4.5 Forbidden completion phrases

The agent **must not** use these without the §12 certificate fully filled:

- “Done”, “Complete”, “Parity achieved”, “Ship it”, “All set”
- “LGTM”, “Should be fine”, “Good enough”
- “Remaining work is minor / cosmetic / docs-only”

If any §11 checkbox is open, the only allowed status is `in_progress` or
`blocked` with a concrete next action.

### 4.6 Premature-completion tripwires (auto-fail)

Any of the following **invalidates** a completion claim:

1. Progress ledger missing or phases marked complete without command evidence
2. Catch-all deny tools still appear in `build_provider_tool_defs` for explore
3. Bash still hard-fails `2>/dev/null` or `find … -name '*.rs'` when Harness
   would allow/ask
4. Tests that encoded old Harness-only bans were deleted without replacement
   Harness-aligned tests
5. Docs claim Harness parity while tests fail
6. `inspirations/` was modified
7. Coordinator permission path skipped for “ergonomics”
8. Global `scripts/test-lanes.sh fast` or quality-gates not run after final
   phase
9. Explore/plan/build default permission postures not covered by automated
   tests
10. Task description still lists agents whose task permission is deny

---

## 5. Target architecture (Harness adaptation of Harness)

### 5.1 Single control plane: permission rules

Implement an ruleset-compatible ruleset model:

```text
Rule = { permission: string|wildcard, pattern: string, action: allow|ask|deny }
```

- Merge order: defaults → agent profile → session overrides (if any) →
  user config (match Harness merge order after re-reading source)
- Evaluate with **last match wins**
- Default action when no match: **ask** (confirm against Harness)

Config compatibility:

- Keep public V1 names (`bash`, `edit`, `question`, `task`, `webfetch`,
  `websearch`, `codesearch`, `lsp`)
- Add Harness keys as needed for parity: at minimum `read`,
  `external_directory`, `doom_loop`, `skill`, `todowrite` (and any others
  present in Harness config schema) — map onto Harness kinds or first-class
  rules without breaking existing configs
- Support pattern maps for bash/edit/task/read as Harness does
- Legacy `tools: { "bash": false }` → deny rule for that tool

### 5.2 Model-visible tool filtering

Port Harness:

```ts
// disabled: last rule for tool's permission has pattern "*" and action "deny"
// visibleTools: exclude disabled
```

Apply in `build_provider_tool_defs*` **and** any other path that exports tools
to a provider.

Tool → permission name mapping must match Harness’s `disabled()` logic
(edit/write/apply_patch → `edit`; MCP resource tools → `read`; else tool id).

### 5.3 Agent defaults

Re-derive shipped agents from Harness `agent.ts` defaults (re-read source):

| Agent | Must match Harness defaults for |
|-------|----------------------------------|
| build | permission overlay + tool visibility |
| plan | edit path allows for plan files; task.general deny; plan_exit allow |
| explore | `*`:deny then allow read/search/bash/webfetch/websearch (per source) |
| general | defaults + todowrite deny (per source) |
| title/summary/compaction | effectively no tools (`*`:deny) |

If Harness renames plan paths (`.agent-harness/plans` vs `.harness/plans`),
preserve **behavior** (plan file writable; rest denied) with Harness paths.

### 5.4 Task / child agents

Match Harness task tool:

- Permission check on `task` with patterns = subagent type
- Child session permission = `deriveSubagentSessionPermission(...)` equivalent
- Inject deny for `task` / `todowrite` on children when parent/subagent rules
  require it (per Harness `task.ts`)
- Task tool description lists only agents where
  `evaluate("task", name, rules) !== deny`

### 5.5 Bash / shell

Match Harness shell tool ergonomics:

1. Parse command; extract bash permission patterns + external directory paths
2. Ask/deny via permission system (not silent hard fail for normal idioms)
3. Allow shell globs and `/dev/null` (and other Harness-allowed redirects)
4. Keep truly dangerous constructs only if Harness also blocks them — verify
   against source; if Harness allows, Harness allows (under permission)
5. Bash tool description should track Harness shell prompt guidance
   (workdir, timeout, chaining) adapted to Harness naming

Workspace safety may remain as a **last-line** check for true escapes outside
workspace **and** outside Harness’s external_directory allow/ask flow — but
must not reject in-workspace globs or `/dev/null`.

### 5.6 Tool schemas and descriptions

For every overlapping tool that is implemented in Harness:

- Parameter names and requiredness match Harness
- Description text is Harness-equivalent in constraints and next-action
  guidance (reword only for Harness product names)
- Provider function names remain stable where tests require

Coordinate with `docs/tools-parity-prd.md` but **this PRD owns**
permission, visibility, agent defaults, bash permission ergonomics, and
task-description filtering.

---

## 6. Phased delivery

Execute phases **in order**. A phase is incomplete until its gates pass.

### Phase P0 — Inventory & golden fixtures

**Goal:** Freeze Harness behavior as executable fixtures.

**Work:**

1. Script or test helper that, from documented Harness source snippets /
   hand-derived tables, records:
   - default agent permission rulesets (build/plan/explore/general/…)
   - `disabled(tools, ruleset)` outcomes for each agent’s full tool id list
   - bash cases: allow/ask/deny/external for representative commands
2. Commit golden JSON under e.g.
   `crates/harness-core/tests/fixtures/permission_ruleset_parity/`
3. Progress ledger created with all phases `not_started` except P0

**Acceptance (all required):**

- [ ] Golden fixtures exist and are loaded by at least one test
- [ ] Each fixture cites Harness file paths + content hash or line range
- [ ] `cargo nextest` filter for the inventory test passes
- [ ] Progress ledger P0 = `complete` with command evidence

### Phase P1 — Permission ruleset + evaluate parity

**Goal:** Config + evaluate match Harness semantics.

**Work:**

1. Implement/adjust ruleset types, `from_config`, merge, evaluate
2. Pattern matching parity with Harness wildcards
3. Public config docs + schema examples for ruleset-compatible shapes
4. Backward compatibility: existing Harness configs still load; map into
   rulesets

**Acceptance:**

- [ ] Unit tests: last-match-wins, default ask, pattern maps, merge order
- [ ] Fixtures from P0 evaluate identically in Harness evaluate function
- [ ] `cargo nextest run -p harness-core` permission-related tests pass
- [ ] Config load tests for scalar / map / selector forms pass
- [ ] Docs: `docs/permissions.md` + `docs/config.md` updated
- [ ] Progress ledger P1 complete with evidence

### Phase P2 — Deny hides tools (model visibility)

**Goal:** `build_provider_tool_defs` matches Harness `visibleTools`.

**Work:**

1. Implement `permission_disabled_tools(ruleset, tool_ids)` equivalent to
   Harness `disabled`
2. Filter provider tool list
3. Tests per agent: explore has no edit/write/task if denied; plan still has
   edit if partial allow; compaction/title have empty or noop tools as
   Harness

**Acceptance:**

- [ ] Automated assertion: for each shipped agent, model-visible tool set
      equals Harness disabled-filter outcome (from fixtures)
- [ ] Explore cannot receive `edit`/`write`/`apply_patch` in provider defs
      under default rules
- [ ] Plan still receives `edit` when plan-path allow exists
- [ ] Regression: worker toolset membership still enforced
- [ ] Progress ledger P2 complete

### Phase P3 — Agent default permissions & toolsets

**Goal:** Shipped agents match Harness defaults.

**Work:**

1. Rewrite `default_shipped_agents` / public agent defaults from Harness
2. Align explore with Harness (including bash/webfetch if Harness allows)
3. Align plan edit rules to plan directory (Harness path)
4. Bootstrap / doctor catalog reflects new posture
5. Update `.agent-harness/agents/*.md` runtime text so it does not contradict
   runtime policy

**Acceptance:**

- [ ] Bootstrap profile tests assert permission posture + tool visibility
- [ ] Doctor/catalog permission_posture fields match runtime
- [ ] Prompt assets do not claim tools that are hidden
- [ ] Progress ledger P3 complete

### Phase P4 — Task tool + child permission derivation

**Goal:** Task delegation matches Harness.

**Work:**

1. Filter task description agent list by permission
2. Child permission derivation + default child denies (task/todowrite/etc.)
3. Tests for plan→explore only (if Harness/Harness policy requires),
   category task deny, resume path

**Acceptance:**

- [ ] Unit/integration: denied agent types absent from task description
- [ ] Child session inherits derived rules; cannot redelegate when denied
- [ ] Coord/task lifecycle tests pass
- [ ] Progress ledger P4 complete

### Phase P5 — Bash / shell Harness ergonomics

**Goal:** Stop Harness-trained agents from thrashing on bash.

**Work:**

1. Re-read Harness `shell.ts` permission + path collection
2. Remove or narrow Harness hard bans that Harness does not apply:
   - `/dev/null` and safe device redirects
   - shell globs in arguments
   - other idioms proven allowed in Harness
3. Route out-of-workspace paths through external_directory-style permission
   ask/deny where possible
4. Align bash tool schema/description with Harness shell prompt
5. Keep permission pattern ask/deny for bash commands

**Acceptance (command-level — all must pass as tests or dogfood):**

- [ ] `git status -sb` works under allow/ask as configured
- [ ] `git log --oneline -5 2>/dev/null` does **not** fail with
      `path escapes workspace root ... /dev/null`
- [ ] `find crates -name '*.rs' | wc -l` does **not** fail with
      `shell glob path expansion is not allowed`
- [ ] True escape outside workspace + denied external_directory is denied
- [ ] Plan-mode read-only shell boundary still enforced if Harness plan
      restricts shell (verify source; adapt)
- [ ] Shell safety tests updated to Harness-aligned expectations
- [ ] Progress ledger P5 complete

### Phase P6 — Tool schema & description parity (permission-relevant set)

**Goal:** Model sees Harness-like tool contracts for tools involved in
permission UX.

**Minimum tools:** `bash`, `read`, `glob`, `grep`, `list`, `edit`, `write`,
`apply_patch`, `task`, `skill`, `webfetch`, `websearch`, `question`, `lsp`,
todo tools.

**Work:**

1. Diff schemas/descriptions against Harness tool modules
2. Align parameters and guidance
3. Snapshot tests for provider-exported defs per agent

**Acceptance:**

- [ ] Snapshot or structured equality tests for provider tool defs
- [ ] `native_tool_parity_matrix_test` updated if catalog rows change
- [ ] Progress ledger P6 complete

### Phase P7 — Error surfaces & model recovery

**Goal:** Denied/blocked results teach the model like Harness.

**Work:**

1. Permission deny/ask messages: short, actionable, no infinite-retry bait
2. Distinguish permission deny vs invalid args vs shell failure
3. Tests for message content contracts

**Acceptance:**

- [ ] Contract tests for deny/ask/block message shapes
- [ ] Dogfood: after one denied tool, next model action can succeed without
      human (mock provider scenario acceptable)
- [ ] Progress ledger P7 complete

### Phase P8 — Docs, examples, dogfood, full gates

**Goal:** Public contract and proof.

**Work:**

1. Update permissions/config/agents docs and examples
2. Update `configs/harness.example.jsonc` comments for ruleset-compatible
   permission examples
3. Full test lanes
4. End-to-end dogfood with mock and (if credentials exist) one live prompt
   that exercises explore + bash + plan edit boundary

**Acceptance:**

- [ ] Doc reference tests pass
- [ ] `scripts/test-lanes.sh fast` exit 0
- [ ] `scripts/test-lanes.sh quality-gates` exit 0
- [ ] `scripts/test-lanes.sh all-deterministic` exit 0 (or document
      pre-existing failures with proof they are unrelated — still must not
      introduce new failures)
- [ ] Dogfood artifact paths recorded in progress ledger
- [ ] Progress ledger P8 complete

---

## 7. Cross-cutting test matrix (must be green at end)

| ID | Scenario | Expected (Harness-like) |
|----|----------|---------------------------|
| T1 | Explore provider tools | No edit/write/task if denied; has read/glob/grep/list |
| T2 | Plan provider tools | Has edit; non-plan paths deny at execute; plan path allow |
| T3 | Build provider tools | Full overlapping tool set visible under allow |
| T4 | `permission.edit: deny` catch-all | `edit`/`write`/`apply_patch` absent from tools |
| T5 | `permission.bash: { "git *": "allow", "*": "ask" }` | git allow without prompt; other bash ask |
| T6 | Task description | Denied agents not listed |
| T7 | Child category agent | Cannot task-redelegate when deny |
| T8 | Bash `2>/dev/null` | Not path-escape hard fail |
| T9 | Bash glob `*.rs` | Not hard fail for glob expansion ban |
| T10 | External path | ask/deny via external_directory-equivalent, not silent wrong error |
| T11 | Config migration | Old Harness permission configs still load |
| T12 | Replay | Permission events still append-only; replay side-effect free |

---

## 8. Deferred / decision-required items

These may be deferred **only** with a dated decision record in the progress
ledger (`decision: defer|reject|implement`, rationale, Harness citation):

| Item | Default disposition |
|------|---------------------|
| Harness V2 SQLite persistent always-allow | Prefer event-sourced grants; match **session** always-allow UX |
| Harness `doom_loop` exact heuristics | Implement if present in agent defaults; else defer with citation |
| MCP tool permission naming parity | Implement if MCP tools are model-visible in shipped config |
| Desktop/app permission UI chrome | Out of scope unless required for CLI/TUI ask flow |
| Full non-permission tool behavior from tools PRD | Defer to `docs/tools-parity-prd.md` |

Deferred items **do not** count as complete parity for §0.1 rows that depend
on them. If a §0.1 row needs a deferred item, either implement it or change
the objective with an explicit human-approved decision (record in ledger).

---

## 9. Commands the implementer must run

### Per phase (minimum)

```bash
# After permission/core changes
cargo nextest run -p harness-core --test coord_test
cargo nextest run -p harness-core  # or targeted permission test binaries

# After tools/shell changes
cargo nextest run -p harness-tools
cargo nextest run -p harness-tools --test native_tool_parity_matrix_test

# After config/docs
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness --test bootstrap_profiles_test
```

### Before claiming PRD complete (all required)

```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh all-deterministic
cargo nextest run -p harness-core
cargo nextest run -p harness-tools
cargo nextest run -p harness-providers
cargo nextest run -p harness --test config_docs_reference_test
cargo nextest run -p harness --test bootstrap_profiles_test
```

Record exit codes in the progress ledger. Missing exit codes = incomplete.

---

## 10. Dogfood scenarios (required before complete)

Run through real Harness surfaces (mock provider acceptable for determinism):

1. **Explore child:** parent `task(subagent_type=explore)` → child never
   emits edit tool calls in provider request log / events
2. **Bash thrash repro:** agent/command executes the screenshot-class
   commands (`git … 2>/dev/null`, `find … '*.rs'`) successfully or with
   permission ask — not hard safety thrash
3. **Plan edit boundary:** plan can edit active plan file; cannot edit
   arbitrary source without deny
4. **Deny hide:** config deny on `webfetch` → tool absent from provider tools
   for that agent

Store event logs or test outputs under a path recorded in the progress ledger
(e.g. `target/harness-permissions-parity/dogfood/`).

---

## 11. Global completion checklist

The PRD is **not complete** until every box is checked with evidence links:

### Behavior

- [ ] ruleset-compatible permission evaluate + merge implemented and tested
- [ ] Catch-all deny tools omitted from provider tool lists
- [ ] Partial path/command allows keep tools visible
- [ ] Shipped agent defaults match Harness (build/plan/explore/general/hidden)
- [ ] Task description filters denied agents
- [ ] Child permission derivation matches Harness intent
- [ ] Bash Harness ergonomics (globs, `/dev/null`, pipes) verified by tests
- [ ] external_directory-equivalent for out-of-workspace paths
- [ ] Tool schemas/descriptions for permission-critical tools aligned
- [ ] Error messages support recovery (no thrash loops)

### Quality

- [ ] All phases P0–P8 marked complete in progress ledger with evidence
- [ ] No `inspirations/` modifications
- [ ] No tests deleted solely to avoid failures
- [ ] `scripts/test-lanes.sh fast` exit 0
- [ ] `scripts/test-lanes.sh quality-gates` exit 0
- [ ] `scripts/test-lanes.sh all-deterministic` exit 0
- [ ] Docs updated: permissions, config, native-tool-catalog, agents
- [ ] Example config shows ruleset-compatible permission examples
- [ ] Dogfood scenarios 1–4 recorded

### Honesty

- [ ] Every intentional divergence listed in progress ledger with rationale
- [ ] Deferred items dispositioned (§8)
- [ ] No completion claim without §12 certificate

---

## 12. Completion certificate (fill only when true)

Copy into the progress ledger when finished. **Leaving this blank or partial
means the work is incomplete.**

```markdown
## Completion certificate — Harness permissions parity

Date (ISO): ________
Implementer session / agent: ________

### Declaration
I certify that:
1. inspirations/harness was re-read for every phase and citations are in the ledger.
2. All phases P0–P8 are complete with command exit codes recorded.
3. All §11 checkboxes are true.
4. The following commands were run after the final change and exited 0:
   - scripts/test-lanes.sh fast → exit ___
   - scripts/test-lanes.sh quality-gates → exit ___
   - scripts/test-lanes.sh all-deterministic → exit ___
5. Dogfood artifact paths:
   - Explore: ________
   - Bash: ________
   - Plan: ________
   - Deny-hide: ________
6. Intentional divergences (or "none"):
   ________
7. I have not weakened tests or edited inspirations/ to force a pass.

Signed: ________
```

**If any line cannot be filled truthfully, continue the loop.**

---

## 13. Suggested first actions for the implementer

1. Create `docs/permissions-ruleset-parity-progress.md` from a template with
   all phases `not_started`
2. Re-read Harness permission + agent + shell + task sources; record sha
3. Implement P0 golden fixtures and a failing test for explore tool
   visibility vs current Harness (proves the gap)
4. Proceed P1→P8 without skipping gates

---

## 14. Relationship to other PRDs

| Document | Relationship |
|----------|----------------|
| `docs/tools-parity-prd.md` | Broader tool quality; this PRD owns permissions/agents/visibility/bash permission ergonomics |
| `docs/permissions.md` | Must be updated to match implemented behavior |
| `docs/tools-parity-progress.md` | Separate ledger; do not mark tools PRD complete from this work alone |

When both PRDs touch the same file, keep changes minimal and note cross-links
in both ledgers.

---

## 15. Success picture (operator-visible)

After completion, an operator should observe:

1. Explore/subagents **do not attempt** denied tools (they are not offered)
2. Bash commands that work in Harness **work in Harness** under the same
   permission posture
3. Config written like Harness permission docs **loads and enforces** the
   same way
4. Plan mode still protects the tree but allows plan-file edits like Harness
5. No multi-turn red error loops on `2>/dev/null` or `find -name '*.rs'`

Until that picture is proven by §11–§12, the implementer keeps working.
)
