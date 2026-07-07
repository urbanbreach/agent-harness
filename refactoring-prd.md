# PRD: Rust Codebase Alignment with Programming Skill Rules

**Document Type:** Product Requirements Document / Implementation Spec
**Target:** Autonomous implementer agent running in a loop until completion
**Created:** 2026-07-07
**Provenance:** Distilled from a 5-member adversarial hyperplan team (quick-scanner, arch-auditor, type-system-hawk, prd-architect, loc-feasibility) over 3 rounds of independent analysis, cross-attack, and defend/refine/concede. Every claim was verified against the codebase with grep/awk/codegraph. The plan agent verified all claims independently before producing this structure.

---

## How to Use This PRD — Progress Tracking Guide

This PRD contains **checkboxes** (`- [ ]`) throughout every actionable section. The implementer agent MUST:

1. **Check a box** (`- [x]`) immediately after completing the corresponding item
2. **Never skip ahead** — checkboxes are ordered; do not check a box until all prior boxes in the same section are checked
3. **Update the progress manifest** (`docs/refactoring-progress.json`) every time you check a box, with the current LOC count and gate status
4. **Do not claim completion** until ALL checkboxes in ALL sections are checked `[x]`

### Master Progress Checklist

Use this top-level checklist to track overall progress. Check each box only when the entire section is complete.

- [ ] **Section 0:** Baseline LOC pinned and recorded in progress manifest
- [ ] **Section 2:** All 5 skills loaded and read
- [ ] **Section 3:** All 9 violation categories (V1-V9) fixed
- [ ] **Section 4 Track A:** All 7 cross-cutting changes (A1-A7) complete (in dependency order)
- [ ] **Section 4 Track B:** All oversized files processed
- [ ] **Section 4 Phase 1:** 10% net reduction achieved
- [ ] **Section 4 Phase 2:** 20% net reduction achieved
- [ ] **Section 4 Phase 3:** All opportunities exhausted (exhaustion checklist complete)
- [ ] **Section 5:** cargo-machete and cargo-udeps run (ask user to install if missing)
- [ ] **Section 6:** All 8 completion gates (G1-G8) pass
- [ ] **Section 7:** All 11 anti-shortcut mechanisms verified (including rollback + resume protocol)
- [ ] **Section 10:** Loop termination protocol satisfied — ALL conditions + exhaustion checklist met

**The loop is NOT complete until every checkbox in this document is `[x]`.**

---

## Section 0 — Baseline Pin (MANDATORY FIRST STEP)

Before any work begins, the implementer MUST lock the baseline LOC. Run this exact command and record the result:

- [ ] **Run the baseline command and record the result:**

```bash
find crates -name "*.rs" \
  -not -path "*/tests/*" \
  -not -path "*/lib_tests/*" \
  -not -path "*/ui_transcript_exact_tests/*" \
  -not -path "*/fixtures/*" \
  | grep -vE '(^|/)test_|_test\.rs$|_tests\.rs$|(^|/)tests\.rs$' \
  | grep -viE 'fixture|snapshot' \
  | while read f; do scripts/strip-cfg-test.sh "$f" | awk '!/^[[:space:]]*$/ && !/^[[:space:]]*\/\//'; done \
  | wc -l
```

**Verified result:** `124979`

This command:
1. Excludes test directories (`tests/`, `lib_tests/`, `ui_transcript_exact_tests/`, `fixtures/`)
2. Excludes test-named files (`test_*.rs`, `*_test.rs`, `*_tests.rs`, `tests.rs`) using precise patterns (not a broad "test" substring match that would exclude files like `latest_version.rs`)
3. Strips inline `#[cfg(test)]` module bodies from each file using `scripts/strip-cfg-test.sh`
4. Counts only non-blank, non-comment lines (pure LOC)

- [ ] **Record `BASELINE_LOC` in `docs/refactoring-progress.json`** (set `baseline_loc` field to the command output)
- [ ] **Confirm the number matches ~124,979** (if significantly different, investigate before proceeding)

This number is `BASELINE_LOC`. Every LOC measurement in this PRD references this baseline. The implementer must record it in the progress manifest (Section 8) before touching any file.

**Why this number and not 170K:** The codebase has 762 Rust files totaling 278K LOC. Of that, ~34,908 LOC are inline `#[cfg(test)]` modules living inside `src/` directories — these are TEST code that only compiles in test builds, not production code. The baseline strips them using `scripts/strip-cfg-test.sh`. The remaining ~114K LOC are dedicated test files under `tests/` directories — also excluded. The 124,979 figure is the true non-test production source LOC across 373 files.

---

## Section 1 — Scope

### In Scope
- All 373 non-test Rust source files under `crates/` (excluding `*/tests/*`, `*/lib_tests/*`, `*/ui_transcript_exact_tests/*`, `*/fixtures/*`, and test-named files)
- Workspace manifests (`Cargo.toml`) for lint configuration
- The `build.rs` files if any exist

### Out of Scope
- Test files (under `tests/` directories, or named `*_test.rs`, `tests.rs`, `*_tests.rs`)
- Inline `#[cfg(test)]` modules within source files (these are test code)
- Generated code (e.g., `configs/` schemas)
- Documentation files (`*.md`)
- Configuration files (`*.json`, `*.jsonc`, `*.toml` except `Cargo.toml` lint config)
- The `target/` directory
- The `inspirations/`, `.codex/`, `.omx/` directories (external reference content)

### Tests: Excluded from LOC Target, Mandatory for Verification
Tests are excluded from the LOC reduction target. However, ALL tests MUST pass at every checkpoint. No test may be deleted, skipped, or weakened to achieve compliance. The test suite (114K+ LOC of tests) IS the functionality-preservation gate.

### Definition of "Functionality"
**Functionality = anything exercised by the test suite OR reachable by a caller.** If a test covers a code path, removing that code path is functionality loss and is FORBIDDEN. If no test exercises a code path BUT the code is reachable (a caller invokes it), the implementer MUST write a test covering it BEFORE removing it — if the test passes, the code is functionality and must be kept; if the test fails, the code was broken and should be fixed. If the code is UNREACHABLE (no caller invokes it, verified by `codegraph_callers` or grep), removing it is dead-code removal, NOT functionality loss. When in doubt: verify reachability first, test second, remove third.

---

## Section 2 — Skill Integration Requirements (MANDATORY)

The implementer MUST load and follow these skills throughout the entire refactoring loop. This is non-negotiable.

### Mandatory Skill Loading Order (BEFORE any work)

- [ ] **1. `shared/programming`** — Load this FIRST. Read `references/rust/README.md` and every file it references on demand. This skill defines the iron rules: type-strict, parse-don't-validate, exhaustive matching, no escape hatches, 250 LOC ceiling, TDD, post-write review loop.

- [ ] **2. `shared/refactor`** — Load this BEFORE any structural change. Follow its safe-refactor protocol: codemap → plan → LSP-driven edits → test after each step. NEVER improvise a refactor. The refactor skill exists so you do not corrupt behavior while reshaping structure.

- [ ] **3. `karpathy-guidelines`** — Behavioral guidelines: surgical changes, surface assumptions, define verifiable success criteria, avoid overcomplication.

- [ ] **4. `rust-best-practices`** — Idiomatic Rust patterns: borrowing vs cloning, ownership, Result types, error handling, performance.

- [ ] **5. `rust-async-patterns`** — Tokio async patterns: JoinSet, cancellation, select, blocking work. Load when touching async code.

### Post-Write Review Loop (after EVERY file change)

The programming skill mandates a post-write review loop after every code change. The implementer MUST run this loop:

- [ ] **1. Measure:** `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/)/' <file> | wc -l`
- [ ] **2. Interpret:** ≤200 healthy | 200-250 warning | >250 DEFECT (refactor before adding lines)
- [ ] **3. Architectural self-review** (11 questions — see programming skill's POST-WRITE REVIEW LOOP section)
- [ ] **4. If any smell fired:** load `refactor` skill and execute its safe-refactor protocol

### TDD Orientation

- [ ] Lock behavior with tests BEFORE structural changes
- [ ] If a refactor changes behavior, the test that catches it must exist BEFORE the refactor
- [ ] Never delete a failing test — fix the code or fix the test

---

## Section 3 — Violation Catalog (with verified evidence)

Each violation category includes verified counts, file:line examples, severity, and fix approach. All counts were verified by the adversarial team and independently confirmed by the plan agent.

### Violation Fix Progress

- [ ] **V1: Oversized Files** — All 189 files >250 pure LOC split or marked SIZE_OK
- [ ] **V2: UnwrapOrAbort Duplication** — Consolidated to 1 definition in harness-core
- [ ] **V3: serde_json::Value in Config** — Replaced with typed structs
- [ ] **V4: Missing Newtypes** — All 6+ newtypes added (RunId, SessionId, TaskId, RequestId, ToolCallId, RunName)
- [ ] **V5: `as` Numeric Casts** — All 49 non-test casts replaced with `try_from`/`From`
- [ ] **V6: `#[allow]` Attributes** — All 14 reviewed; 12 removed (V2 fix), 2 justified with `reason="..."`
- [ ] **V7: Tool Boilerplate Duplication** — Macro/builder created, ~630 LOC eliminated
- [ ] **V8: map_err Pattern Duplication** — Helper created, 89 call sites simplified
- [ ] **V9: unwrap()/expect() Outside Tests** — All ~60 non-test calls replaced with `?`/typed errors

---

### V1: Oversized Files (>250 Pure LOC) — CRITICAL

**Verified count:** 189 files exceed 250 pure LOC
**Total LOC in oversized files:** ~113K (85% of non-test code)
**Severity:** DEFECT (per programming skill — ">250 is a DEFECT, refactor before adding lines")

**Top 15 offenders (by pure LOC):**

| Pure LOC | File | What it owns | Split recommendation |
|----------|------|-------------|---------------------|
| 1574 | `harness-tui/.../parity_matrix.rs` | Command palette data + enums | SIZE_OK — pure data table |
| 1543 | `harness-tools/src/shell_safety.rs` | Shell safety checking (82 fns) | Split by safety category |
| 1182 | `harness-tools/src/shell_run.rs` | Bash execution (54 fns) | Split by execution phase |
| 1163 | `harness-tui/.../key_interaction.rs` | TUI key dispatch (20 fns) | Split by key category |
| 1052 | `harness-tui/src/keybindings.rs` | Keybinding registry | Split registry from parsing |
| 980 | `harness/src/doctor.rs` | CLI doctor (25 fns) | Split by diagnostic category |
| 963 | `harness/src/tui.rs` | TUI entrypoint | Split entrypoint from setup |
| 961 | `harness-tui/src/ui_chrome.rs` | TUI rendering | Split by UI component |
| 954 | `harness-tui/.../session_projection.rs` | Session state | Split projection from state |
| 951 | `harness-core/.../agent_turn_completion.rs` | Coordinator turn completion | Split by completion phase |
| 951 | `harness-core/src/config/public.rs` | Public config contract | Split by config domain |
| 948 | `harness-tools/src/ast_grep.rs` | ast-grep wrapper (37 fns) | Split by operation type |
| 939 | `harness-core/.../task_lifecycle.rs` | Task lifecycle | Split by lifecycle phase |
| 928 | `harness-tui/src/ui_overlays.rs` | TUI overlays | Split by overlay type |
| 926 | `harness/src/sessions.rs` | CLI session management | Split by session operation |

**Fix approach:** Split by responsibility. Each file must own ONE thing (nameable in a single noun phrase without "and"). Use `// allow: SIZE_OK — <specific reason>` for data tables and indivisible state machines.

**V1 Checklist:**
- [ ] `parity_matrix.rs` — marked SIZE_OK (pure data table) OR split
- [ ] `shell_safety.rs` — split by safety category
- [ ] `shell_run.rs` — split by execution phase
- [ ] `key_interaction.rs` — split by key category
- [ ] `keybindings.rs` — split registry from parsing
- [ ] `doctor.rs` — split by diagnostic category
- [ ] `tui.rs` — split entrypoint from setup
- [ ] `ui_chrome.rs` — split by UI component
- [ ] `session_projection.rs` — split projection from state
- [ ] `agent_turn_completion.rs` — split by completion phase
- [ ] `config/public.rs` — split by config domain
- [ ] `ast_grep.rs` — split by operation type
- [ ] `task_lifecycle.rs` — split by lifecycle phase
- [ ] `ui_overlays.rs` — split by overlay type
- [ ] `sessions.rs` — split by session operation
- [ ] All remaining 174 oversized files processed (see Track B)

### V2: UnwrapOrAbort Duplication — HIGH

**Verified count:** Trait defined in 5 crates (harness-core, harness-providers, harness-tools, harness-tui, harness)
**Each copy:** ~15-20 LOC of trait + impls with `#[allow(clippy::panic, clippy::match_wild_err_arm)]`
**Total removable:** ~75-120 LOC
**Severity:** HIGH (duplicated panic-in-lib pattern)

**Evidence:**
- `harness-core/src/lib.rs` — original definition
- `harness-tools/src/lib.rs` — duplicate
- `harness-providers/src/lib.rs` — duplicate
- `harness/src/lib.rs` — duplicate
- `harness-tui/src/lib.rs` — duplicate

**Fix approach:** Consolidate to 1 definition in `harness-core/src/lib.rs`. Re-export from other crates. This removes ~75-120 LOC of duplicate code AND 12 `#[allow]` attributes.

**V2 Checklist:**
- [ ] Consolidate trait definition to `harness-core/src/lib.rs`
- [ ] Remove duplicate from `harness-tools/src/lib.rs`
- [ ] Remove duplicate from `harness-providers/src/lib.rs`
- [ ] Remove duplicate from `harness/src/lib.rs`
- [ ] Remove duplicate from `harness-tui/src/lib.rs`
- [ ] Add re-exports from each crate that previously had its own definition
- [ ] Verify `cargo check --workspace` passes
- [ ] Verify `cargo nextest run --workspace` passes

**Note:** All `unwrap_or_abort()` CALLS are inside `#[cfg(test)]` modules. The trait is in the public API but only invoked in tests. The violation is the duplicated definition + allow attributes, not production panics.

### V3: serde_json::Value in Config — HIGH

**Verified count:** 53 matches in `config/public.rs`
**Actual fields typed as `Option<serde_json::Value>` or `BTreeMap<String, serde_json::Value>`:** 11
**Dead validation code:** ~280-624 LOC of runtime Value normalization that becomes dead when types are correct
**Severity:** HIGH (parse-don't-validate breach — the programming skill's Rule #2)

**Fix approach:** Replace `serde_json::Value` fields with typed structs using `#[derive(Deserialize)]`. This ADDS LOC (struct definitions) but REMOVES LOC (dead validation code). Net impact: approximately neutral per file, but type safety is significantly improved.

**V3 Checklist:**
- [ ] Audit all 53 `serde_json::Value` matches in `config/public.rs`
- [ ] Identify which fields are genuinely dynamic (MCP options) vs should be typed
- [ ] Create typed structs for non-dynamic fields with `#[derive(Deserialize)]`
- [ ] Replace `serde_json::Value` fields with typed structs
- [ ] Remove dead validation/normalization code (~280-624 LOC)
- [ ] Mark genuinely dynamic fields with `// allow: DYNAMIC — <reason>`
- [ ] Verify `config_schema_cli_test` passes
- [ ] Verify net LOC delta for the file is ≤0

**Constraint:** 2 fields may intentionally use `serde_json::Value` for dynamic MCP options. These should be documented with `// allow: DYNAMIC — MCP plugin options are intentionally untyped` and are exempt from G6.

### V4: Missing Newtypes — MEDIUM

**Verified count:** Only 1 newtype exists (`ProviderId`)
**References to String-typed IDs in non-test code:** 364
**Severity:** MEDIUM (type system underutilization — the programming skill's Rule #3)

**Newtypes needed:**
- `RunId` (78 occurrences of `run_id: String`/`run_id: &str`)
- `TaskId` (70 occurrences)
- `RequestId` (199 occurrences — includes `request_id: Option<String>`)
- `SessionId` (68 occurrences)
- `ToolCallId` (if applicable)
- `RunName` (if applicable)

**Fix approach:** Add newtype tuple structs with `Display`/`Debug`/`From`/`Into`/`serde` impls. Update all call sites.

**V4 Checklist:**
- [ ] Add `RunId` newtype (78 occurrences to update)
- [ ] Add `TaskId` newtype (70 occurrences to update)
- [ ] Add `RequestId` newtype (199 occurrences to update)
- [ ] Add `SessionId` newtype (68 occurrences to update)
- [ ] Add `ToolCallId` newtype (if applicable)
- [ ] Add `RunName` newtype (if applicable)
- [ ] Update all call sites with `.into()` / `.0` / `.to_string()` conversions
- [ ] Verify net LOC delta per affected file is ≤0 (offset additions with reductions)

**LOC impact:** +80-120 LOC (struct definitions + impls) + +300-600 LOC (call site conversions). This is a LOC INCREASE but is REQUIRED by the programming skill. Must be offset by equal/greater reduction in the same file.

### V5: `as` Numeric Casts — MEDIUM

**Verified count:** 49 in non-test code
**Top offenders:** `hashline_edit.rs` (6), `auth/codex.rs` (5), `clipboard.rs` (4)
**Severity:** MEDIUM (escape hatch — the programming skill's iron list: "no `as` for narrowing")

**Fix approach:** Replace with `try_from().unwrap_or()` or proper `From`/`TryFrom` impls. For safe widening casts (`as usize`), document with `// allow: WIDENING — value is bounded` if replacement is impractical.

**V5 Checklist:**
- [ ] Fix `hashline_edit.rs` (6 casts)
- [ ] Fix `auth/codex.rs` (5 casts)
- [ ] Fix `clipboard.rs` (4 casts)
- [ ] Fix all remaining ~34 casts across other files
- [ ] Mark any safe widening casts with `// allow: WIDENING — <reason>`

### V6: `#[allow]` Attributes — LOW (after V2 fix)

**Verified count:** 14 in non-test code
**Breakdown:** 12 silence `clippy::panic` for UnwrapOrAbort (removed by V2 fix), 2 have documented `reason = "..."` fields
**Severity:** LOW (after V2 consolidation removes 12 of 14)

**Fix approach:** After V2 consolidation, 2 `#[allow]` attributes remain. These must have `reason = "..."` fields documenting why the allowance is justified. If no justification can be written, the allowance must be removed and the underlying issue fixed.

**V6 Checklist:**
- [ ] Confirm 12 `#[allow]` for UnwrapOrAbort removed (depends on V2 completion)
- [ ] Audit remaining 2 `#[allow]` attributes
- [ ] Add `reason = "..."` to each justified allowance
- [ ] Remove any unjustified allowances and fix the underlying issue

### V7: Tool Boilerplate Duplication — HIGH (biggest LOC reduction opportunity)

**Verified count:** 16 files with tool implementations containing repeated `id()`, `description()`, `capability()`, `parameters_json_schema()` methods
**Estimated boilerplate:** ~630 LOC across 42 tool implementations
**Severity:** HIGH (biggest single LOC reduction opportunity)

**Fix approach:** Create a `#[derive(Tool)]` macro or builder pattern that eliminates the boilerplate. Each tool definition should be data-driven (attributes or a builder), not method-driven.

**V7 Checklist:**
- [ ] Design the `#[derive(Tool)]` macro or builder pattern
- [ ] Implement the macro/builder
- [ ] Migrate all 42 tool implementations to use the macro/builder
- [ ] Verify `native_tool_parity_matrix_test` passes
- [ ] Verify ~630 LOC of boilerplate eliminated

**Risk:** MEDIUM — macro complexity. Must verify all 42 tools still pass `native_tool_parity_matrix_test`.

### V8: map_err Pattern Duplication — MEDIUM

**Verified count:** 89 `map_err(|err| ToolError::Execution(format!(...)))` call sites
**Severity:** MEDIUM (repeated pattern)

**Fix approach:** Create a helper macro or function:
```rust
macro_rules! tool_exec_err {
    ($e:expr) => { $e.map_err(|err| ToolError::Execution(format!("{err}"))) };
}
```
Or a `From` impl for the error type.

**V8 Checklist:**
- [ ] Create the `tool_exec_err!` macro or `From` impl
- [ ] Replace all 89 `map_err(|err| ToolError::Execution(format!(...)))` call sites
- [ ] Verify `cargo check --workspace` passes
- [ ] Verify `cargo nextest run --workspace` passes

### V9: unwrap()/expect() Outside Tests — MEDIUM

**Verified count:** ~60 in non-test code (41% of total unwrap/expect calls)
**Severity:** MEDIUM (escape hatch — the programming skill's iron list: "no `unwrap`/`expect` outside `main`/tests")

**Fix approach:** Replace with `?` operator or typed error returns. Each replacement may ADD LOC (error variant definitions, Result propagation) — must be offset by equal/greater reduction in the same file.

**V9 Checklist:**
- [ ] Enumerate all ~60 non-test `unwrap()`/`expect()` calls with file:line
- [ ] Replace each with `?` operator or typed error returns
- [ ] Add error variant definitions where needed (offset LOC with reductions in same file)
- [ ] Verify zero `unwrap()`/`expect()` remain in non-test code (G4)

---

## Section 4 — Two-Track Execution Plan

### Track A: Cross-Cutting Changes (DO FIRST — Biggest ROI)

These changes affect multiple files simultaneously and CANNOT be done in a file-by-file loop. They must be completed before Track B begins.

**Track A Steps (check each box when the step is complete and verified):**

Steps MUST be done in this order — dependencies are explicit:

- [ ] **A1: UnwrapOrAbort consolidation** (5 crates → 1) — ~75-120 LOC saved, LOW risk
  - *No dependencies. Do first — it's the safest win and unblocks A7.*
- [ ] **A2: Tool boilerplate macro** (16+ tool files) — ~630 LOC saved, MEDIUM risk
  - *No dependencies on other Track A steps. Do early — biggest LOC reduction.*
- [ ] **A3: map_err helper** (89 call sites) — ~89-178 LOC saved, LOW risk
  - *No dependencies. Do early — safe LOC reduction that banks savings before A4-A6 add LOC.*
- [ ] **A4: Newtype definitions** (RunId, SessionId, TaskId, RequestId, ToolCallId, RunName) — +380-720 LOC INCREASE, MEDIUM risk
  - *DEPENDS ON A1-A3 being complete first. A4 ADDS LOC — it must be offset by reductions already banked from A1-A3. Do AFTER the LOC-reducing steps so the net LOC trend is still downward.*
- [ ] **A5: serde_json::Value → typed config structs** (config/public.rs) — ~neutral, MEDIUM risk
  - *Can be done in parallel with A4. Net-neutral per file, but removes dead validation code.*
- [ ] **A6: `as` cast → `try_from` replacements** (~10 files) — +30 LOC INCREASE, LOW risk
  - *DEPENDS ON A1-A3 being complete first. A6 ADDS LOC — offset with banked reductions.*
- [ ] **A7: `#[allow]` cleanup** (after A1 removes 12 of 14) — ~0 LOC, LOW risk
  - *DEPENDS ON A1. A1 removes 12 of 14 `#[allow]` attributes. A7 audits the remaining 2.*

**Track A verification (run after EACH step):**
```bash
cargo check --workspace && cargo clippy --workspace -- -D warnings && cargo nextest run --workspace
```

- [ ] **Track A complete:** All 7 steps (A1-A7) done and verified

### Track B: File-by-File Splitting & Cleanup (DO SECOND)

Process all 189 oversized files, largest first. For each file:

- [ ] **1. Measure:** `awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/)/' <file> | wc -l`
- [ ] **2. Name what it owns:** Can you describe it in one noun phrase without "and"? If not, it needs splitting.
- [ ] **3. YAGNI audit:** Run the per-file checklist (Section 5)
- [ ] **4. Split or mark SIZE_OK:** Split by responsibility, or mark with `// allow: SIZE_OK — <reason>`
- [ ] **5. Verify:** `cargo check && cargo clippy -- -D warnings && cargo nextest run` (adjacent tests)
- [ ] **6. Record:** Update progress manifest with before/after LOC

**Track B ordering:** Process files largest-first (parity_matrix.rs → shell_safety.rs → shell_run.rs → ...). This maximizes early LOC reduction and exposes cross-file dependencies early.

**Track B Progress (check after each file is fully processed):**
- [ ] All 189 oversized files processed (check when `track_b_files_remaining == 0` in manifest)

### Graduated Phases

The 50% LOC reduction target is the **goal**. The implementer MUST attempt every possible reduction strategy to reach it. The phases below are progress milestones, NOT stopping points.

| Phase | Target | Completion Criteria |
|-------|--------|---------------------|
| Phase 1 | 10% net reduction (~12,498 LOC) | Track A complete + Track B in progress |
| Phase 2 | 20% net reduction (~24,996 LOC) | Track B 50% complete |
| Phase 3 | Exhaust ALL opportunities | Track B 100% + exhaustion checklist complete |

**Phase Progress:**
- [ ] **Phase 1:** 10% net reduction achieved (current LOC ≤ 112,481)
- [ ] **Phase 2:** 20% net reduction achieved (current LOC ≤ 99,983)
- [ ] **Phase 3:** All opportunities exhausted (exhaustion checklist in Section 10 complete)

**CRITICAL — Do not use the estimate as an excuse to stop.** The adversarial analysis estimated 3.0-4.7% net reduction is achievable. This estimate exists for PLANNING CONTEXT ONLY. It is NOT a license to stop at 5%. The implementer MUST:
- Attempt every reduction strategy in the catalog (Section 3, Appendix B)
- Run the full exhaustion checklist (Section 10) before declaring completion
- Treat each phase milestone as a progress signal, not a finish line
- Only terminate when the exhaustion checklist is complete AND all 8 gates pass

**50% is the goal.** The implementer aims for it, exhausts every opportunity, and reports the actual achieved percentage. The agent terminates when all 8 gates pass AND the exhaustion checklist (Section 10) is complete — NOT when 50% is hit, and NOT when the estimate is reached.

---

## Section 5 — YAGNI Audit Framework

The implementer MUST run this checklist on EVERY file during Track B:

### Per-File YAGNI Checklist

- [ ] **Does this code need to exist?** (Axiom 0: the best code is the code never written)
- [ ] **Does the codebase already have this?** (Reuse the helper or pattern, do not re-implement)
- [ ] **Does the standard library do it?** (Use std instead of custom impl)
- [ ] **Does an installed dependency solve it?** (Use the dependency instead of custom impl)
- [ ] **Can it be one line?** (If yes, inline it)
- [ ] **Are there helpers for one-off use?** (Inline them — "No helpers for one-off" per iron list)
- [ ] **Is there redundant verification after destructive actions?** (Delete it — the operation's contract IS the proof)
- [ ] **Negative-form names?** (Rename to positive form: `isValid` not `isNotValid`)
- [ ] **Parameter bloat (>3 params)?** (Group related params into a typed value object)
- [ ] **File >250 pure LOC?** (Split by responsibility, or mark SIZE_OK with justification)

### What is NOT a YAGNI Violation

- **DI traits with Real + Fake implementations** — These are the standard Rust testing pattern. The programming skill endorses fakes behind interfaces. 12 of 15 single-impl traits in this codebase are DI for testing. DO NOT delete them.
- **Config structs in the public contract** — Config is the public API. Cutting here risks breaking downstream consumers. DO NOT remove public config keys.
- **Justified `#[allow]` with `reason = "..."`** — The programming skill permits documented allowances. These are NOT violations.

### YAGNI Toolchain (run these as part of the YAGNI audit)

The programming skill's toolchain table lists these tools. The implementer MUST run them as part of the YAGNI audit. These tools may already be installed in the environment — if not, ask the user to install them.

- [ ] **Run `cargo-machete`** — `cargo machete --workspace`. Detects unused dependencies in `Cargo.toml`. Remove any unused dependencies found.
- [ ] **Run `cargo-udeps`** — `cargo +nightly udeps --workspace`. Detects unused crate features. Remove any unused features found.

---

## Section 6 — 8 Completion Gates (Non-Gameable)

The implementer MUST pass ALL 8 gates before claiming completion. Each gate is verifiable by a specific command — no subjective judgment.

**Gate Progress (check each box only after the gate command outputs zero violations):**

- [ ] **G1: Clippy Regression Gate**
```bash
cargo clippy --workspace --all-features -- -D warnings
```
**Status:** Currently PASSES. Must remain passing after every change.

- [ ] **G2: Full Test Suite Gate**
```bash
cargo nextest run --workspace
```
**Status:** Must pass with zero failures. No test may be deleted, skipped, or weakened.

- [ ] **G3: No Oversized Files Gate**
```bash
# For each non-test .rs file, strip #[cfg(test)] modules, count pure LOC, flag >250
find crates -name "*.rs" \
  -not -path "*/tests/*" \
  -not -path "*/lib_tests/*" \
  -not -path "*/ui_transcript_exact_tests/*" \
  -not -path "*/fixtures/*" \
  | grep -vE '(^|/)test_|_test\.rs$|_tests\.rs$|(^|/)tests\.rs$' \
  | grep -viE 'fixture|snapshot' \
  | while read f; do
      loc=$(scripts/strip-cfg-test.sh "$f" | awk '!/^[[:space:]]*$/ && !/^[[:space:]]*\/\//' | wc -l)
      if [ "$loc" -gt 250 ]; then
        if ! grep -q "allow: SIZE_OK" "$f"; then
          echo "VIOLATION: $f has $loc pure LOC (no SIZE_OK marker)"
        fi
      fi
    done
```
**Status:** Must output zero violations. Files >250 LOC must have `// allow: SIZE_OK — <specific reason>`. Uses `scripts/strip-cfg-test.sh` to strip inline `#[cfg(test)]` module bodies before counting.

- [ ] **G4: Zero unwrap()/expect() Outside Tests**
```bash
# Find unwrap/expect in non-test code, properly excluding #[cfg(test)] module bodies
find crates -name "*.rs" \
  -not -path "*/tests/*" \
  -not -path "*/lib_tests/*" \
  -not -path "*/ui_transcript_exact_tests/*" \
  -not -path "*/fixtures/*" \
  | grep -vE '(^|/)test_|_test\.rs$|_tests\.rs$|(^|/)tests\.rs$' \
  | grep -viE 'fixture|snapshot' \
  | while read f; do
      scripts/strip-cfg-test.sh "$f" | grep -nE '\.unwrap\(\)|\.expect\(' | while read line; do
        echo "$f:$line"
      done
    done
```
**Status:** Must output zero lines. Uses `scripts/strip-cfg-test.sh` to strip inline `#[cfg(test)]` module bodies before searching, so unwrap/expect calls inside test modules are not counted.

- [ ] **G5: Zero Unjustified #[allow]**
```bash
grep -rn '#\[allow(' crates/ --include="*.rs" \
  | grep -v '/tests/' \
  | grep -viE 'test|fixture|snapshot' \
  | grep -v 'reason ='
```
**Status:** Must output zero lines. All `#[allow]` must have `reason = "..."` field.

- [ ] **G6: Zero serde_json::Value in Public Config**
```bash
grep -n 'serde_json::Value' crates/harness-core/src/config/public.rs
```
**Status:** Must output zero lines (or only lines with `// allow: DYNAMIC — <reason>` marker).

- [ ] **G7: Zero `as` Numeric Casts in Non-Test Code**
```bash
# Find `as` numeric casts in non-test code, properly excluding #[cfg(test)] module bodies
find crates -name "*.rs" \
  -not -path "*/tests/*" \
  -not -path "*/lib_tests/*" \
  -not -path "*/ui_transcript_exact_tests/*" \
  -not -path "*/fixtures/*" \
  | grep -vE '(^|/)test_|_test\.rs$|_tests\.rs$|(^|/)tests\.rs$' \
  | grep -viE 'fixture|snapshot' \
  | while read f; do
      scripts/strip-cfg-test.sh "$f" | grep -nE ' as [uiaf][0-9]' | grep -v 'allow: WIDENING' | while read line; do
        echo "$f:$line"
      done
    done
```
**Status:** Must output zero lines. Uses `scripts/strip-cfg-test.sh` to strip inline `#[cfg(test)]` module bodies before searching. All `as` casts must be replaced with `try_from`/`From`, or marked `// allow: WIDENING — <reason>`.

- [ ] **G8: LOC Reduction Reported**
```bash
find crates -name "*.rs" \
  -not -path "*/tests/*" \
  -not -path "*/lib_tests/*" \
  -not -path "*/ui_transcript_exact_tests/*" \
  -not -path "*/fixtures/*" \
  | grep -vE '(^|/)test_|_test\.rs$|_tests\.rs$|(^|/)tests\.rs$' \
  | grep -viE 'fixture|snapshot' \
  | while read f; do scripts/strip-cfg-test.sh "$f" | awk '!/^[[:space:]]*$/ && !/^[[:space:]]*\/\//'; done \
  | wc -l
```
**Status:** Must be less than `BASELINE_LOC` (124,979). Report actual reduction percentage. 50% is aspirational — the agent terminates when all other gates pass AND the exhaustion checklist (Section 10) is complete.

---

## Section 7 — Anti-Shortcut Mechanisms

These mechanisms prevent the implementer from claiming completion prematurely.

**Anti-Shortcut Verification (check each box when the mechanism is confirmed in place):**

- [ ] **1. No "Will Fix Later"**
Every file touched must be fully compliant with ALL gates before moving to the next file. No violations may be deferred.

- [ ] **2. Verification After EVERY Change**
After every file modification, run the tiered verification strategy defined in Section 8 (Test Running Strategy). At minimum:
```bash
cargo check --workspace                           # Type check (always)
cargo clippy --workspace -- -D warnings           # Lint check (always)
cargo nextest run -p <crate> --test <nearest>     # Adjacent tests (Track B single-file edits)
awk '!/^[[:space:]]*$/ && !/^[[:space:]]*(\/\/)/' <changed_file> | wc -l  # LOC count
```
For Track A step completions and phase boundaries, run the full workspace test suite or `scripts/test-lanes.sh all-deterministic` per the Section 8 table. If ANY step fails, the change is NOT complete. Fix it before proceeding.

- [ ] **3. Progress Manifest (JSON)**
Maintain a `docs/refactoring-progress.json` file tracking every file processed:

```json
{
  "baseline_loc": 124979,
  "current_loc": null,
  "reduction_pct": null,
  "phase": "track-a",
  "track_a_complete": false,
  "track_b_files_processed": [],
  "track_b_files_remaining": 189,
  "violations": {
    "found": 0,
    "fixed": 0,
    "deferred": 0
  },
  "gates": {
    "g1_clippy": null,
    "g2_tests": null,
    "g3_no_oversized": null,
    "g4_no_unwrap": null,
    "g5_no_unjustified_allow": null,
    "g6_no_json_value": null,
    "g7_no_as_casts": null,
    "g8_loc_reported": null
  },
  "files": [
    {
      "path": "crates/harness-core/src/lib.rs",
      "processed": true,
      "loc_before": 922,
      "loc_after": null,
      "violations_found": ["V2: UnwrapOrAbort duplication"],
      "violations_fixed": [],
      "size_ok": false,
      "notes": ""
    }
  ]
}
```

- [ ] **4. No Skipping Files**
Every non-test Rust source file must be processed, even if "already clean." The implementer must run the YAGNI checklist on every file and record the result in the progress manifest.

- [ ] **5. No Deferring Violations**
If a violation is found during file processing, it must be fixed in the current iteration. No "TODO" comments, no "will fix in next pass."

- [ ] **6. SIZE_OK Requires Justification**
Any file >250 pure LOC must have a `// allow: SIZE_OK — <specific reason>` comment at the top of the file. The reason must be specific:
- ✅ `// allow: SIZE_OK — pure data table (command palette key bindings)`
- ✅ `// allow: SIZE_OK — indivisible state machine (coordinator turn phases)`
- ❌ `// allow: SIZE_OK — too complex to split` (not specific enough)
- ❌ `// allow: SIZE_OK — don't want to` (not a reason)

- [ ] **7. Type-Safety LOC Neutrality**
Type-safety improvements (newtypes, typed structs, safe casts) are REQUIRED by the programming skill but ADD LOC. To reconcile with the LOC reduction target:
- Type fixes are permitted only if the net pure LOC delta for the affected file is ≤0
- If a type fix adds 10 lines, the implementer must find 10+ lines of reduction in the same file or file group (dead code, duplication, consolidation)
- Type fixes that cannot be made LOC-neutral must be documented in the progress manifest with justification

**How to find offsetting reductions when a type fix adds LOC:**
1. **Dead validation code** — Look for runtime type checking that the newtype makes unnecessary (e.g., `if value.is_string()` guards, `serde_json::Value` normalization). These become dead code once types are correct.
2. **Duplicated logic** — Search for the same pattern repeated in the same file. Consolidating duplicates reduces LOC.
3. **Verbose error handling** — Look for multi-line `match` arms that can be simplified with `?` or `.map_err()`. Shorter error handling offsets newtype additions.
4. **Over-abstraction** — Look for traits, generics, or config structs that serve only one caller. Inlining them reduces LOC.
5. **Redundant comments** — Look for comments that restate what the code does (e.g., `// increment counter` above `counter += 1`). Delete them.
6. **Expand scope if needed** — If no reduction is findable in the same file, expand to the file's module (same directory). The LOC budget is per-file-group, not strictly per-file.

- [ ] **8. Full Test Suite Must Pass**
No test may be deleted, skipped, or weakened. If a test fails after a change:
1. The change broke behavior — fix the change, not the test
2. If the test was wrong (testing implementation, not behavior), fix the test AND document why
3. Never delete a failing test to "unblock" — that's deleting a bug report

- [ ] **9. Completion Requires ALL Gates + Exhaustion Declaration**
Completion requires ALL of:
1. All 8 gates pass (G1-G8)
2. All non-test files have been processed (manifest `track_b_files_remaining == 0`)
3. A full sweep finds zero new violations
4. No further LOC reduction is findable (documented exhaustion — the implementer has tried every reduction strategy and found no more removable code)
5. Actual reduction % reported in manifest

**50% is NOT a blocking gate.** The agent terminates when gates pass + exhaustion checklist complete, NOT when 50% is hit.

### 10. Rollback and Escalation Strategy

If a change breaks tests and the implementer cannot fix it:

1. **Attempt 1:** Fix the change (not the test). Re-run tests.
2. **Attempt 2:** Try a materially different approach. Re-run tests.
3. **Attempt 3:** Revert the change entirely with `git checkout -- <file>`. Document the blocker in the progress manifest under a `"blocked"` key with the file path, the violation attempted, and the failure reason.
4. **Move on:** Proceed to the next file/item. Blocked items are revisited in a final sweep after all other work is complete.
5. **Final sweep:** Re-attempt blocked items with fresh context. If still blocked after 3 more attempts, document as a permanent blocker with root cause analysis.

**A blocked item does NOT count as complete.** The progress manifest must track blocked items separately. The loop cannot terminate with blocked items unless they are documented as permanent blockers with root cause analysis.

### 11. Resume Protocol (After Context Reset)

If the implementer's session restarts (context window reset, crash, new session):

1. **Read the progress manifest:** `docs/refactoring-progress.json`
2. **Find the last completed step:** The last `- [x]` checkbox in this PRD corresponds to the last completed entry in the manifest
3. **Resume from the next unchecked box:** Do NOT re-do completed work. The manifest's `files` array records which files have been processed.
4. **Verify state:** Run `cargo check --workspace && cargo clippy --workspace -- -D warnings` to confirm the codebase is in a compilable state before resuming.
5. **Re-read this PRD:** Reload the `shared/programming` and `shared/refactor` skills before resuming work.
6. **Update the manifest:** Set `session_resumed_at` timestamp in the manifest.

**The progress manifest is the source of truth for resume.** If the manifest is missing or corrupted, the implementer must re-run the baseline command (Section 0) and re-audit all files from scratch.

---

## Section 8 — Risk Mitigation

### Functionality Preservation
The test suite (114K+ LOC of tests) IS the functionality-preservation gate. Any change that breaks a test is rejected. The implementer must run the full test suite after every change.

### Test Running Strategy (when to run what)

Running the full workspace test suite after every single file change is expensive on a 762-file codebase. Use this tiered strategy:

| After what change | Run what | Command |
|---|---|---|
| Single file edit (Track B) | Adjacent tests only | `cargo nextest run -p <crate> --test <nearest_test>` |
| Track A step completion | Full workspace tests | `cargo nextest run --workspace` |
| Phase boundary (Phase 1→2, 2→3) | Full deterministic lane | `scripts/test-lanes.sh all-deterministic` |
| Before declaring completion | Full deterministic lane + all critical lanes | `scripts/test-lanes.sh all-deterministic` + every lane in the table below |

**Rule:** If adjacent tests pass but you're unsure whether the change affects other crates, run `cargo check --workspace` first (fast). If it compiles, run the full workspace test suite before committing.

### Critical Test Lanes (MUST pass at every checkpoint)

| Test Lane | What it protects | Command |
|-----------|-----------------|---------|
| Coordinator invariants | Event/replay correctness, permissions, compaction | `cargo nextest run -p harness-core --test coord_test` |
| Event schema | Event schema v1 and append-only envelopes | `cargo nextest run -p harness --test event_docs_reference_test` |
| Config schema | Public config contract stability | `cargo nextest run -p harness --test config_schema_cli_test` |
| Provider protocol | Streaming, redacted metadata, cassette replay | `cargo nextest run -p harness-providers` |
| Tool schema parity | Native tool surface and schema parity | `cargo nextest run -p harness-tools --test native_tool_parity_matrix_test` |
| TUI rendering | App state, view models, renderer, shell geometry | `cargo nextest run -p harness-tui --test deterministic_render_test` |
| Testkit evidence | Fakes, simulation evidence, visual/PTY helpers | `cargo nextest run -p harness-testkit --test simulation_validator_test` |
| Full deterministic lane | All deterministic tests | `scripts/test-lanes.sh all-deterministic` |

### Specific Risk Areas

| Area | LOC | Risk if cut | Protection |
|------|-----|-------------|------------|
| harness-tui (36%) | 48K | Visual regressions, broken keybindings, lost overlays | TUI snapshot tests MUST pass |
| harness-core/coord (11%) | 14.5K | Event/replay bugs, permission bypass, compaction failures | `coord_test` + event replay tests MUST pass |
| harness-tools (17%) | 22.5K | Tool schema parity breaks, path safety bypass | `native_tool_parity_matrix_test` MUST pass |
| harness-core/auth | ~2K | Auth bypass, credential leak | Auth tests + redaction tests MUST pass |
| harness-core/config | ~3K | Public contract break, downstream consumer failure | `config_schema_cli_test` MUST pass |
| harness-providers (3%) | 4.3K | Provider metadata leak (raw requests/responses persisted) | Provider redaction tests MUST pass |

---

## Section 9 — Atomic Commit Strategy

- One commit per logical change (one Track A step, or one Track B file/group)
- Commit message format: `refactor(<area>): <what changed> — <LOC delta>`
- Examples:
  - `refactor(core): consolidate UnwrapOrAbort to single definition — -75 LOC`
  - `refactor(tools): add Tool derive macro to eliminate boilerplate — -630 LOC`
  - `refactor(tui): split ui_chrome.rs by UI component — -0 LOC (reorganized)`
- NEVER commit with failing tests
- NEVER squash unrelated changes
- Run `cargo clippy + cargo nextest` before EVERY commit

---

## Section 10 — Loop Termination Protocol

The implementer loops until ALL of the following are true:

- [ ] **1. All 8 gates pass** (G1-G8, verified by the commands in Section 6)
- [ ] **2. All non-test files processed** (manifest `track_b_files_remaining == 0`)
- [ ] **3. Full sweep finds zero new violations** (re-scan all files, confirm no violations)
- [ ] **4. No further LOC reduction findable** (documented exhaustion — the implementer has tried every reduction strategy from Section 4 and found no more removable code)
- [ ] **5. Actual reduction % reported** in the progress manifest

### What "No Further Reduction Findable" Means

The implementer has:
- [ ] Completed all Track A cross-cutting changes
- [ ] Processed all 189 oversized files in Track B
- [ ] Run the YAGNI checklist on every non-test file
- [ ] Attempted every reduction strategy (dedup, dead code removal, consolidation, macro extraction)
- [ ] Found no more code that can be removed without losing functionality
- [ ] Documented this exhaustion in the progress manifest

**Exhaustion Verification Checklist (ALL must be checked before declaring exhaustion):**

- [ ] **Ran `cargo-machete`** — no unused dependencies found (or all findings documented as intentional)
- [ ] **Ran `cargo-udeps`** (nightly) — no unused crate features found (or all findings documented)
- [ ] **Searched for duplicated patterns** — `grep -rn` for repeated code blocks across files; no removable duplication found
- [ ] **Searched for dead validation code** — reviewed all `serde_json::Value` normalization, runtime type checks, and re-validation of typed values; all removable code removed
- [ ] **Searched for boilerplate** — reviewed all trait impls, method signatures, and repeated patterns; no macro-extractable boilerplate remains
- [ ] **Searched for over-abstraction** — reviewed all traits with ≤1 production impl (excluding DI traits); no collapsible abstractions remain
- [ ] **Searched for inlineable helpers** — reviewed all functions called from exactly one site; all single-use helpers inlined or justified
- [ ] **Searched for redundant verification** — reviewed all post-destructive-action checks (delete then query, insert then select); all redundant checks removed
- [ ] **Searched for negative-form names** — reviewed all boolean variables/functions; all negative-form names renamed to positive
- [ ] **Re-ran all 8 gates (G1-G8)** — all pass with zero violations
- [ ] **Re-measured LOC** — current LOC recorded in manifest; reduction % calculated and reported

### What Does NOT Terminate the Loop

- "I've done a lot of work" — completion requires gates + exhaustion, not effort
- "50% is too hard" — 50% is aspirational, not blocking; but ALL opportunities must be exhausted
- "This file is fine" — every file must be processed, even if "already clean"
- "Tests are passing" — tests passing is necessary but not sufficient; all 8 gates must pass
- "I fixed the violations I found" — a full re-sweep must find zero NEW violations

---

## Appendix A — Programming Skill Rules Summary

The programming skill's iron rules that this PRD enforces:

1. **Type system is your proof system** — Make illegal states unrepresentable
2. **Parse, don't validate** — Untrusted input crosses a boundary once, parsed into a typed value
3. **One name = one concept** — Use newtypes for distinct semantic primitives
4. **Exhaustive variant matching** — `match` (compiler-enforced), never `if`/`else` on enums
5. **Trust framework guarantees** — No null checks for values the type system proves non-null
6. **No escape hatches** — No `unwrap`/`expect` outside `main`/tests, no `as` for narrowing, no `#[allow]` to silence real warnings
7. **250 LOC ceiling** — Files >250 pure LOC are DEFECTS (with SIZE_OK escape hatch)
8. **TDD** — Red → Green → Refactor. Behavior locked by tests.
9. **YAGNI** — The best code is the code never written. No helpers for one-off. No speculative abstraction.
10. **Post-write review loop** — 11-question self-review after every code change

---

## Appendix B — Verified LOC Reduction Opportunities

| Opportunity | Est. LOC Saved | Evidence | Risk |
|---|---|---|---|
| Tool boilerplate macro (V7) | ~630 | 42 tools × 15 LOC each | MEDIUM |
| UnwrapOrAbort consolidation (V2) | ~75-120 | 5 crates × 15-20 LOC each | LOW |
| map_err helper (V8) | ~89-178 | 89 call sites | LOW |
| Dead validation code in config (V3) | ~280-624 | config/public.rs Value normalization | MEDIUM |
| File splitting dead-code exposure | ~2,000-3,000 | Speculative — splitting reveals removable code | MEDIUM |
| TUI module consolidation | ~1,000-2,000 | 18 transcript files, overlapping render logic | HIGH |
| Clone reduction | ~200-300 | 2,694 non-test clones, ~10% genuinely removable | LOW |
| **TOTAL GROSS** | **~4,274-6,852** | **3.4% - 5.5% of 124,979** | |

### Type-Safety Additions (REQUIRED, adds LOC)

| Work Item | Est. LOC Added |
|---|---|
| 6+ newtype definitions (V4) | +80-120 |
| Call site conversions (V4) | +300-600 |
| Typed config structs (V3) | +150-200 |
| Safe cast replacements (V5) | +30 |
| Exhaustive match rewrites | +20-50 |
| **TOTAL ADDED** | **+580-1,000** |

### Net LOC Impact
- Gross reduction: ~4,274-6,852 LOC (3.4-5.5%)
- Type-safety additions: +580-1,000 LOC
- **Net reduction: ~3,694-5,852 LOC (3.0-4.7%)**
- 50% target (62,490 LOC) is aspirational — the gap between achievable (~4%) and target (50%) is the honest reality of a clean, well-maintained codebase.

---

## Appendix C — Provenance

This PRD was produced through the `/hyperplan` adversarial multi-agent planning process:

1. **Team created:** 5 hostile category members (quick-scanner=unspecified-low, arch-auditor=unspecified-high, type-system-hawk=ultrabrain, prd-architect=artistry, loc-feasibility=deep)
2. **Round 1:** Each member independently analyzed the codebase from their angle
3. **Round 2:** Each member ruthlessly attacked the other 4's findings (gaps, wrong assumptions, missed violations, flawed logic)
4. **Round 3:** Each member defended, refined, or conceded attacks on their findings
5. **Distillation:** Lead synthesized surviving insights into a structured bundle
6. **Plan agent:** Verified all claims independently, produced the PRD structure
7. **User decisions:** Confirmed aspirational 50% target + two-track design

**Key concessions made during adversarial review:**
- quick-scanner's "clippy compile failure" claim was FALSE (clippy passes on stable)
- arch-auditor's "37 config structs" was WRONG (actually 5)
- arch-auditor's "4 independent output-preview implementations" was OVERSTATED (1 impl + 3 consumers)
- type-system-hawk's "+40 LOC for type fixes" was UNDERCOUNTED (actual: +580-1,000)
- loc-feasibility's "dead code: effectively ZERO" missed ~280-624 LOC of semantically dead validation code
- prd-architect's baseline of 147,332 was INFLATED (corrected to 133,922)
- loc-feasibility's "15% realistic target" was UNSUPPORTED by itemized wins (corrected to 3-5%)

**Verified facts (all members agree):**
- Baseline non-test pure LOC: 124,979 (corrected from 133,922 — original command had overly broad "test" filter and did not strip #[cfg(test)] modules)
- Non-test source files: 373
- Oversized files (>250 pure LOC): 189
- UnwrapOrAbort duplicated in: 5 crates
- `as` casts in non-test code: 49
- `#[allow]` in non-test code: 14 (12 removed by UnwrapOrAbort consolidation)
- Newtypes existing: 1 (ProviderId)
- Clippy passes: YES (stable, `-D warnings`)
- Dead code (`#[allow(dead_code)]`): 0
- 50% LOC reduction without functionality loss: INFEASIBLE (realistic: 3-5% net)
