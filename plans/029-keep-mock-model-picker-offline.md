# Plan 029: Keep the mock TUI model picker offline

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness/src/tui.rs crates/harness/src/tui/tests.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** DONE — independent verification PASS (2026-09-20)
- **Issue:** [#252](https://github.com/urbanbreach/agent-harness/issues/252)
- **Priority:** P2
- **Effort:** S
- **Risk:** LOW
- **Depends on:** none
- **Category:** bug
- **Audit finding:** 28 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

Mock TUI bootstrap shares the live model-picker initialization path, which spawns ProviderCatalog::from_env and can refresh a remote catalog. Demo startup can therefore use the network despite its mock/offline contract. Select the embedded catalog before any live loader is spawned.

## Current state

`crates/harness/src/tui.rs:161` — Model picker startup spawns the environment-backed catalog loader.

```rust
fn set_pending_connect_providers_from_config(config: Option<&harness_core::config::HarnessConfig>) {
    use harness_tui::app::set_pending_connect_providers;

    let registry = AuthPluginRegistry::with_builtins();
    let catalog = std::thread::spawn(harness_core::provider_catalog::ProviderCatalog::from_env)
        .join()
        .ok()
        .and_then(Result::ok);
    set_pending_connect_providers(connect_provider_options(
        config,
        &registry,
        catalog.as_ref(),
    ));
```

`crates/harness/src/tui.rs:362` — Mock and live initialization share the model-picker setup.

```rust
        ResolvedTuiMode::Mock { settings } => {
            runtime.block_on(run_interactive_mode(&cmd, &settings, true))
        }
        ResolvedTuiMode::Scenario { settings, scenario } => {
            runtime.block_on(run_live_mode(&cmd, &settings, scenario))
        }
    };

    match run_result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(stderr, "tui failed: {err}");
            ExitCode::from(1)
        }
    }
}

async fn run_interactive_mode(
    cmd: &TuiCommand,
    settings: &LiveSettings,
    demo_mode: bool,
) -> Result<(), String> {
    profile_handoff("interactive_mode.begin");
    fs::create_dir_all(&settings.session_dir)
        .map_err(|err| format!("failed to create session dir: {err}"))?;

    set_pending_connect_providers_from_config(settings.config.as_ref());
    let launch_selection = Arc::new(Mutex::new(
```

## Conventions and exemplar

Mock/demo uses ProviderCatalog::from_embedded directly. Live behavior keeps cache and refresh semantics. Preserve existing configured model visibility and options; do not change global environment variables or disable networking for all modes.

`crates/harness/src/tui/tests.rs:255` — The existing embedded-catalog provider-options test supplies a deterministic fixture.

```rust
fn connect_provider_options_seed_from_models_dev_catalog() {
    let catalog =
        harness_core::provider_catalog::ProviderCatalog::from_embedded().unwrap_or_abort();
    let registry = AuthPluginRegistry::with_builtins();

    let providers = connect_provider_options(None, &registry, Some(&catalog));

    assert!(providers.len() > registry.providers().len());
    assert!(providers
        .iter()
        .any(|provider| provider.id.as_str() == "anthropic"));
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(connect_provider_options) \| test(mock_mode_ignores_discovered_cwd_config) \| test(mock_model_picker)'` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness/src/tui.rs`
- `crates/harness/src/tui/tests.rs`

Administrative updates to `plans/029-keep-mock-model-picker-offline.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-029-keep-mock-model-picker-offline` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Select the catalog source at the mock/live boundary

Thread the existing demo/mock flag to model picker initialization and branch before spawning the environment loader. Use from_embedded for mock mode and the current from_env path for live mode. Keep the async provider-option update path where needed.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) | test(mock_model_picker)'` → Existing provider-option and mock configuration tests pass.

### Step 2: Prove mock startup cannot invoke the live loader

Add the smallest private injected FnOnce loader seam at that boundary. In one behavioral test named mock_model_picker, make the live loader observable or fail if called: mock initialization must still populate embedded options without invoking it; the live control must invoke it. Avoid an environment-wide network-disable flag, since that would mask an accidental call.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) | test(mock_model_picker)'` → The mock case records zero live-loader calls and nonempty embedded options; the live control records one invocation.

### Step 3: Run the focused TUI bootstrap checks

Run the listed library selection and workspace compilation. Preserve replay/readiness no-network behavior and do not launch a real provider request as evidence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) | test(mock_model_picker)'` → All selected checks pass.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness --lib -E 'test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) | test(mock_model_picker)'` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Mock model-picker initialization populates embedded options without invoking the environment-backed loader.
- [x] The live control still invokes the existing loader and model/provider selection tests pass.
- [x] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [x] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- The test prevents networking globally instead of proving the live loader was not called.
- A fix requires changing the remote catalog's cache policy for real launches.

## Maintenance notes

Choose offline data sources before task/thread creation. Plan 037 documents the resulting mock exception and ordinary live catalog behavior.

## Execution evidence — 2026-09-20

- The only model-picker bootstrap caller now passes its existing demo flag.
  Catalog selection happens before the live loader closure can spawn a thread:
  mock loads `ProviderCatalog::from_embedded`; live retains the existing
  `ProviderCatalog::from_env` thread/join and failure handling.
- Added one loader-boundary regression with an observable `FnOnce` closure.
  Mock returns embedded provider options with zero live-loader invocations;
  the live control invokes the closure exactly once. No network-disabling
  environment flag masks an accidental loader call.
- Final command: `cargo nextest run --profile ci --locked --offline -p harness
  --test prompt_cli_test --test cli_authority_matrix_cli_test --lib -E
  'binary(prompt_cli_test) | binary(cli_authority_matrix_cli_test) |
  test(connect_provider_options) | test(mock_mode_ignores_discovered_cwd_config) |
  test(mock_model_picker)'`: **42 passed**, including all three selected
  catalog/mock library cases.
- Final verification used a private reflink copy of the dependency cache after
  `cargo clean -p harness -p harness-core -p harness-providers -p harness-tools
  -p harness-tui -p harness-testkit --locked --offline` in that private target.
  This avoids cross-worktree workspace-artifact reuse observed in the shared
  target. Nextest run ID: `2cc378b6-d0de-44f2-bca4-3de6b345f40e`.
- Final Cargo commands used `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-cli-private`,
  `CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0.
- No renderer or layout changes; loader-boundary verification establishes the
  offline contract without terminal visual checks or real provider traffic.
- Scoped `cargo check` and `cargo clippy` were queued, then canceled before
  execution at the integrating operator's request. Integrated workspace checks
  and independent review remain the closure gate, and `plans/README.md` is left
  to that operator. Neither deferred check is claimed as passed here.

## Independent closeout — 2026-09-20

Independent agent `fix_provider_sessions` verified issue #252: **PASS**. The [combined verification record](2026-09-20-issue-closeout.md) records the attached commits, accepted checks, integration follow-ups and remaining global limitations.
