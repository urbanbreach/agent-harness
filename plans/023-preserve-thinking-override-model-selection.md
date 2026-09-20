# Plan 023: Preserve configured model selection when overriding thinking settings

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness/src/prompt.rs crates/harness/tests/prompt_cli/02_prompt_cli_model_variant_and_thinking_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — targeted checks pass; integrated lint/check and independent review pending.
- **Issue:** [#246](https://github.com/urbanbreach/agent-harness/issues/246)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 22 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

The CLI model override path reparses the configured model but omits the configured variant when only thinking or effort is supplied. This can silently change the effective model settings. Resolve the same configured selector and variant as normal bootstrap before applying the requested thinking change.

## Current state

`crates/harness/src/prompt.rs:781` — The override path supplies only cmd.variant to model selection.

```rust
            parse_cli_model_ref(model_ref)?
        } else {
            let profile = config.agents.get(profile_name).ok_or_else(|| {
                format!("unknown agent `{profile_name}` while resolving prompt model override")
            })?;
            parse_cli_model_ref(&profile.model_ref)?
        };

        let mut resolved = resolve_model_selection(
            config,
            &format!("{provider}:{model}"),
            cmd.variant.as_deref(),
        )
        .map_err(|err| err.to_string())?
        .primary;

        model_settings.variant = resolved.variant.clone();
```

`crates/harness/src/bootstrap.rs:415` — Normal selection retains the profile variant.

```rust
    for (profile_name, profile_cfg) in &cfg.agents {
        let model_selection =
            resolve_model_selection(cfg, &profile_cfg.model_ref, profile_cfg.variant.as_deref())
                .map_err(|err| {
                    format!(
                        "agent `{profile_name}` has invalid model selection `{}`: {err}",
                        profile_cfg.model_ref
                    )
                })?;
```

## Conventions and exemplar

Use the current model resolver; do not duplicate alias resolution or change configuration precedence. When --model is absent retain the configured selector and use CLI variant if present, otherwise the profile variant. An explicit new --model keeps its existing variant contract and must not blindly inherit a variant from the old model.

`crates/harness/tests/prompt_cli/02_prompt_cli_model_variant_and_thinking_test.rs:36` — Reuse the existing captured provider request assertions.

```rust
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Thinking: Drafting a careful answer."));
    assert!(stdout.contains("Hello world"));
    let requests = provider.requests();
    assert_eq!(requests[0].reasoning_effort.as_deref(), Some("low"));
    assert_eq!(requests[0].reasoning_summary.as_deref(), Some("auto"));
    assert_eq!(requests[0].text_verbosity.as_deref(), Some("low"));
}
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_model_override_records_selected_model_in_run_metadata() {
    // arrange
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --test prompt_cli_test -E 'test(part_02_prompt_cli_model_variant_and_thinking_test)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness/src/prompt.rs`
- `crates/harness/tests/prompt_cli/02_prompt_cli_model_variant_and_thinking_test.rs`

Administrative updates to `plans/023-preserve-thinking-override-model-selection.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-023-preserve-thinking-override-model-selection` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Extend the existing CLI request fixture

Add table-driven thinking-only and effort-only rows with a configured model alias and variant. Assert the captured provider request and recorded runtime selection preserve model, variant-derived verbosity/limits and unrelated settings. Keep an explicit --model row as the precedence control.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test prompt_cli_test -E 'test(part_02_prompt_cli_model_variant_and_thinking_test)'` → Thinking/effort-only rows expose the variant loss on the baseline.

### Step 2: Resolve the baseline selection before applying overrides

In prompt.rs retain the configured model selector verbatim when cmd.model is absent, resolving it with cmd.variant.or(profile.variant) through the existing resolver. Apply thinking/effort overrides to the resolved selection afterward. Preserve explicit-model behavior and the existing error path for invalid variants.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test prompt_cli_test -E 'test(part_02_prompt_cli_model_variant_and_thinking_test)'` → Both partial-override rows retain model/variant settings and change only their requested field.

### Step 3: Run the prompt CLI target

Run the focused module and full prompt CLI target to cover explicit model, alias, variant and request recording behavior.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test prompt_cli_test` → All selected cases pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --test prompt_cli_test -E 'test(part_02_prompt_cli_model_variant_and_thinking_test)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Thinking-only and effort-only overrides preserve configured alias/variant resolution.
- [x] Explicit --model/--variant precedence and invalid-selection errors remain covered and pass.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The resolver's alias contract differs from normal bootstrap; reconcile that contract rather than manually rewriting identifiers.
- A fix requires changing permission configuration or provider transport request schemas.

## Maintenance notes

Partial CLI overrides must start from the same resolved selection as a launch with no overrides.

## Execution evidence — 2026-09-20

- Implemented in isolated `codex/issues-cli-catalog` checkout based on `3d8e3d4f`.
  The baseline drift check found no changes to this issue's implementation files.
- The shared prompt override resolver now preserves the configured model-profile
  selector and chooses the CLI variant or configured profile variant before
  applying thinking/effort changes. An explicit model retains its existing
  parsing and does not inherit the configured variant.
- Extended the existing request/metadata test with thinking-only, effort-only,
  explicit-model/variant, explicit-model-without-variant, and invalid-variant
  checks. Requests and recorded metadata retain the expected model, variant,
  verbosity, and output limits.
- Baseline focused module: 10 selected, 9 passed and the regression failed with
  `--thinking`: `invalid model selector configured`. An initial test fixture
  omitted a required context limit; that fixture was corrected before this
  regression result.
- Final command: `cargo nextest run --profile ci --locked --offline -p harness
  --test prompt_cli_test --test cli_authority_matrix_cli_test --lib -E
  'binary(prompt_cli_test) | binary(cli_authority_matrix_cli_test) |
  test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) |
  test(mock_model_picker)'`: **42 passed**, including all 24 prompt CLI cases.
- Cargo commands used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure`,
  `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- Scoped `cargo check` and `cargo clippy` were queued, then canceled before
  execution at the integrating operator's request to avoid duplicate builds.
  Integrated workspace checks and independent review remain the closure gate;
  neither is claimed as passed here. `plans/README.md` is left to that operator.
