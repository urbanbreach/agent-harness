# Plan 008: Preserve authored permission-rule order through configuration loading

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-core/src/config.rs crates/harness-core/src/config/loader.rs crates/harness-core/src/config/permission_order.rs crates/harness-core/src/config/public.rs crates/harness-core/src/config/public/normalization.rs crates/harness-core/src/config/public/agents.rs crates/harness-core/src/config/tests.rs crates/harness-core/src/config/tests/permission_order_test.rs crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs docs/permissions/permissions.md`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 7 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: plan 002: Preserve inherited permissions across configuration layers (`plans/002-preserve-inherited-config-permissions.md`); [prerequisite issue](https://github.com/urbanbreach/agent-harness/issues/225)
- **Category**: security
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/231

## Execution evidence (2026-09-20)

- Drift review: plan 002 is implemented in `efe71a16`; its explicit-field merge, alias handling, per-layer reference expansion and inheritance tests remain intact. Later effective-target changes only adjusted existing coordinator test call signatures and added permission-guide paragraphs; those changes were preserved.
- Implementation: a private Serde visitor collects only permission selector order from raw JSON5. Root and named-agent paths carry that metadata through alias folding and layer merging. Public pattern maps stay maps until the merged configuration is expanded; overridden raw keys move last, scalar/legacy-array replacements clear stale order, and missing fields inherit. Value/order disagreement fails closed. The evaluator, public schema types, Cargo manifests/lockfile, request identities and durable serialization code are unchanged.
- Regression proof: `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order)'` failed before the fix: authored `"git status": "deny", "*": "allow"` incorrectly produced Deny instead of Allow.
- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order) | test(permission_rule) | test(layer_permissions)'` → **15 passed**, non-empty selection. The raw-text table covers all five selector kinds, the shell alias, root/named-agent scopes, string/file/context/content loaders, layered overrides and omissions, scalar replacements, duplicate/raw selector keys, nested alias precedence, legacy arrays and model-only agent overlays. Existing coordinator denial assertions remain intact with corrected fixture order.
- `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(config::tests::) | test(permission_flow_rule_tests)'` → **111 passed**.
- `cargo nextest run --profile ci --locked --offline -p harness --test config_schema_cli_test` → **54 passed** in a temporary detached checkout of `bff719c7` with byte-identical issue code/tests applied and the committed example config. `CARGO_TARGET_DIR` reused the main checkout's build cache. No schema snapshots or source fixtures were changed.
- Schema environment evidence: the original working-tree run had **53 passed, 1 failed** because the pre-existing `harness.jsonc` edit changes the model/provider expected by `root_runtime_example_uses_canonical_public_keys`. The first isolated run had **39 passed, 15 failed** because doctor tests assume an existing `.agent-harness` parent relative to the test crate. Creating only the empty temporary `crates/harness/.agent-harness` directory made all 54 pass. The operator's config was never rewritten or staged; no sessions or startup probe files were created. These local setup differences do not block the verified commit candidate.
- `cargo check --workspace --locked --offline` → exit 0.
- `cargo fmt --all -- --check` → exit 0.
- `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` → exit 0.
- `git diff --check` and `git diff --cached --check` → exit 0. Changed-path and checksum audits confirm only allowed issue paths changed; pre-existing files are preserved except this plan's authorized execution record and its index entries. The commit stages only issue work, leaving unrelated working-tree/index-document edits outside the commit.

## Why this matters

The documented rule contract is last match wins, but JSON value parsing and PublicRulePermissionValue::Rules currently sort object keys. An authored catch-all can therefore move ahead of a specific rule and reverse the effective decision. Preserve order from input text through layer merge and typed selector expansion; keep the evaluator itself unchanged.

## Current state

- [Cargo.toml:27](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/Cargo.toml#L27) — serde_json currently has no preserve_order feature; order is lost before typed deserialization.
- [crates/harness-core/src/config/loader.rs:325](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/loader.rs#L325) — JSON5 is first parsed into serde_json::Value.
- [crates/harness-core/src/config/public.rs:115](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/public.rs#L115) — Public pattern maps use BTreeMap.
- [crates/harness-core/src/config/public.rs:297](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/public.rs#L297) — Rule expansion iterates the map into a Vec.
- [crates/harness-core/src/config/public/agents.rs:315](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/public/agents.rs#L315) — Named-agent permissions use the same selector expansion and need their own source-order metadata.
- [crates/harness-core/src/config.rs:1081](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config.rs#L1081) — Layer merge must respect later ordering for overridden rule entries.
- [crates/harness-core/src/coord/permission.rs:84](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/permission.rs#L84) — Request identity hashes serialized JSON arguments; enabling order preservation globally could change identities outside configuration.
- [crates/harness-core/src/perm/ruleset.rs:135](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/perm/ruleset.rs#L135) — Evaluator already implements last-match semantics.
- [crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs:4](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs#L4) — This fixture currently writes the specific deny before the catch-all allow but expects the deny; update its authored order, not its intended policy assertion.
- [crates/harness-core/src/perm/tests.rs:262](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/perm/tests.rs#L262) — Existing typed-vector policy test is an assertion exemplar, not coverage of parsing order.

[crates/harness-core/src/config/public.rs:112](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/public.rs#L112):

```rust

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PublicRulePermissionValue {
    Mode(PermissionMode),
    Rules(BTreeMap<String, PermissionMode>),
}
```

[crates/harness-core/src/config/public.rs:297](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/public.rs#L297):

```rust
fn public_selector_rules(
    kind: &str,
    value: Option<PublicRulePermissionValue>,
) -> Result<Vec<PermissionSelectorRule>, ConfigError> {
    match value {
        Some(PublicRulePermissionValue::Rules(rules)) => rules
            .into_iter()
            .map(|(selector, mode)| {
                Ok(PermissionSelectorRule {
                    selector: public_permission_selector(kind, &selector)?,
                    mode,
                })
            })
            .collect(),
        Some(PublicRulePermissionValue::Mode(_)) | None => Ok(Vec::new()),
    }
```

[crates/harness-core/src/perm/ruleset.rs:140](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/perm/ruleset.rs#L140):

```rust
    permission: &str,
    pattern: &str,
    rulesets: impl IntoIterator<Item = impl AsRef<[PermissionRule]>>,
) -> PermissionRule {
    let merged: Vec<PermissionRule> = rulesets
        .into_iter()
        .flat_map(|set| set.as_ref().to_vec())
        .collect();
    let match_rule = merged.into_iter().rev().find(|rule| {
        wildcard_match(permission, &rule.permission) && wildcard_match(pattern, &rule.pattern)
    });

```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-core/src/config/tests/discovery_schema_test.rs:546](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/config/tests/discovery_schema_test.rs#L546):

```rust
        r#"{ bash: "allow", edit: "allow" }"#,
    ));
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("content overlay should load: {error}"))
        .unwrap_or_abort();
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Allow
    ));
```

Relevant design contract: [docs/permissions/permissions.md:92](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/docs/permissions/permissions.md#L92).

The permissions guide explicitly states, “Rules are ordered; last match wins.” Preserve scalar permission behavior, the public JSON object shape, generated schema meaning, child/shared policy ceilings and the evaluator's deny/ask/allow contract. Plan 002's absence/inheritance semantics remain intact: only explicit layer fields override inherited values, defaults apply after the merge, and supported aliases share a canonical representation. Keep source ordering local to permission configuration. Do not enable serde_json's workspace-wide preserve_order feature or change request/event serialization.

## Commands you will need

Run commands from the repository root. Use the existing installed toolchain, serde and JSON5 dependencies; no dependency or manifest changes are needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order) \| test(permission_rule) \| test(layer_permissions)'` | Selected tests pass after the repair; selection must not be empty. |
| Schema contract | `cargo nextest run --profile ci --locked --offline -p harness --test config_schema_cli_test` | Existing schema/CLI cases pass without a public schema change. |
| Configuration compatibility | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(config::tests::) \| test(permission_flow_rule_tests)'` | Existing configuration and policy cases pass. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-core/src/config.rs`
- `crates/harness-core/src/config/loader.rs`
- `crates/harness-core/src/config/permission_order.rs` — optional private helper module for narrowly scoped source-order collection; do not build a general JSON parser.
- `crates/harness-core/src/config/public.rs`
- `crates/harness-core/src/config/public/normalization.rs`
- `crates/harness-core/src/config/public/agents.rs`
- `crates/harness-core/src/config/tests.rs`
- `crates/harness-core/src/config/tests/permission_order_test.rs` — create only this focused test module.
- `crates/harness-core/src/coord/tests/permission_flow_rule_tests.rs`
- `docs/permissions/permissions.md`

Administrative updates are limited to execution status/evidence in `plans/008-preserve-authored-permission-order.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-008-preserve-authored-permission-order`.
- Keep this repair in one logical change; if instructed to commit, use `fix(config): retain authored permission rule precedence`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Add raw-input ordering cases

Create config/tests/permission_order_test.rs and register it. Parse literal JSON5 strings, not json! maps, for a specific rule and an overlapping wildcard in both authored orders. Use the same file names/actions in both cases so only order changes. Cover global and named-agent permission maps, then a later layer that changes an already present rule. Assert the effective PermissionPolicy decision, not merely Vec order. Exercise load_config_from_str, file loading and a content overlay through the existing fixtures. Keep one scalar-permission control and plan 002's sparse-layer cases.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order)'` → At least one opposing-order pair fails on the baseline, proving that input order is currently lost.

### Step 2: Collect permission order directly from the source text

Keep the existing validated serde_json::Value pipeline. Alongside it, use JSON5 deserialization with a small serde map visitor to collect the authored selector keys into Vec<String> before a sorted Value or BTreeMap can discard their order. Collect only the five selector-bearing permission kinds (bash/shell, edit, task, read and external_directory), separately for root permissions and each public agent. A narrow envelope can ignore unrelated values through serde::de::IgnoredAny; this is source-order metadata, not a second configuration model. A second parse of the same in-memory source is acceptable; do not reread files or duplicate reference expansion. Scalar modes carry no selector order. Keep the existing typed validation authoritative and propagate errors instead of treating malformed metadata as an empty policy.

Use the raw selector spelling as the metadata key; order is applied before selector normalization. Follow existing duplicate-key acceptance and alias precedence rather than introducing a new format rule. If duplicate keys are accepted with the last value winning, retain their last occurrence position. Legacy internal rule arrays already have explicit order and must keep it. The private metadata may live in loader.rs or the allowed permission_order.rs module if that makes the visitor readable. Do not add a dependency, change public schema types, or enable global JSON order preservation.

**Verify:** `cargo check --workspace --locked --offline` → Compilation succeeds using the unchanged dependency graph; the normal config parser still performs validation and reference expansion.

### Step 3: Carry the order through merging and selector expansion

Thread the private metadata through both the single-document path and the layer path established by plan 002. Merge it with the same canonical scopes, alias precedence and replacement decisions as the explicit permission values. Retain the relative order of inherited entries and append later layer entries in their authored order; remove an overridden pattern from its earlier position before appending it. A scalar or other replacement that removes a map must also remove its stale ordering metadata. Do not derive precedence from iteration of the merged Value.

In public_selector_rules, consume validated map entries by the corresponding raw-key order, then convert each selector and produce the existing Vec<PermissionSelectorRule>. The BTreeMap may remain a value lookup; it must no longer decide priority. Pass each named agent's order through public_agent_to_profile and translate_public_profile_permissions as well as the root normalization path. Ensure every validated entry is consumed once; a metadata/value mismatch must be an error, never a sorted-order fallback or silently dropped rule. Preserve preexisting legacy rule-array order and scalar defaults. Keep PermissionPolicy and shared deny ceilings unchanged.

Reorder the existing bash/task fixture literals whose intended specific denial was relying on alphabetic sorting; retain their denial assertions.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order) | test(permission_rule) | test(layer_permissions)'` → Authored order and cross-layer order decide policy; existing intended denies and sparse inheritance still pass.

### Step 4: Check compatibility and document precedence

Document this as a correction to the already-stated last-match contract, including the migration consequence for configurations relying on the bug. Extend the ordering table with the accepted shell alias and a map replaced by a scalar to protect the metadata/value merge boundary. Check the generated schema through the existing CLI contract test. Keep Cargo manifests/lockfile, request identities, durable event digests and unrelated JSON output unchanged; do not accept new snapshot values to hide a serialization change.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test config_schema_cli_test` → The public schema/CLI contract is unchanged. The configuration-compatibility command in the table also passes with the fixture's intended deny assertions retained.

### Step 5: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

One table-driven public-loader test covers the order-sensitive cases, including global and agent scopes, supported aliases and an overridden rule in a later layer. Reuse its raw-text fixtures across the existing string/file/content entry points; add only cases needed to detect a path losing metadata. Existing evaluator tests already cover ordered Vec behavior; duplicating them would miss the parser defect. Schema and configuration compatibility checks protect the public format. No workspace-wide serialization change is part of this repair.

## Done criteria

All must hold:

- [x] Opposite raw authored orders produce the documented opposite last-match decisions.
- [x] Later-layer pattern overrides occur after inherited rules without resetting omitted permissions.
- [x] The public configuration schema stays an object/scalar union with the same accepted action values.
- [x] Source-order handling stays inside configuration; dependency features, request identities and durable serialization are unchanged.
- [x] Root and named-agent rules retain order through string, file and content-overlay loading; stale metadata cannot survive a scalar replacement.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(permission_order) | test(permission_rule) | test(layer_permissions)'` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- Plan 002 is incomplete or the current merge no longer matches its explicit-field contract.
- Order is reconstructed from an already sorted serde_json::Value/BTreeMap, any loader drops the metadata, or the proposed fix changes the evaluator to specificity-first.
- The approach requires a workspace dependency-feature change, a general JSON AST/parser, or changes to durable serialization/request identities. Report the boundary and revise the local approach.
- Alias or layer precedence cannot keep order metadata consistent with the accepted permission values; do not silently sort, omit rules or invent new precedence.

## Maintenance notes

Treat map order as part of the permission configuration contract. Tests must start with raw text; constructing their input through a sorted map defeats the test. Keep order-sensitive maps separate from places that require canonical sorted serialization.
