# OpenCode Auth-to-Model Provider Parity Progress

Source PRD: `docs/opencode-auth-model-parity-prd.md`.

## Baseline/reference audit

Read before coding:

- Repo guidance: root `AGENTS.md`, `crates/harness-core/AGENTS.md`, `crates/harness-providers/AGENTS.md`, `crates/harness-tools/AGENTS.md`, `crates/harness-tui/AGENTS.md`, `crates/harness-testkit/tests/AGENTS.md`, `.agent-harness/AGENTS.md`.
- OpenCode reference files: `inspirations/opencode/packages/opencode/src/server/routes/instance/httpapi/handlers/provider.ts`, `.../provider/auth.ts`, `.../auth/index.ts`, `.../cli/cmd/tui/component/dialog-provider.tsx`, `.../cli/cmd/tui/component/dialog-model.tsx`, `.../cli/cmd/tui/context/local.tsx`, `.../cli/cmd/tui/component/prompt/index.tsx`, `.../cli/cmd/run/footer.command.tsx`, `.../cli/cmd/run/runtime.ts`.
- Harness source map: `crates/harness-core/src/config.rs`, `crates/harness-core/src/auth.rs`, `crates/harness-core/src/auth/codex.rs`, `crates/harness-core/src/auth/copilot.rs`, `crates/harness/src/bootstrap.rs`, `crates/harness/src/models.rs`, `crates/harness/src/prompt.rs`, `crates/harness/src/tui.rs`, `crates/harness-tui/src/app.rs`, `crates/harness-tui/src/app/session_navigation.rs`.

## Acceptance/evidence ledger

| PRD area | Current status | Evidence / next proof |
| --- | --- | --- |
| Runtime built-in catalog | Implemented. `crates/harness/src/runtime_catalog.rs` composes OpenAI Codex and GitHub Copilot built-ins from the generated catalog/fallback seams, with explicit config precedence and provider filters. | `cargo test -p harness runtime_catalog -- --nocapture` passed runtime-catalog tests for Codex, Copilot, no-provider, explicit config precedence, provider filters, auth-profile routing metadata, secret-free env activation, and Copilot fallback availability. |
| No-config stored Codex | Implemented for runtime catalog, prompt setup, and TUI launch metadata. Stored Codex credential activates `openai-codex` and filters with `codex_oauth_model_allowed`. | Runtime-catalog, prompt, and TUI no-config tests passed. |
| No-config stored Copilot | Implemented for runtime catalog and model-list CLI. Stored Copilot credential activates `github-copilot`; offline fallback remains the deterministic path when generated/live data is unavailable. | Runtime-catalog and `models::tests::no_config_models_with_stored_copilot_lists_builtin_provider` passed. |
| No provider/no config | Implemented for startup surfaces. Runtime resolution has an explicit no-provider connect-state flag; TUI launch metadata uses `local` with no model, prompt setup blocks with connect guidance, and `harness models` prints connect guidance. | `cargo test -p harness prompt::tests::no_config_prompt -- --nocapture`; `cargo test -p harness no_config_tui -- --nocapture`; `cargo test -p harness models::tests::no_config_models -- --nocapture` passed. |
| Explicit config authority | Implemented. Matching explicit provider IDs are not overwritten by built-ins; disabled/enabled provider filters apply to configured and authenticated built-in providers. | Runtime-catalog provider precedence/filter tests passed; `docs/config.md`, `configs/config.json`, and `configs/harness.example.jsonc` document the non-inert filter/auth-provider behavior. |
| Model picker | Implemented for runtime launch metadata. The picker opens in no-provider state with connect guidance, groups authenticated built-in provider rows, searches provider labels, emits switch intents with provider/model metadata, and restores persisted recent built-in selections when valid. | `cargo test -p harness-tui model_switcher -- --nocapture` and `cargo test -p harness no_config_tui -- --nocapture` passed. |
| Prompt HUD/chrome | Implemented for selected runtime metadata. HUD/source labels update from `LaunchMetadata`; no-provider prompt submission blocks with connect guidance and does not emit a submit intent. | `cargo test -p harness-tui model_switcher -- --nocapture` plus `cargo test -p harness-tui no_provider_prompt_submission -- --nocapture` passed. |
| Provider routing | Implemented through the shared runtime catalog and `UiIntent::SwitchModel`/prompt launch metadata path. Built-in provider configs carry `authProvider` IDs for bootstrap/router construction, and switch intents carry the chosen provider/model for the next prompt. | `runtime_catalog::tests::builtin_provider_configs_carry_auth_profiles_for_router` and model switcher switch-intent tests passed. |
| Auth refresh | Implemented for TUI auth backend login success. The backend reloads runtime launch metadata from stored credentials, sends an auth-provider catalog refresh update, and the app opens the model picker with a provider-connected notice. | `cargo test -p harness auth_refresh_reloads -- --nocapture`, `cargo test -p harness-tui auth_catalog_refresh -- --nocapture`, and deterministic CLI smoke with `target/debug/harness auth login codex --mock-token …` followed by no-config `target/debug/harness models` passed. Live OAuth remains human/env-gated. |
| Security | Implemented with deterministic redaction coverage. Credential material stays in `CredentialStore`; runtime catalog, launch metadata, model selection state, TUI notices, snapshots, docs, and support bundles store provider/model/variant/status metadata only. | `cargo test -p harness --test replay_sessions_cli_test sessions_export_cli_excludes_stored_credentials_and_scans_for_leaks -- --nocapture`; final workspace grep/secret scan and required gates are recorded below. |

## Final reconciliation evidence

- Required final gates: `cargo fmt --all -- --check`, `cargo check --workspace`,
  `cargo test -p harness auth -- --nocapture`, `cargo test -p harness-core auth -- --nocapture`,
  `cargo test -p harness-providers -- --nocapture`,
  `cargo test -p harness-tui model_switcher -- --nocapture`,
  `cargo test -p harness --test config_docs_reference_test -- --nocapture`,
  and `cargo clippy --workspace --all-targets -- -D warnings`.
- Additional targeted gates cover no-config prompt/TUI/models, runtime catalog,
  TUI no-provider prompt blocking, auth-refresh model-picker refresh, and support
  export credential leak scanning.
- Deterministic manual smoke used a temporary `HARNESS_DATA_HOME` with
  `target/debug/harness auth login codex --mock-token ...` followed by no-config
  `target/debug/harness models`, which listed `openai-codex:*` rows without a
  project `harness.json{,c}`.
- Live OAuth with real OpenAI/GitHub accounts remains manual/env-gated and was
  not faked by deterministic completion.

## Live OAuth / manual QA boundary

Live OpenAI/GitHub OAuth signoff requires a human account and is env/manual-gated by the PRD. Autonomous completion will use deterministic credential-store fixtures, mock/provider tests, and PTY/TUI simulation. Final reporting must clearly distinguish deterministic proof from any remaining human live-account signoff.
