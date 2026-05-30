# V1 Roadmap Claim Correction PRD

**Status:** Active implementation PRD for correcting checked roadmap claims that
are not yet true.
**Audience:** A single autonomous implementing agent working in this repository.
**Authority:** This PRD is subordinate to [`docs/roadmap-v1.md`](roadmap-v1.md)
for product scope, but it is the operational spec for making checked roadmap
claims truthful. Where a checked roadmap claim overstates the implementation,
this PRD controls the correction path: implement the claim with evidence, or
uncheck/reword it honestly.

---

## 0. Read this first: anti-gaming contract

This PRD exists because some roadmap boxes are checked even though the underlying
behavior is incomplete or overstated. The implementing agent must treat checkboxes
as evidence-backed claims, not intentions.

Violating any rule in this section means this PRD is not complete, regardless of
checkbox state.

### 0.1 Forbidden shortcuts

- **Do not** broadly flip roadmap checkboxes without source citations and evidence
  commands.
- **Do not** change, delete, ignore, or weaken tests to match false claims.
- **Do not** claim runtime extension support from manifest schema validation.
- **Do not** claim extension replay support from descriptor/config evidence only.
- **Do not** claim path-scoped permission support from coarse edit-deny tests.
- **Do not** move permission checks into adapters to bypass coordinator preflight.
- **Do not** add fallback/shim code solely to satisfy roadmap wording.
- **Do not** implement a full plugin architecture unless explicitly choosing the
  implementation path for extension runtime rows.
- **Do not** require or implement TUI reference-image comparison. The user has
  explicitly removed that item from this PRD's scope.
- **Do not** reopen unrelated roadmap percentages, denominator math, or unchecked
  future work.
- **Do not** treat stale evidence command names as a primary implementation gap
  unless directly tied to a checked roadmap row.

### 0.2 Required operating rules

- Keep scope small and limited to the checked-but-not-done claims in this PRD.
- Use TDD for implementation-backed fixes: write the failing test first, then the
  smallest correct implementation, then the evidence row.
- Preserve the coordinator as the preflight permission authority.
- Prefer honest documentation correction over speculative runtime implementation
  when runtime support is not required for V1.
- Every changed roadmap checkbox must have a source citation and evidence command
  in the progress/evidence docs.
- Before editing code, load the repository-mandated coding skill from
  [`AGENTS.md`](../AGENTS.md) and read any crate-scoped `AGENTS.md` that applies.

---

## 1. Problem statement

[`docs/roadmap-v1.md`](roadmap-v1.md) contains checked claims that are not
actually implemented. These false positives create bad readiness signals for
agents, reviewers, and release decisions.

The implementing agent must correct only the checked roadmap/progress/docs claims
that are provably false or incomplete. The agent must not broaden the project
scope, build unrelated roadmap work, or include the TUI reference-image
comparison item.

---

## 2. Scope

This PRD covers only checked roadmap/progress/docs claims that are marked done
but are not actually done.

The surviving checked-but-not-done items are:

1. `ast_grep_replace` list-valued path preflight permissions are incomplete.
2. Extension manifest runtime/replay claims overstate descriptor-only support.
3. Doctor/readiness messaging may overstate extension readiness beyond
   descriptor-only support.
4. Stale replay-hook evidence names may be corrected only as documentation
   hygiene if directly tied to a checked roadmap row.

---

## 3. Verified findings to re-check before implementation

These are point-in-time findings from the roadmap audit. The implementing agent
must re-read the cited files before changing code or docs.

### 3.1 AST-grep path-scoped permission gap

The roadmap/progress evidence treats `ast_grep_replace` as first-class and
edit-policy safe, but coordinator preflight path extraction misses list-valued
paths.

Observed gap:

- `crates/harness-tools/src/ast_grep.rs` defines `AstGrepReplaceArgs` with
  `paths: Vec<String>`.
- `crates/harness-core/src/coord.rs` extracts workspace path selectors from
  scalar keys such as `path`, `filePath`, and related scalar fields.
- `crates/harness-core/tests/coord_auth_test.rs` proves coarse `edit` denial, not
  path-scoped denial for list-valued `paths: [...]`.

Required outcome:

- Coordinator preflight must extract list-valued path selectors before adapter
  execution.
- `ast_grep_replace` with denied `paths: [...]` must be denied before
  `ToolCallStarted`.
- The AST-grep adapter must not execute when preflight denies a listed path.
- Allowed listed paths must still execute normally.

### 3.2 Extension manifest runtime/replay overclaims

The current extension manifest implementation is descriptor-only. Checked
roadmap/progress claims overstate runtime extension-provided tools and replay
rendering of extension events.

Observed gap:

- `crates/harness-core/src/extension_manifest.rs` reports descriptor-only runtime
  effects, including `registers_tools: false`.
- `configs/extension-manifest.v1.schema.json` covers manifest shape, not runtime
  tool registration.
- `crates/harness-core/tests/extension_manifest_test.rs` covers manifest
  validation, not runtime tool registration or extension event replay.
- [`docs/extension-strategy.md`](extension-strategy.md) already describes the V1
  seam as descriptor-only; checked roadmap/progress claims must match that truth
  unless runtime behavior is implemented and tested.

Required outcome:

For each checked extension-related roadmap/progress/docs row, choose exactly one
honest path:

1. **Implementation path:** implement the claimed runtime/replay behavior and add
   tests proving it.
2. **Documentation truth path:** uncheck or reword the checked claim so it states
   descriptor-only support honestly.

If choosing the documentation truth path, docs must explicitly say:

- Extension manifests are descriptor-only in V1.
- Extension-provided tools are not registered or executed in V1.
- No runtime permission path exists yet for extension-provided tools.
- Replay support is limited to existing descriptor/config evidence and does not
  render extension tool events.

If choosing the implementation path, tests must prove:

- Runtime extension-provided tools register through the real registry/coordinator
  path.
- Runtime extension tool calls go through coordinator permission checks.
- Replay renders old extension events if that behavior is claimed.

### 3.3 Doctor/readiness messaging weak gate

Doctor/readiness messaging must not imply extension readiness beyond descriptor-only
support.

Required outcome:

- Any checked roadmap row, progress evidence, or doctor output suggesting runtime
  extension readiness must be corrected.
- Extension readiness messaging must include `runtime effects: descriptor-only` or
  equivalent wording unless runtime extension behavior is implemented and tested.
- The wording must not imply extension-provided tools can currently be registered,
  executed, permissioned, or replay-rendered unless that is actually true.

### 3.4 Replay-hook evidence hygiene

This is documentation hygiene only, not a primary implementation workstream.

Required outcome:

- If a checked roadmap row cites stale replay-hook evidence command names, the
  agent may correct those names.
- The agent must not expand this into unrelated evidence cleanup.

---

## 4. Workstreams

### WS1: AST-grep permission TDD

Add focused coordinator authorization tests before implementation.

Required red test:

- Configure `edit` default allow.
- Add a deny rule for a subpath such as `src/`.
- Call `ast_grep_replace` with `paths: ["src/..."]` or the closest existing test
  fixture path syntax.
- Assert the call is denied before `ToolCallStarted`.
- Assert adapter execution does not occur.

Required green tests:

- Configure allowed path selectors.
- Call `ast_grep_replace` with allowed `paths: [...]`.
- Assert the call reaches normal execution behavior.
- Preserve existing scalar path selector behavior.

Implementation requirements:

- Extend coordinator preflight path extraction to include list-valued path
  selectors.
- Keep the change generic only if the existing selector logic naturally supports
  it.
- Do not move permission enforcement into the AST-grep adapter.
- Permission checks must remain before tool adapter execution.

### WS2: Extension claim truthfulness

Audit checked extension-related rows in roadmap/progress/docs. For each checked
row claiming runtime extension behavior, either implement the behavior with tests
or uncheck/reword the claim to descriptor-only support.

Forbidden wording unless implementation and tests exist:

- “Extension runtime support is ready.”
- “Extension tools go through permissions.”
- “Replay supports extension events.”

Required wording if runtime support is not implemented:

- “Extension manifests are descriptor-only in V1.”
- “Extension-provided tools are not registered or executed in V1.”
- “No runtime permission path exists yet for extension-provided tools.”
- “Replay support is limited to existing descriptor/config evidence and does not
  render extension tool events.”

### WS3: Doctor/readiness messaging

Inspect doctor/readiness output and checked roadmap evidence that references it.

Required correction:

- Any extension-readiness line must say `runtime effects: descriptor-only` or
  equivalent.
- The output must not claim runtime extension tool readiness unless implemented
  and tested.
- Tests or evidence checks must prove the corrected messaging.

### WS4: Evidence hygiene, only if needed

Correct stale replay-hook evidence command names only if directly tied to a
checked roadmap row.

This workstream must not include unrelated cleanup.

---

## 5. Acceptance criteria

- [ ] `ast_grep_replace` path-scoped deny rules for list-valued `paths: [...]` are
  enforced before adapter execution.
- [ ] A red/green coordinator authorization test proves denied
  `paths: ["src/..."]` does not emit `ToolCallStarted`.
- [ ] A green coordinator authorization test proves allowed list-valued AST-grep
  paths still execute.
- [ ] Existing scalar path selector authorization behavior remains covered and
  passing.
- [ ] Every checked extension-related roadmap/progress/docs claim is either
  implemented with tests or reworded/unchecked to descriptor-only truth.
- [ ] No extension runtime behavior is claimed from schema validation alone.
- [ ] If extension tools are not implemented, docs explicitly state
  extension-provided tools are not registered or executed in V1.
- [ ] If extension tools are not implemented, docs explicitly state no runtime
  permission path exists yet.
- [ ] If extension event replay is not implemented, docs explicitly state replay
  does not render extension tool events.
- [ ] Doctor/readiness messaging states `runtime effects: descriptor-only` or
  equivalent unless runtime extension behavior is implemented and tested.
- [ ] Any roadmap checkbox changed by the agent includes a source citation and
  evidence command.
- [ ] The TUI reference-image comparison issue is not included as required work.
- [ ] No unrelated roadmap percentages, denominator math, or unchecked items are
  reopened.
- [ ] Stale replay-hook evidence command names are corrected only if directly tied
  to checked roadmap evidence.

---

## 6. Verification gates

Run targeted tests first, then workspace gates.

### 6.1 Targeted gates

- [ ] Run the focused coordinator authorization test covering list-valued AST-grep
  paths.
- [ ] Run existing coordinator authorization tests.
- [ ] If extension runtime behavior is implemented, run tests proving extension
  tool registration, permission enforcement, and execution.
- [ ] If extension replay behavior is implemented, run tests proving old extension
  events render as claimed.
- [ ] If extension rows are reworded/unchecked instead of implemented, run
  doc/evidence consistency checks proving docs no longer claim runtime behavior.
- [ ] If doctor/readiness output changes, run the relevant doctor/readiness test or
  command proving descriptor-only wording.

### 6.2 Workspace gates

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace --all-features`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

### 6.3 Verification rules

- Runtime claims require runtime tests.
- Replay claims require replay tests.
- Permission claims require permission tests that exercise the relevant selector
  shape.
- Schema validation tests do not prove runtime extension behavior.
- Coarse permission tests do not prove path-scoped list-valued selector behavior.
- Docs-only rewording requires doc/evidence consistency checks, not fake runtime
  tests.

---

## 7. Progress and evidence requirements

Use the existing readiness evidence posture instead of inventing a new evidence
system.

For every checked roadmap box changed by this PRD's implementation, record:

- The exact roadmap checkbox text.
- The changed files.
- The evidence type: test, lane, command, docs-reference check, or documented
  limitation.
- The verification command and observed result.
- The source citation that proves the checkbox is now true, or the citation that
  justifies unchecking/rewording it.
- For every new test: a one-line “breaks if:” statement.

Valid evidence locations include
[`docs/v1-release-readiness-slice-progress.md`](v1-release-readiness-slice-progress.md)
and [`docs/claim-evidence-matrix.md`](claim-evidence-matrix.md), depending on the
kind of claim being changed. Do not mark a roadmap checkbox from intention.

---

## 8. Suggested atomic commit strategy

Use small, reviewable commits.

1. **AST-grep permission fix**
   - Add failing coordinator authorization tests.
   - Implement list-valued path selector preflight.
   - Prove denied paths stop before `ToolCallStarted`.
   - Prove allowed paths still execute.

2. **Extension truthfulness**
   - Audit checked extension rows.
   - Either implement runtime/replay behavior with tests or reword/uncheck
     descriptor-only claims.
   - Include evidence commands and source citations for every checkbox change.

3. **Doctor/readiness messaging**
   - Correct readiness wording to descriptor-only where applicable.
   - Add or update tests/evidence for doctor output.

4. **Evidence hygiene, only if needed**
   - Correct stale replay-hook evidence names tied to checked rows.
   - Do not include unrelated cleanup.

Each commit should pass its targeted tests before moving to the next commit. The
final branch must pass all workspace gates in §6.2.

---

## 9. Out of scope

The following are explicitly out of scope:

- TUI reference-image comparison.
- Full plugin architecture unless the agent chooses to implement extension runtime
  rows instead of correcting docs.
- Unchecked roadmap items.
- Roadmap percentage or denominator recalculation.
- Broad roadmap cleanup.
- Unrelated permission tools beyond the list-valued selector gap needed for
  `ast_grep_replace`.
- Replay-hook evidence cleanup not tied to checked roadmap claims.
- Cosmetic refactors.
- Backward-compatibility shims without a concrete runtime need.

---

## 10. Evidence references

Use these references when implementing and citing changes:

- `crates/harness-tools/src/ast_grep.rs`: `AstGrepReplaceArgs` uses
  `paths: Vec<String>`.
- `crates/harness-core/src/coord.rs`: coordinator selector preflight currently
  extracts scalar path keys such as `path`, `filePath`, and related scalar fields.
- `crates/harness-core/tests/coord_auth_test.rs`: existing coverage proves coarse
  edit deny only, not path-scoped deny for `paths: [...]`.
- `crates/harness-core/src/extension_manifest.rs`: extension manifest runtime
  effects are descriptor-only / `registers_tools: false`.
- `configs/extension-manifest.v1.schema.json`: covers manifest shape, not runtime
  tool registration.
- `crates/harness-core/tests/extension_manifest_test.rs`: covers manifest
  validation, not runtime tool registration or extension event replay.
- [`docs/extension-strategy.md`](extension-strategy.md): current V1 extension
  strategy states descriptor-only manifest support.
- [`docs/roadmap-v1.md`](roadmap-v1.md): source of checked roadmap claims that
  must be made truthful.
