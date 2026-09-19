# Plan 004: Evaluate file permissions against effective workspace targets

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-core/src/path_selector.rs crates/harness-core/src/tool.rs crates/harness-core/src/coord/permission.rs crates/harness-core/src/coord/tool_execution.rs crates/harness-core/src/coord/tests.rs crates/harness-core/src/coord/tests/permission_flow_tests.rs crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs crates/harness-core/src/coord/tests/permission_path_tests.rs docs/permissions/permissions.md`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 3 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: None
- **Category**: security
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/227

## Why this matters

Permission selection and native file execution normalize paths differently. A supplied internal path can disappear from the policy selector list, and a symlink alias can be evaluated under a different name from its target. Use the same effective target semantics for policy decisions, sensitive-file asks and reusable grants while retaining the separate external-directory approval flow.

## Current state

- [crates/harness-core/src/path_selector.rs:22](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/path_selector.rs#L22) — Lexical selector normalization currently rejects every parent component.
- [crates/harness-core/src/coord/permission.rs:756](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/permission.rs#L756) — Rejected paths are silently omitted.
- [crates/harness-core/src/coord/permission.rs:106](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/permission.rs#L106) — Always-approve protects sensitive names using raw input rather than effective targets.
- [crates/harness-core/src/coord/tool_execution.rs:172](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tool_execution.rs#L172) — Coordinator builds the selectors before policy evaluation and grant handling.
- [crates/harness-core/src/tool.rs:305](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/tool.rs#L305) — Existing read resolver normalizes and canonicalizes execution paths, then applies external grants.
- [crates/harness-tools/src/hashline_apply.rs:319](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L319) — Existing-ancestor validation shows how creation targets are checked.
- [crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs:4](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs#L4) — Existing public tool-request policy test structure.

[crates/harness-core/src/path_selector.rs:22](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/path_selector.rs#L22):

```rust
fn normalize_relative_components(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (!parts.is_empty()).then(|| parts.join("/"))
```

[crates/harness-core/src/coord/permission.rs:756](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/permission.rs#L756):

```rust
fn insert_workspace_path_selector(
    workspace_root: &Path,
    raw_path: &str,
    paths: &mut BTreeSet<String>,
) {
    if let Some(path) =
        workspace_relative_path_from_maybe_absolute(workspace_root, Path::new(raw_path))
    {
        paths.insert(path);
    }
}

```

[crates/harness-core/src/coord/permission.rs:553](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/permission.rs#L553):

```rust
    if selectors.is_empty() {
        return evaluate(None);
    }
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-tools/src/hashline_apply.rs:319](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L319):

```rust
fn ensure_target_stays_within_workspace(
    workspace: &Path,
    resolved: &Path,
    input: &Path,
) -> Result<(), ToolError> {
    let Some(existing_ancestor) = nearest_existing_ancestor(resolved) else {
        return Err(ToolError::Execution(format!(
            "failed to resolve an existing parent for {}",
            input.display()
        )));
    };

    let canonical_ancestor = existing_ancestor.canonicalize().map_err(|err| {
        ToolError::Execution(format!(
            "failed to canonicalize resolved path ancestor {}: {err}",
            existing_ancestor.display()
        ))
    })?;

    if !canonical_ancestor.starts_with(workspace) {
        return Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: input.display().to_string(),
        });
    }

    Ok(())
}

fn nearest_existing_ancestor(path: &Path) -> Option<&Path> {
    let mut candidate = Some(path);
    while let Some(current) = candidate {
```

Relevant design contract: [docs/permissions/permissions.md:35](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/docs/permissions/permissions.md#L35).

The permissions guide states, “A permission allow does not bypass workspace path validation, shell safety parsing, or the doom-loop streak counter.” Paths outside the workspace use the distinct external_directory gate and call-scoped prefixes. Preserve that supported behavior; do not turn all absolute paths into unconditional denial. The coordinator remains the only authority, and grants must never override static denies. This is a policy-correctness repair, not a race-free filesystem sandbox.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_) \| test(always_approve)'` | Selected tests pass after the repair; selection must not be empty. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-core/src/path_selector.rs`
- `crates/harness-core/src/tool.rs`
- `crates/harness-core/src/coord/permission.rs`
- `crates/harness-core/src/coord/tool_execution.rs`
- `crates/harness-core/src/coord/tests.rs`
- `crates/harness-core/src/coord/tests/permission_flow_tests.rs`
- `crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs`
- `crates/harness-core/src/coord/tests/permission_path_tests.rs` — create only this focused test module.
- `docs/permissions/permissions.md`

Administrative updates are limited to execution status/evidence in `plans/004-align-file-permissions-with-targets.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-004-align-file-permissions-with-targets`.
- Keep this repair in one logical change; if instructed to commit, use `fix(permissions): check effective file targets before tool execution`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Add an execution-boundary regression

Add permission_path_tests.rs under coord/tests and register it in coord/tests.rs using the existing delegated test pattern. Through CoordinatorHandle::request_tool_call, compare a normal target, an equivalent internal parent-component path, and a Unix symlink alias to the same restricted target. Cover read and edit with a table, assert deny/ask before ToolCallStarted, and retain an allowed-file control. Use typed ordered policy rules in fixture setup so this test does not depend on the separate public-map ordering bug. Keep every target and symlink inside a temporary fixture.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_path)'` → Before the repair, at least an alias/equivalent-path case exposes policy disagreement; the allowed control remains valid.

### Step 2: Resolve request targets once and fail closed on invalid internal paths

Keep normalize_workspace_relative_path's pure configuration-selector contract unchanged. Add or narrow an existing crate-internal effective-target helper in path_selector.rs, reusing tool.rs normalization rather than maintaining another parent-component algorithm. Resolve the canonical workspace and the nearest existing ancestor, append only the unresolved leaf suffix, and return a typed result that distinguishes an internal target, an external target and invalid/unresolvable input. Use symlink_metadata while locating the ancestor so a dangling link is not mistaken for a normal absent directory. A missing creation leaf is valid; an unknown root or unresolvable ancestor is an error. In permission.rs, evaluate both the normalized requested internal name and its canonical internal target when they differ, combining decisions with the existing deny-then-ask behavior. Preserve the existing external-path collector/gate for external targets. Propagate invalid supplied input through tool_execution.rs before grants or execution rather than treating it as an empty selector list. Keep the genuine no-path case for tools that do not carry path selectors.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_path) | test(path_selector)'` → All equivalent internal targets receive the intended policy; invalid internal input cannot reach ToolCallStarted. Existing pure selector tests still pass.

### Step 3: Use effective targets for bypass and grant safety

Update permission_grant_matcher and always_approve_can_bypass to consume the validated path information, threading the workspace/context through their callers in permission.rs and tool_execution.rs as needed. Sensitive-target asks must remain promptable even through a harmlessly named alias and even with always-approve enabled. If multiple identities prevent a safe single-path reusable grant, keep the existing request-digest fallback; do not broaden it. Revalidate targets when resuming a pending permission so a changed alias does not silently reuse stale authorization. Extend the existing always-approve and grant tests; preserve external-directory and question special handling.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_) | test(always_approve)'` → All policy/grant cases pass, including static-deny precedence, sensitive alias asks, and explicitly approved external access.

### Step 4: Document and verify the resolution contract

Document that internal aliases cannot bypass target policy, while external targets still require their separate gate. Leave native file operation behavior, shell parsing and broad directory-search semantics unchanged. Run the existing native containment integration target to verify the coordinator change has not disabled legitimate routes or relaxed execution-side containment.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_workspace_edit_routing_test` → All existing native edit routing and symlink containment cases pass.

### Step 5: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

One coordinator behavior table should cover read/edit aliases and equivalent paths. Extend existing always-approve/grant coverage for the distinct bypass risk. Keep denied targets unchanged and assert no ToolCallStarted event; a selector-only unit test is insufficient. Use cfg(unix) for symlinks and retain platform-neutral normalization cases.

## Done criteria

All must hold:

- [x] The permission_path table verifies identical policy for effective internal targets and rejects invalid supplied paths before execution.
- [x] Sensitive target aliases remain promptable with always-approve enabled; static denies still beat grants.
- [x] Existing approved external-directory behavior and native containment tests pass.
- [x] The shared effective-target helper accepts missing internal leaves through validated ancestors; plan 006 can reuse that contract.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_) | test(always_approve)'` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- The implementation would remove external_directory approval support or treat a path-resolution error as Allow.
- A proposed fix depends on adding an OS sandbox, unsafe code, or a new path library.
- Revalidating pending requests requires a durable event/schema change rather than using current coordinator request state.

## Maintenance notes

New native path argument aliases must enter both authorization and execution routing. Do not use the pure configuration-selector normalizer for runtime targets. Plan 006 reuses the helper with a stricter restore policy that never authorizes external targets.

## Execution evidence (2026-09-19)

Reimplemented from `e5e41075` (issue #226), after saving and discarding the
abandoned source changes. The prior attempt's execution addenda were removed;
they do not describe this implementation.

- The coordinator regression was red on the baseline: `read
  missing/../secret/data` returned an accepted tool call despite a deny rule for
  `secret/*`.
- Runtime resolution reuses `normalize_workspace_target_path`, canonicalizes the
  nearest existing ancestor, and rejects dangling links and unavailable roots.
  Configuration-selector normalization is unchanged.
- Policy checks both internal names. Existing grant matchers carry a digest bound
  to effective targets; pending approvals compare it again after permission hooks.
  No new pending-state fields, durable schema, dependencies, or unrelated fixture
  repairs were needed.
- The table covers read/edit deny and ask, internal parent components, both
  directions of alias policy, missing leaves, allowed controls, and invalid
  supplied arguments. Further checks cover sensitive aliases with always-approve,
  stable and retargeted run grants, static denies, and stale ordinary,
  external-directory, and doom-loop approvals.
- The existing external-directory collector remains unchanged. An initial attempt
  to alter its alias handling made two native edit containment tests wait for
  approval; that scope expansion was removed. Existing native containment and
  supported explicitly approved external access pass.

Validation:

- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E
  'test(permission_) | test(always_approve) | test(path_selector) |
  test(external_directory)'`: 42 passed, non-empty selection.
- `cargo nextest run --profile ci --locked --offline -p harness-tools --test
  native_workspace_edit_routing_test`: 7 passed.
- `cargo check --workspace --locked --offline`: passed.
- `cargo clippy -p harness-core --all-targets --all-features --locked --offline --
  -D warnings`: passed after simplifying the regression table's nesting.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The full workspace test suite and unrelated repository-wide gates were not run.
Only the allowed issue files are included in the commit. Pre-existing config,
audit documents, and the untracked capture directory remain outside this change.
Plan 006 can reuse `effective_workspace_target`; an absent `relative` identifies
an external target, while an error is an unresolvable/invalid target.
