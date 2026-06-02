# reference implementation Auth-to-Model Provider Parity PRD

**Status:** Active implementation PRD for making Harness provider authentication,
model discovery, model selection, and active-provider display match reference implementation for
the V1-supported providers.
**Audience:** One autonomous implementation agent working in this repository until
the strict end-state goal below is true.
**Product authority:** The intended product behavior is reference implementation parity for the
supported providers: OpenAI/Codex and GitHub Copilot. Extra providers, plugin auth,
and custom provider installation are post-V1 unless they are needed as shared
architecture seams.

---

## 0. Strict end-state goal

This PRD is complete only when all of the following are true at the same time:

1. A user can run `harness auth login`, authenticate OpenAI/Codex or GitHub
   Copilot, and then use the authenticated provider in the TUI without manually
   editing `harness.json{,c}`.
2. The model picker shows authenticated built-in provider models, grouped and
   labelled by provider, after auth is stored.
3. Selecting a model in the picker immediately changes the active model for the
   current primary agent, persists it as recent selection state, and subsequent
   prompts use that model.
4. The prompt shell/HUD/input chrome shows the active agent, model, and provider
   label in the same product sense as reference implementation: a user can see whether they are
   using CLIProxyAPI/mock/local, OpenAI Codex, or GitHub Copilot before submitting.
5. If no provider is authenticated/configured, the UI does not crash or require
   config editing. It clearly offers `/connect` or `/auth` and blocks prompt send
   with an operator-visible notice.
6. Existing explicit config remains authoritative for users who intentionally set
   providers, default models, agent model mappings, disabled/enabled providers, or
   custom provider options.
7. All deterministic tests and manual QA gates in this PRD pass. No acceptance box
   may be considered complete from intention alone.

If any item above is false, continue implementing. Do not stop at a partial
catalog, a CLI-only flow, a docs-only change, or a picker that displays models but
does not route prompts through the selected provider.

---

## 1. Problem statement

Harness now has a close reference-style `auth login` UI for OpenAI/Codex and
GitHub Copilot, and it stores credentials securely. However, the stored
credential does not currently activate a usable provider/model catalog on its
own. Users still need to understand and edit provider config before the runtime,
model picker, and prompt shell can use those credentials.

That is not the reference implementation product model. In reference implementation, provider auth and model
availability are connected: once a provider is connected, the provider appears in
the runtime state, its models appear in the model picker, the selected model is
persisted locally, and the prompt input chrome shows the active model/provider.
Config is for overrides and advanced customization, not the normal post-login
path.

---

## 2. Desired user experience

### 2.1 First authenticated OpenAI/Codex run

1. User starts Harness with no project provider config.
2. User runs `/connect`, `/auth`, or `harness auth login`.
3. User chooses `OpenAI` and one of:
   - `ChatGPT Pro/Plus (browser)`
   - `ChatGPT Pro/Plus (headless)`
   - `Manually enter API Key`
4. Harness stores the credential outside config.
5. When the user opens `/model`, they see an OpenAI Codex provider group with
   V1-supported GPT/Codex models.
6. User selects an OpenAI Codex model.
7. The prompt input chrome/HUD shows the active model and provider.
8. The next prompt is sent through the OpenAI-compatible provider decorated with
   the Codex auth profile and credential source.

### 2.2 First authenticated GitHub Copilot run

1. User starts Harness with no project provider config.
2. User runs `/connect`, `/auth`, or `harness auth login`.
3. User chooses `GitHub Copilot`.
4. User chooses GitHub.com or GitHub Enterprise and completes device login.
5. Harness stores the credential outside config.
6. `/model` shows GitHub Copilot models.
7. User selects a Copilot model.
8. The prompt input chrome/HUD shows GitHub Copilot as the active provider.
9. The next prompt is sent through the OpenAI-compatible provider decorated with
   the Copilot auth profile and credential source.

### 2.3 Existing configured provider run

If the user has an explicit provider config, Harness must preserve today’s
behavior: configured providers, defaults, agent model mappings, model profiles,
and custom provider options are still respected. Built-in authenticated providers
augment the catalog unless disabled; they do not silently rewrite user config.

---

## 3. reference implementation behavior

The implementing agent must re-read these files before coding. Copy observable
behavior and product semantics, not TypeScript architecture or branding.

### 3.1 Provider auth and connected-provider list

- `inspirations/reference implementation/packages/reference implementation/src/server/routes/instance/httpapi/handlers/provider.ts`
  - `list` merges models.dev providers with connected providers.
  - Return shape includes `all`, `default`, and `connected`.
  - `connected` is derived from authenticated provider runtime state, not from a
    manually edited config file.
- `inspirations/reference implementation/packages/reference implementation/src/provider/auth.ts`
  - Auth callbacks store credentials through the auth service.
  - OAuth authorize/callback is provider-id keyed.
- `inspirations/reference implementation/packages/reference implementation/src/auth/index.ts`
  - Auth storage is separate from config.

### 3.2 Connect-provider dialog

- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/tui/component/dialog-provider.tsx`
  - Provider priority puts reference implementation/OpenAI/GitHub Copilot near the top.
  - Connected providers show a checkmark.
  - Selecting a provider chooses an auth method, completes auth, disposes/reboots
    instance state, re-syncs provider state, then opens `DialogModel` scoped to
    the provider.
  - For V1 Harness, only OpenAI/Codex and GitHub Copilot need full behavior.

### 3.3 Model picker

- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/tui/component/dialog-model.tsx`
  - Uses synced provider state for `sync.data.provider`.
  - Groups model rows by provider when connected.
  - Includes recents/favorites when connected.
  - When not connected, shows popular provider connect options instead of
    requiring config editing.
  - Selecting a model calls `local.model.set({ providerID, modelID }, { recent:
    true })`.

### 3.4 Local model state and active-model fallback

- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/tui/context/local.tsx`
  - Local model state persists recent/favorite/variant data in state `model.json`.
  - Active model resolution order is:
    1. CLI `--model`, if valid.
    2. Config default model, if valid.
    3. Recent model, if valid.
    4. First provider’s default model.
    5. First model in the first provider.
  - `parsed()` returns display labels for provider and model, or a connect/no
    provider state.
  - `set()` validates against the current provider/model catalog before changing
    active state.

### 3.5 Prompt input chrome/HUD

- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/tui/component/prompt/index.tsx`
  - When in normal mode, the prompt footer shows agent, model label, provider
    label, and variant if selected.
  - When in shell mode, it shows `Shell` instead of model/provider metadata.
  - If no provider is connected and the user submits, the UI warns `Connect a
    provider to send prompts` and opens the connect dialog when no provider exists.

### 3.6 Run/footer model commands

- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/run/footer.command.tsx`
  - Command-mode model switching also works from provider/model catalog state.
- `inspirations/reference implementation/packages/reference implementation/src/cli/cmd/run/runtime.ts`
  - Runtime state includes `reference implementation.model.provider` and `reference implementation.model.id` in
    metadata/logging.

---

## 4. Current Harness source map

The implementing agent must inspect these paths before editing.

### 4.1 Auth storage and provider credential decoration

- `crates/harness/src/auth_cmd.rs`
  - `harness auth login` interactive flow and explicit provider/method labels.
  - Stores credentials in `CredentialStore`.
- `crates/harness-core/src/auth.rs`
  - Credential store, redaction, credential precedence.
- `crates/harness-core/src/auth/codex.rs`
  - Codex PKCE/device flow and `codex_oauth_model_allowed`.
  - Contains the upstream-compatible allowed GPT/Codex model filter.
- `crates/harness-core/src/auth/copilot.rs`
  - Copilot OAuth flow and `copilot_offline_fallback_models`.
- `crates/harness/src/bootstrap.rs`
  - `build_provider` attaches `OpenAiAuthProfile::{Codex,GithubCopilot}` and a
    `ProviderCredentialManager` only when a configured provider has
    `authProvider` set.

### 4.2 Config and generated model catalog

- `crates/harness-core/src/config.rs`
  - `HarnessConfig.providers` is currently the source of configured runtime
    provider catalog.
  - `configured_model_catalog` builds model picker entries only from configured
    providers.
  - `resolve_model_selection` validates model/profile references against config.
- `crates/harness/src/generated_model_catalog.rs`
  - Embeds `configs/provider-catalog.generated.json`.
- `crates/harness/src/model_probe.rs`
  - Generates and prints the embedded catalog.
- `configs/provider-catalog.generated.json`
  - Static generated models.dev catalog.
- `configs/harness.example.jsonc`
  - Current starter config still assumes manually configured provider defaults.

### 4.3 CLI runtime and no-config behavior

- `crates/harness/src/lib.rs`
  - Top-level command dispatch.
- `crates/harness/src/models.rs`
  - `harness models` currently requires config unless printing/generated catalog.
- `crates/harness/src/prompt.rs`
  - Prompt mode currently requires a config unless mock/provider override paths are
    used.
- `crates/harness/src/tui.rs`
  - TUI live startup, config loading, launch metadata, model selection state, and
    `UiIntent::SwitchModel` handling.
  - `MODEL_SELECTION_STATE_FILE` and `PersistedModelSelection` are the Harness
    counterpart to reference implementation’s state `model.json`.

### 4.4 TUI model picker and prompt chrome

- `crates/harness-tui/src/app.rs`
  - `LaunchMetadata`, active/current model labels, `UiIntent::SwitchModel`, model
    switcher state.
- `crates/harness-tui/src/app/session_navigation.rs`
  - `LaunchMetadata`, `ModelOption`, session/model metadata shaping.
- `crates/harness-tui/src/ui.rs` and related `ui_*` modules
  - Renders command palette, model switcher, prompt/input shell, HUD/chrome.
- `crates/harness-tui/tests/model_switcher/palette_test.rs`
  - Existing deterministic model-switcher tests.

---

## 5. Required implementation decisions

### 5.1 Add a built-in authenticated provider catalog seam

Create a Harness-native seam that returns built-in provider definitions activated
by stored credentials and/or configured fallbacks. Do not make `auth login` write
or mutate project config.

Required behavior:

- Built-in provider id for OpenAI/Codex should be stable and user-comprehensible.
  The UI label should be `OpenAI Codex` or `OpenAI`; the auth provider id remains
  `codex` internally unless a deeper refactor intentionally changes it.
- Built-in provider id for GitHub Copilot should be `github-copilot` with UI label
  `GitHub Copilot`.
- Built-in provider definitions must include:
  - provider id
  - display name
  - OpenAI-compatible base URL
  - auth provider id
  - model list
  - default model id
  - metadata needed by model resolution, prompt family selection, token limits,
    and TUI display.
- Codex model list must follow the existing `codex_oauth_model_allowed` reference
  rule. Do not expose arbitrary non-Codex GPT-4/legacy models as OAuth Codex
  models.
- Copilot model list should use live/probed data when the architecture already has
  it. The reference implementation fetches the authenticated Copilot `/models` endpoint,
  filters disabled/non-picker models, and merges the result into picker-visible
  model metadata. Harness should implement that behavior behind a deterministic
  mock/cassette seam; use the existing offline fallback models only when live
  discovery is unavailable, and surface that fallback state in tests/UI/docs.
- Built-ins must be active when a stored credential exists. A configured env/API
  key fallback may also activate Codex/OpenAI if the user has configured one, but
  stored credentials take precedence.

Recommended shape:

- Put pure catalog composition in a deep, testable module rather than scattering
  it across CLI/TUI/bootstrap. Candidate locations:
  - `crates/harness-core/src/config.rs` if it is purely config/catalog resolution.
  - A new core module if the seam needs credential-store presence input but not
    I/O.
  - A harness crate module if it must read the credential store or embedded JSON.
- The public interface should accept:
  - optional loaded config
  - credential presence/status for `codex` and `github-copilot`
  - environment fallback availability
  - enabled/disabled provider filters
  - embedded/generated catalog data or curated fallback data
- The interface should return one merged runtime provider catalog used by CLI,
  TUI launch metadata, model picker, and provider router.

### 5.2 Merge built-ins with explicit config without surprising users

Merge order must be explicit and tested:

1. Explicit user config wins for matching provider ids and agent/profile defaults.
2. Built-in authenticated providers fill gaps when config is absent or incomplete.
3. Disabled provider filters hide both configured and built-in providers.
4. Enabled provider filters, if honored for built-ins, must be documented and
   tested.
5. No credential secret may be copied into config, model catalog entries, launch
   metadata, TUI state, event logs, snapshots, or support bundles.

### 5.3 Make no-config TUI startup use built-in catalog

Today interactive/live paths report that a config file is required. After this
PRD, no-config startup should be valid if a built-in provider is authenticated.

Required behavior:

- If no config exists and at least one built-in provider has usable credentials,
  build a runtime `HarnessConfig` or equivalent coordinator config from the
  built-in provider catalog and shipped default agents.
- If no config exists and no provider is authenticated, start the TUI in a
  connectable state rather than exiting with only config guidance. The prompt send
  path must remain blocked until a provider exists.
- Keep `--mock`/CLIProxyAPI/demo behavior available and visibly labelled as mock or
  CLIProxyAPI, not confused with real providers.

### 5.4 Make model picker use runtime provider state, not raw config only

The model picker should be backed by the same merged runtime provider catalog used
to route provider requests.

Required behavior:

- `/model` opens even when no project config exists.
- When connected, model rows are grouped by provider display label.
- Search matches model names and provider labels.
- The selected row is visibly marked.
- Selecting a model emits the existing `UiIntent::SwitchModel` or a refined intent
  that contains enough information to rebuild coordinator routing and prompt
  metadata.
- Selecting a model must not be cosmetic. The next prompt must use the selected
  provider/model.
- Recent selection state should be persisted to Harness’s model state file and
  restored when valid, following reference implementation’s fallback order.

### 5.5 Active model resolution order

Implement and test a Harness equivalent of reference implementation’s local model fallback:

1. CLI/profile override, if valid.
2. Explicit config agent/default model, if valid.
3. Persisted recent model, if valid.
4. Built-in/default provider model for the first authenticated provider.
5. First valid model from the first provider.
6. No-provider state with connect guidance.

The exact profile integration can follow existing Harness concepts, but the user
observable result must match reference implementation: a real connected provider becomes usable
without config, and the prompt chrome shows what is active.

### 5.6 Prompt shell/HUD must show active provider/model

The prompt input surface must display the active agent/model/provider label for
normal prompt mode and a shell label for shell mode.

Required behavior:

- When using CLIProxyAPI/mock/local provider, the HUD identifies that provider.
- When switching to OpenAI Codex from `/model`, the HUD changes to OpenAI/Codex
  label before the next prompt is sent.
- When switching to GitHub Copilot, the HUD changes to GitHub Copilot.
- Variant labels remain visible where Harness already supports variants.
- Provider/model display labels come from the runtime model catalog, not hardcoded
  string parsing.

### 5.7 Runtime provider routing must follow selected model

The coordinator/provider router must route provider calls through the selected
provider/model.

Required behavior:

- Switching from CLIProxyAPI/mock/local to OpenAI Codex changes the provider router
  used for the next agent turn.
- Switching to GitHub Copilot changes the provider router used for the next agent
  turn.
- Child agents/subagents use their configured/default profile models unless the
  existing Harness model-override semantics intentionally apply. Document and test
  whichever behavior exists.
- Provider errors must remain visible; do not add silent fallback unless existing
  model-profile fallback explicitly says so.

### 5.8 Auth completion should refresh provider/model state

When TUI auth completes mid-session:

- The credential is stored.
- Provider/model runtime state refreshes.
- The newly connected provider appears in `/model` without requiring restart.
- The connect/auth dialog should either open the model picker scoped to that
  provider or show an operator notice telling the user to pick a model. reference implementation
  opens `DialogModel(providerID)` after auth; match that if feasible.

---

## 6. User stories

1. As a first-time user, I want to log into OpenAI/Codex and immediately pick a
   model, so that I can start using Harness without writing config.
2. As a first-time user, I want to log into GitHub Copilot and immediately pick a
   model, so that Copilot works like it does in reference implementation.
3. As an operator, I want `/model` to show only usable connected provider models
   and clear connect options, so that I know what I can actually run.
4. As an operator, I want the prompt HUD to show the active provider and model, so
   that I do not accidentally send a prompt to CLIProxyAPI/mock/local when I meant
   OpenAI Codex.
5. As an operator, I want model selection to affect the next prompt, so that the
   picker is not cosmetic.
6. As an operator, I want recent model selection to persist, so that restarting the
   TUI keeps using my last valid provider/model.
7. As a project maintainer, I want explicit config to override built-ins, so that
   project defaults and agent-specific model choices stay reproducible.
8. As a security-conscious user, I want credentials stored outside config and never
   printed or snapshotted, so that support bundles and event logs are safe.
9. As an unattended implementation agent, I want deterministic tests for the
   no-config/authenticated path, so that I can prove the feature without live
   credentials.
10. As a release reviewer, I want live OAuth signoff documented separately, so that
    nobody fakes real provider success in deterministic tests.

---

## 7. Acceptance criteria

### 7.1 Catalog and runtime activation

- [ ] With no `harness.json{,c}`, a stored Codex credential causes Harness to
      produce a runtime provider catalog containing OpenAI/Codex models.
- [ ] With no `harness.json{,c}`, a stored GitHub Copilot credential causes
      Harness to produce a runtime provider catalog containing Copilot models.
- [ ] With no stored credentials and no config, Harness starts in a no-provider
      connect state instead of panicking or requiring a manual config edit.
- [ ] Explicit configured providers remain available and authoritative.
- [ ] Disabled/enabled provider filtering behavior is source-documented and tested.

### 7.2 Model picker

- [ ] `/model` opens without project config.
- [ ] After Codex auth, `/model` shows OpenAI/Codex provider rows and models.
- [ ] After Copilot auth, `/model` shows GitHub Copilot provider rows and models.
- [ ] Search matches provider labels and model names.
- [ ] Selecting a row updates current model state and closes/advances the picker in
      the existing Harness UX style.
- [ ] Recent model state persists and is restored when still valid.

### 7.3 Prompt HUD/input shell

- [ ] Prompt chrome shows current agent, model label, provider label, and variant
      in normal mode.
- [ ] Prompt chrome shows shell-mode labeling instead of provider/model metadata in
      shell mode.
- [ ] Switching from CLIProxyAPI/mock/local to OpenAI/Codex updates the HUD before
      the next prompt.
- [ ] Switching to GitHub Copilot updates the HUD before the next prompt.
- [ ] Submitting with no connected provider shows connect guidance and does not
      start a provider turn.

### 7.4 Provider routing

- [ ] The next prompt after selecting OpenAI/Codex is routed through the Codex auth
      profile and stored credential source.
- [ ] The next prompt after selecting GitHub Copilot is routed through the Copilot
      auth profile and stored credential source.
- [ ] Provider/model refs recorded in runtime/session metadata match the selected
      provider/model.
- [ ] Provider errors stay visible; no hidden fallback masks a selected provider
      failure.

### 7.5 Auth-to-model refresh

- [ ] Completing TUI auth mid-session refreshes provider/model state without a TUI
      restart.
- [ ] After TUI auth, the user is guided into model selection for the newly
      connected provider, matching reference implementation’s `DialogModel(providerID)` behavior as
      closely as the Harness UI allows.
- [ ] CLI `harness auth login` followed by `harness tui` works without config.

### 7.6 Security and persistence

- [ ] No API key, OAuth access token, refresh token, device code, authorization
      code, account id, cookie, or bearer appears in stdout/stderr, TUI notices,
      event logs, snapshots, support bundles, committed fixtures, or docs.
- [ ] Credential files remain under the existing credential store and keep current
      restrictive permissions/atomic replacement behavior.
- [ ] Model selection state stores provider/model/variant ids only; it never stores
      credential material.

---

## 8. Testing plan

The implementation agent must add or extend tests before claiming completion.

### 8.1 Unit and integration tests

Add tests around the new catalog seam:

- Stored Codex credential + no config => built-in Codex provider/model catalog.
- Stored Copilot credential + no config => built-in Copilot provider/model catalog.
- Explicit config overrides matching built-in provider ids.
- Disabled provider filters hide built-ins.
- No credential/config produces no-provider connect state without runtime panic.
- Codex model filtering follows `codex_oauth_model_allowed`.
- Copilot offline fallback models are present and labelled.

Likely commands:

```bash
cargo test -p harness-core auth -- --nocapture
cargo test -p harness-core model -- --nocapture
cargo test -p harness auth -- --nocapture
```

### 8.2 TUI model picker tests

Extend `crates/harness-tui/tests/model_switcher/palette_test.rs` or adjacent
fixtures:

- No-config authenticated Codex launch metadata renders OpenAI/Codex model rows.
- No-config authenticated Copilot launch metadata renders GitHub Copilot model rows.
- Search by provider label works.
- Enter emits a switch intent with provider/model/variant data.
- Current selection marker and HUD label update after switching.

Likely command:

```bash
cargo test -p harness-tui --test model_switcher_metadata_test -- --nocapture
cargo test -p harness-tui model_switcher -- --nocapture
```

Use the actual test names that exist after implementation; do not invent passing
commands in progress notes.

### 8.3 CLI/TUI no-config tests

Add tests proving the real top-level surfaces:

- `harness models` or equivalent model-list surface works with stored credentials
  and no config.
- `harness tui` no-config + stored credential enters live mode with built-in
  provider launch metadata.
- `harness tui` no-config + no credential enters connectable no-provider state.
- `harness prompt` no-config + selected/stored model behavior is either supported
  or explicitly blocked with the same connect guidance. If blocked, document why.

Likely commands:

```bash
cargo test -p harness --test config_schema_cli_test -- --nocapture
cargo test -p harness --test tui_cli_test -- --nocapture
cargo test -p harness auth -- --nocapture
```

### 8.4 Provider routing tests

Use mocked providers/credential stores, not live network:

- Selecting built-in Codex creates an OpenAI-compatible provider with Codex auth
  profile and credential source.
- Selecting built-in Copilot creates an OpenAI-compatible provider with Copilot
  auth profile and credential source.
- A provider request after model switch records the selected provider/model in
  event/runtime metadata.

Likely commands:

```bash
cargo test -p harness-providers -- --nocapture
cargo test -p harness-core --test coord_test -- --nocapture
cargo test -p harness --test run_cli_test -- --nocapture
```

### 8.5 Manual QA gate

The final agent must drive the feature through a real terminal surface:

```bash
rm -rf /tmp/harness-auth-model-parity
HARNESS_DATA_HOME=/tmp/harness-auth-model-parity cargo run -p harness -- auth login
HARNESS_DATA_HOME=/tmp/harness-auth-model-parity cargo run -p harness -- tui
```

Manual observations to record:

- `/model` opens without config.
- Connected provider appears.
- Selecting OpenAI/Codex changes the HUD/provider label.
- Selecting GitHub Copilot changes the HUD/provider label.
- Submitting a tiny prompt uses the selected provider in deterministic/mock mode,
  or live mode only when the operator intentionally supplies real credentials.
- With no provider, submission is blocked with connect guidance.

Live OAuth against real OpenAI/GitHub accounts is env-gated/manual signoff. It is
not required for autonomous deterministic completion, and it must never be faked.

### 8.6 Required final gates

At minimum, before marking done:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test -p harness auth -- --nocapture
cargo test -p harness-core auth -- --nocapture
cargo test -p harness-providers -- --nocapture
cargo test -p harness-tui model_switcher -- --nocapture
cargo test -p harness --test config_docs_reference_test -- --nocapture
```

Run broader lanes if the touched files require them under `AGENTS.md`:

```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh signoff-pty
```

---

## 9. Out of scope

- Additional provider auth beyond OpenAI/Codex and GitHub Copilot.
- reference implementation plugin provider auth.
- Custom provider configuration UX beyond preserving existing config behavior.
- Publishing to NPM or changing distribution channels.
- Real live OAuth as an autonomous test requirement.
- Silent automatic fallback between selected providers on provider error.
- Rewriting the entire TUI to OpenTUI/Solid; keep Ratatui architecture.
- Persisting credentials in config.

---

## 10. Implementation notes for the agent

- Start by writing the catalog-seam tests. The hardest bug to avoid is building a
  TUI-only catalog that looks right but is not used by provider routing.
- Keep the merged runtime catalog as the single source for:
  - provider router construction
  - launch metadata
  - model picker options
  - active model labels
  - model validation
- Do not let `auth login` mutate `harness.json{,c}`. Auth stores credentials;
  runtime discovery activates providers.
- Prefer one deep module with a simple interface over conditionals spread across
  `tui.rs`, `bootstrap.rs`, `models.rs`, and `app.rs`.
- Record every acceptance result in a progress log before claiming completion.
  Recommended progress file: `docs/auth-model-parity-progress.md`.
- If you discover an reference implementation conflict, update this PRD with the source
  citation before implementing the resolved behavior.

---

## 11. Completion checklist for final report

The implementing agent’s final answer must include:

1. Files changed.
2. Which reference implementation files were used.
3. How no-config auth activation works.
4. How model picker state and provider routing share the same catalog.
5. Manual PTY/TUI QA observations.
6. Full command evidence with pass/fail status.
7. Any remaining manual live OAuth signoff that requires a human account.
