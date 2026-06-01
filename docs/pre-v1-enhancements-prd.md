# Pre-V1 Enhancements PRD: Provider OAuth, Cache Parity, Prompt Parity, Skill Hardening, Onboarding

**Status:** Complete implementation PRD for the pre-V1 enhancement slice; evidence is recorded in [`docs/pre-v1-enhancements-progress.md`](pre-v1-enhancements-progress.md).
**Audience:** A single autonomous implementing agent working unattended in this
repository over multiple iterations until the strict end-state goal in §0.1 holds.
**Authority:** This PRD is subordinate to [`docs/roadmap-v1.md`](roadmap-v1.md)
for product scope. It is the operational spec for the pre-V1 enhancement items
added to that roadmap. Where this PRD and the roadmap disagree on scope, the
roadmap wins; where they disagree on *how* to make a checked claim truthful, this
PRD's anti-gaming and evidence rules win.

This PRD adds new capability; it does not reopen unrelated checked V1 work. The
companion correction work for already-checked-but-overstated boxes lives in
[`docs/v1-roadmap-claim-correction-prd.md`](v1-roadmap-claim-correction-prd.md)
and is referenced, not duplicated, here.

---

## 0. Read this first

### 0.1 Strict end-state goal (the only definition of "done")

This PRD is **COMPLETE if and only if ALL** of the following hold *simultaneously*
on the working branch. Partial completion is **not** completion.

1. Every acceptance criterion in §5 is satisfied and backed by a cited
   test, lane, or command **plus** a source citation recorded in
   `docs/pre-v1-enhancements-progress.md`.
2. All workspace gates in §6.2 pass with zero failures and zero warnings:
   `cargo fmt --all -- --check`, `cargo check --workspace`,
   `cargo test --workspace --all-features`,
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
3. The targeted gates in §6.1 pass for every workstream that changed code.
4. Every roadmap box this PRD touches in [`docs/roadmap-v1.md`](roadmap-v1.md) is
   updated **honestly**: checked only with a cited evidence command and source
   citation; otherwise left unchecked or reworded to the truth. No box is checked
   from intention.
5. The secret-scan / simulation gate passes and **no** OAuth token, refresh token,
   bearer, authorization code, client secret, account id, or cookie appears in any
   committed file, event log, support bundle, test fixture, or snapshot.
6. `docs/pre-v1-enhancements-progress.md` exists and records, per acceptance
   criterion: the changed files, the evidence command, the observed result, the
   source citation, and a one-line "breaks if:" statement for every new test.

If any single item above is unmet, the PRD is **NOT** complete. Continue working.

### 0.2 How to behave as an unattended agent

- You run until the §0.1 end-state holds. Do not stop at analysis, scaffolding, or
  "most of it works."
- When one workstream is blocked, switch to an independent workstream (see the
  dependency graph in §8) rather than halting. Come back to the blocked one.
- The **only** acceptable reasons to stop before §0.1 holds are: (a) the end-state
  goal holds, or (b) a workstream's *remaining* work requires a real external
  secret you cannot synthesize (e.g., a live ChatGPT or GitHub account to perform a
  real end-to-end login). In case (b): complete **all** deterministic and
  fixture/mock work for that workstream, mark the live step as a manual signoff
  item in the progress doc, and continue with every other workstream. A missing
  live credential never blocks the deterministic done-condition (see §0.4).
- You decide the implementation. This PRD specifies *required outcomes, seams to
  respect, reference sources, and tests*. It deliberately does not dictate exact
  Rust type names, module layouts, or function signatures. Choose the cleanest
  design that fits the existing crate seams and invariants.

### 0.3 Anti-gaming contract (forbidden shortcuts)

Violating any rule here means the PRD is not complete regardless of checkbox state.

- **Do not** weaken, skip, delete, `#[ignore]`, or assert-loosen any test or gate
  to reach a green state.
- **Do not** flip a roadmap checkbox without a source citation and a runnable
  evidence command.
- **Do not** make deterministic lanes perform real network calls to OpenAI,
  ChatGPT, GitHub, or any provider. OAuth and provider transports must be proven
  with mocked endpoints, loopback servers, fixtures, or cassettes.
- **Do not** print, log, persist to events, or commit any secret material. Route
  every credential through the existing redaction path.
- **Do not** invent OAuth flows, endpoints, client IDs, or header conventions from
  memory. Re-read the cited reference files in `inspirations/` and copy
  *observable behavior*, not branding or source architecture.
- **Do not** build a new provider transport protocol. Codex and Copilot are
  OpenAI-compatible execution decorated with OAuth credentials, a base-URL
  rewrite, and extra request headers. Implement them on top of the existing
  OpenAI-compatible path (see §3.3), not as bespoke streaming engines.
- **Do not** add an OS-level sandbox, native Anthropic transport,
  `previous_response_id` server-state reuse, additional regional/Chinese
  providers, or a logo redesign. Those are explicitly post-V1 (§9). Building them
  is scope violation, not progress.
- **Do not** reopen unrelated roadmap percentages, denominator math, or unchecked
  future work outside this PRD's scope.
- **Do not** introduce backward-compatibility shims without a concrete runtime
  need.

### 0.4 Live vs deterministic proof (resolves the most likely confusion)

Real end-to-end OAuth logins require human-held accounts and a browser, which an
unattended agent does not have. Therefore:

- The **done-condition** (§0.1) requires only **deterministic, fixture/mock-based**
  proof of every OAuth mechanism: PKCE generation, the loopback callback server,
  device-code polling state machine, token-exchange request shape, refresh logic,
  request decoration (headers + base-URL rewrite), secure storage, redaction, and
  doctor reporting.
- A **real login** against live ChatGPT/GitHub is recorded as a **manual,
  env-gated signoff item** in the progress doc and is **not** part of the
  autonomous done-condition. Do not block on it, and do not fake it.

### 0.5 Required operating rules

- Before the first code edit, load the mandatory coding skill `karpathy-guidelines`
  per [`AGENTS.md`](../AGENTS.md), and read any crate-scoped `AGENTS.md` that
  applies to the crate you are about to change (notably
  `crates/harness-providers/AGENTS.md` for transport/credential work,
  `crates/harness-core/AGENTS.md` for coordinator/config/event work,
  `crates/harness-tui/AGENTS.md` for onboarding/UX work).
- Use TDD: write the failing test first, then the smallest correct implementation,
  then the evidence row.
- Preserve every invariant in [`AGENTS.md`](../AGENTS.md) §INVARIANTS: events are
  the source of truth, the coordinator is the only append/permission/lifecycle
  authority, replay is side-effect free, session-inspection tools never hit the
  network, and provider-context compaction never rewrites `events.jsonl`.
- Honor the UPDATE-TOGETHER table in [`AGENTS.md`](../AGENTS.md): when you touch
  public config keys, provider catalog, native tool ids, event variants, or test
  lanes, update the paired docs/schemas/tests in the same change.
- Keep `inspirations/` as read-only reference. Never import its code or branding.

### 0.6 Parity mandate and stop condition

The bar for every in-scope surface is **functional and visual/behavioral parity
with its named reference authority**, adapted to the harness's Rust, event-sourced,
Ratatui-TUI architecture and to harness branding. "It works" is not the bar.
"It matches the reference, or is the best possible harness-native version of it"
is the bar.

- Reference authorities by surface: **opencode** for provider auth, the `auth`
  command surface, onboarding, and TUI/skill UX; **pi-mono** for prompt caching;
  **OMO (`oh-my-openagent`/`oh-my-codex`)** for model resolution and model-family
  prompts.
- Do not stop a workstream at a thin or "good enough" version. Compare your result
  against the reference and the parity screenshots in
  `inspirations/screenshots opencode ui parity/` and
  `inspirations/opencode-ui-images/`. If the reference does something this
  workstream's surface should do and you have not matched it, you are not done.
- Parity means *observable behavior and visual design*, reimplemented natively —
  never copied source code, never opencode/pi/OMO branding. Where the harness's
  architecture lets it do better than the reference (e.g., event-sourced replay,
  determinism, stronger redaction), do better and note it; never do worse.
- The two explicit, non-negotiable parity targets called out by the product owner:
  (1) the first-run onboarding must **function and look exactly like opencode's**
  onboarding (harness identity substituted for opencode's name/logo only — see
  WS6), and (2) the `auth` CLI and auth management must be runnable **at any time**,
  not only during first run (see WS6).
- This mandate does not expand scope into §9. It raises the quality bar *within*
  the WS1–WS8 surfaces only.

---

## 1. Problem statement and context

The pre-V1 roadmap foundations are checked, but the product is missing
high-leverage, well-scoped capability that makes a vanilla local-coding harness
trustworthy and pleasant on first contact:

1. **No provider authentication beyond pasted API keys.** There is no
   `auth`/`login`/`oauth` surface anywhere in the binary (verified: a workspace
   grep for `oauth`/`login`/`device_code`/`access_token`/`refresh_token` across
   `crates/harness/src` and `crates/harness-core/src` returns nothing). Users must
   hand-edit `apiKeyEnv` into config. The two providers the project wants first are
   **Codex (ChatGPT subscription)** and **GitHub Copilot**, both of which use OAuth
   in the reference implementations.
2. **Cache hit rate is left on the table.** The runtime *reads* cache telemetry
   but sets **no** request-side cache controls. The single highest-leverage,
   lowest-effort cache lever — a stable per-session `prompt_cache_key` on
   OpenAI-compatible requests — is absent, and the system prompt places volatile
   fields early in the prefix.
3. **Model-family selection is substring heuristics**, which the roadmap itself
   warned must not be the only long-term seam, yet a box claiming "explicit prompt
   presets" is checked.
4. **Non-GPT model prompts are thin** relative to the GPT body and are hardcoded as
   Rust constants rather than data assets. Once Copilot (which can expose GPT,
   Claude, and Gemini families) lands, those thin prompts ship to real users.
5. **Skill bundled-resource loading is deferred**, and there is no first-run
   onboarding flow; the checked "first-run" items are documentation-only.

This PRD closes those gaps for V1. It deliberately excludes the heavier items
(OS sandbox, native Anthropic transport, server-side response reuse, more
providers, logo) which remain post-V1 (§9).

---

## 2. Scope

In scope (pre-V1), as workstreams WS1–WS8 in §4:

- WS1 OpenAI-compatible prompt-cache parity (pi-mono technique).
- WS2 Model family/capability resolution seam (replace substring heuristics).
- WS3 Provider credential abstraction + secure auth store + refresh.
- WS4 Codex (ChatGPT) OAuth provider (PKCE loopback + device-code), OpenAI-compatible.
- WS5 GitHub Copilot OAuth provider (device-code), OpenAI-compatible.
- WS6 First-run onboarding + `auth` CLI/TUI login UX (+ skill-list UX alignment).
- WS7 Non-GPT family prompt parity, sourced from data assets (depends on WS2).
- WS8 Skill hardening: bundled-resource progressive loading + escape tests.

Everything in §9 is out of scope.

---

## 3. Verified findings (re-read the cited files before coding)

These are point-in-time findings. They are accurate as of authoring but you must
re-read each cited file before changing code, because line numbers and exact
shapes may have shifted.

### 3.1 Caching: request side is empty, prefix is volatile-early

- `crates/harness-providers/src/openai.rs` reads cache usage
  (`cache_read_tokens`, `cache_write_tokens`, `cached_tokens`,
  `cache_creation_input_tokens`, `provider_cache_id`) from responses (around the
  usage/metadata parsing near lines 1700–1770) but sets **no** `prompt_cache_key`,
  `cache_control`, or `previous_response_id` on the request.
- `crates/harness-core/src/event.rs` already carries `cache_read_tokens` /
  `cache_write_tokens` / `provider_cache_id` on the relevant events (around lines
  424–464), so telemetry is available to surface.
- `crates/harness/src/dynamic_prompt.rs` composes the system prompt in the order
  `base_model → environment → delegation_reminder → project_instructions →
  skill_guidance` (the `compose_with_environment` section list). The `environment`
  section (its `environment_prompt`) embeds **Today's date and the git branch**
  near the front of the prompt, so a branch switch or midnight rollover
  invalidates the cacheable prefix tail.

Reference behavior to copy (pi-mono is the caching authority):

- `inspirations/pi-mono/packages/ai/src/providers/openai-prompt-cache.ts` —
  `clampOpenAIPromptCacheKey` clamps the key to 64 chars.
- `inspirations/pi-mono/packages/ai/src/providers/openai-responses.ts` and
  `.../openai-completions.ts` — set `prompt_cache_key: clamp(sessionId)` on the
  request (omit when retention is disabled).

### 3.2 Model selection: substring heuristics vs an explicit seam

- `crates/harness/src/dynamic_prompt.rs` `provider_prompt(model_id)` selects the
  body via `model_id.contains("gpt-4")`, `.contains("gpt")`, `.contains("claude")`,
  `.contains("gemini-")`, `.contains("kimi")`, `.contains("trinity")`, etc.,
  returning hardcoded `const &str` bodies (`PROMPT_GPT`, `PROMPT_CODEX`,
  `PROMPT_BEAST`, `PROMPT_ANTHROPIC`, `PROMPT_GEMINI`, `PROMPT_KIMI`,
  `PROMPT_TRINITY`, `PROMPT_DEFAULT`).
- [`docs/roadmap-v1.md`](roadmap-v1.md) (Agent prompt depth) states "substring
  heuristics do not become the only long-term seam" yet checks
  "Model-specific prompt tuning is ... explicit prompt presets with tests." This
  tension must be reconciled honestly (implement the seam, or reword the box).

Reference behavior (OMO is the model-resolution authority):

- `inspirations/oh-my-openagent/packages/model-core/src/model-family-detectors.ts`,
  `.../model-capabilities*`, `.../variant-resolver.ts`,
  `.../model-resolution-pipeline.ts` — a capability/family seam keyed off model
  metadata, not raw substring scans.

### 3.3 Provider auth: none exists; Codex/Copilot are OpenAI-compatible + decoration

Today the provider config accepts `apiKey` and `apiKeyEnv` plus `options.baseURL`
(see [`README.md`](../README.md) first-run section and
[`docs/config.md`](config.md)). There is no OAuth credential kind, no token store,
and no `auth` command.

Both target providers are OpenAI-compatible execution with (a) an OAuth-derived
bearer token, (b) a base-URL rewrite, and (c) provider-specific request headers.
The reference flows, to copy as behavior:

**Codex / ChatGPT** — `inspirations/opencode/packages/opencode/src/plugin/openai/codex.ts`:

- Client id `app_EMoamEEZ73f0CkXaXp7hrann`; issuer `https://auth.openai.com`;
  Codex request endpoint `https://chatgpt.com/backend-api/codex/responses`.
- Browser flow: loopback HTTP server on port 1455, redirect
  `http://localhost:1455/auth/callback`; PKCE S256 (43-char verifier, SHA-256
  challenge, base64url); CSRF `state`; authorize URL at `${ISSUER}/oauth/authorize`
  with scope `openid profile email offline_access` and the
  `id_token_add_organizations`, `codex_cli_simplified_flow`, `originator` params;
  code→token exchange at `${ISSUER}/oauth/token` (grant_type
  `authorization_code`, with `code_verifier`); 5-minute callback timeout with
  success/error HTML pages.
- Headless device flow: POST `${ISSUER}/api/accounts/deviceauth/usercode` →
  `device_auth_id`/`user_code`/`interval`; display `${ISSUER}/codex/device` + the
  user code; poll `${ISSUER}/api/accounts/deviceauth/token` until it returns an
  authorization code + verifier, then exchange at `${ISSUER}/oauth/token`.
- Token refresh: grant_type `refresh_token` at `${ISSUER}/oauth/token`; refresh
  proactively when access is empty or `expires < now`.
- Request decoration: strip any inbound `Authorization`; set
  `Authorization: Bearer <access>`; set `ChatGPT-Account-Id: <accountId>` where
  `accountId` is extracted from the id/access JWT claims
  (`chatgpt_account_id` / `organizations[0].id`); rewrite `/v1/responses` or
  `/chat/completions` to the Codex endpoint; add `originator`, `User-Agent`,
  `session-id`. The OAuth model set is the gpt-5.x family.

**GitHub Copilot** — `inspirations/opencode/packages/opencode/src/plugin/github-copilot/copilot.ts`:

- Client id `Ov23li8tweQw6odWQebz`; device-code POST
  `https://github.com/login/device/code` with scope `read:user`; poll
  `https://github.com/login/oauth/access_token` with grant_type
  `urn:ietf:params:oauth:grant-type:device_code`; handle `authorization_pending`
  and `slow_down` per RFC 8628 with a small safety margin.
- API base `https://api.githubcopilot.com` (enterprise:
  `https://copilot-api.<normalized-enterprise-domain>`); deployment-type select
  prompt (GitHub.com vs Enterprise) with enterprise-URL validation.
- Request headers: `Authorization: Bearer <token>`, `x-initiator: agent|user`,
  `Openai-Intent: conversation-edits`, `Copilot-Vision-Request: true` for image
  requests, `User-Agent`. Verify whether the GitHub token must be exchanged for a
  short-lived Copilot token against
  `inspirations/opencode/packages/opencode/src/plugin/github-copilot/` (models +
  the `github-copilot-models` test) before relying on it directly; implement
  whichever the reference proves.

**Auth orchestration & storage** — opencode separates the auth *method* contract
(`packages/opencode/src/provider/auth.ts`: `methods`/`authorize`/`callback`, with
`oauth` vs `api` kinds and prompt schemas) from the credential *store*
(`packages/opencode/src/auth/index.ts`, `OAUTH_DUMMY_KEY`, persisted outside
runtime config). Mirror that separation: a credential store distinct from
`harness.json`, persisted in the platform data dir with restrictive permissions,
never in the event log or support bundle.

### 3.4 Normative architecture decisions for this PRD

These decisions remove the hidden choices most likely to make independent agents
diverge. Treat them as part of the spec unless a cited reference proves them
wrong.

- **OpenAI-compatible transport remains the only execution path** for Codex and
  Copilot. Provider-specific code may decorate requests, resolve credentials,
  rewrite base URLs, and expose auth/model metadata; it must not fork streaming,
  tool-call handling, event emission, retry behavior, or response parsing away from
  the existing OpenAI-compatible adapter.
- **Credential precedence is deterministic:** stored `oauth` credential first,
  stored `api_key` credential second, then `apiKeyEnv`, then inline `apiKey`. This
  lets a user upgrade to OAuth without deleting config, while preserving existing
  API-key setups when no stored credential exists. `harness auth logout` removes
  only stored credentials; it never edits `harness.json` or unsets environment
  variables.
- **One active stored credential per auth provider for V1.** Re-running
  `harness auth login <provider>` replaces that provider's active stored OAuth or
  stored API-key credential. Multiple-account selection, named credential profiles,
  and account switching UIs are post-V1 unless a reference-parity screen requires a
  minimal active-account picker.
- **Provider request context is explicit.** The OpenAI-compatible request path must
  have access to a stable session id, a per-request id, an initiator (`agent` vs
  `user`), whether the request includes images/media, and the configured cache
  retention. Do not infer these from prompt text, model id substrings, or global
  mutable state.
- **Sensitive metadata is redacted like secrets.** Account ids, enterprise domains,
  device codes, user codes, OAuth authorization codes, refresh tokens, access
  tokens, cookie-like values, and bearer headers must be registered with the
  redactor before they can appear in doctor output, events, support bundles,
  snapshots, or errors. Account ids may be persisted as credential metadata only
  when needed for request decoration; they must not be written to event logs.
- **Provider auth outcomes are separate from transport health.** `doctor` may report
  configured/stored/missing/expired/refresh-needed auth state using redacted
  values, but it must not make live provider calls to prove the bearer works. Live
  provider execution belongs to live prompt/signoff lanes only.
- **Prompt assets are data, not source branding.** Prompt-family assets and skill
  bundled resources may borrow reference behavior and structure, but must be
  harness-authored text with upstream branding, repository names, and unsupported
  tool claims removed.
- **Reference parity is observable parity.** Snapshot and PTY evidence should assert
  screen sequence, labels, grouping, keyboard behavior, focus movement, and
  redaction. Do not depend on pixel comparison, terminal-specific colors, or copied
  reference source.

### 3.5 Public contracts and shared seams

Use this section for decisions opencode/pi-mono/OMO cannot make for Harness.
These are public contracts or cross-workstream seams, not implementation
micro-design. If implementation discovers a reference conflict, update this PRD
and the evidence log before changing behavior.

- **Stable auth-provider ids:** the V1 built-in auth provider ids are `codex` and
  `github-copilot`. Use these ids in `harness auth`, stored credentials, doctor
  output, roadmap/evidence rows, and provider-auth tests. Existing arbitrary
  `providers.<id>` config entries may opt into one of these built-in auth behaviors
  through `authProvider`; do not introduce extra aliases such as `copilot` or
  `openai-codex` unless docs, schemas, CLI tests, and evidence rows are updated.
- **Provider config keys:** extend existing `providers.<id>`
  `openai_compatible` config entries; do not add a new top-level auth config area.
  Canonical public keys are `authProvider` (`codex`/`github-copilot`, optional) and
  `cacheRetention` (`short`/`long`/`none`, optional). Existing `apiKeyEnv` and
  inline `apiKey` stay supported as config fallbacks. OAuth tokens, refresh tokens,
  user codes, and stored API-key secrets never appear in `harness.json{,c}`.
- **Cache-retention semantics:** the default is `short`, matching pi-mono's cache
  optimization posture. `none` omits `prompt_cache_key`, `prompt_cache_retention`,
  and cache-affinity headers. `short` sends a clamped `prompt_cache_key` and any
  session/request affinity headers supported by the target path. `long` additionally
  sends `prompt_cache_retention: "24h"` only when the target is a direct
  OpenAI-compatible API that accepts it; proxies or capability metadata that disable
  long retention must silently degrade to `short` behavior and report that in
  doctor/readiness.
- **Credential kinds and precedence:** the secure credential store supports
  `oauth` and `api_key` stored credentials. Resolution order is stored OAuth,
  stored API key, `apiKeyEnv`, then inline `apiKey`. `harness auth login` with an
  API-key method stores an `api_key` credential in the secure store; it never edits
  config. `harness auth logout` deletes stored credentials for the auth-provider id
  only, so the next request naturally falls through to `apiKeyEnv` or inline
  `apiKey` if configured.
- **Auth method availability is provider-declared:** onboarding and `auth login`
  show only methods the selected auth provider supports. Codex supports browser
  PKCE, device-code, and API-key entry. GitHub Copilot supports device-code for V1;
  do not show an API-key method for Copilot unless a cited reference proves one.
- **Credential store layout:** store one JSON file per auth-provider id under the
  existing platform data-dir helper at `credentials/{auth_provider_id}.json`.
  The schema is versioned and includes `version`, `provider`, `kind`, secret
  material for the kind (`accessToken`/`refreshToken` or `apiKey`), `expiresAt`
  as RFC3339 when known, optional `accountId`, optional `enterpriseUrl`, optional
  `scopes`, and `updatedAt`. Writes are atomic. POSIX files are `0600`; Windows
  ACLs restrict access to the current user. Tests may substitute a temp data dir.
- **One active stored credential per auth-provider id:** re-running login for
  `codex` replaces the active stored `codex` credential, whether the new credential
  is OAuth or API-key. Re-running login for `github-copilot` does the same for
  Copilot. Multi-account pickers and named credential profiles remain post-V1.
- **Provider request context owner:** add an explicit `ProviderRequestContext` on
  the provider-facing `CompletionRequest` boundary. The coordinator constructs it;
  providers consume it. Required fields are `session_id`, `request_id`, `initiator`
  (`agent`/`user`), `has_media`, and `cache_retention`. Codex/Copilot decoration,
  cache-key generation, affinity headers, and vision headers consume this context;
  they must not re-infer those facts from prompt text or model ids.
- **Model-resolution seam surface:** WS2 produces a small Harness-native model
  resolution API with `ModelFamily`, `ModelCapabilities`, `ModelResolution`, and a
  resolver entry point. It must expose family, context limits, reasoning/thinking
  support, tool-call support, vision support, cache-retention support, parameter
  compatibility, and fallback result. Existing prompt composition and provider
  request decoration consume this result instead of substring-parsing model ids.
- **Prompt-family asset path:** first-party family prompt bodies live under
  `.agent-harness/prompt-families/{family}.md`, with `{family}` matching the WS2
  `ModelFamily` id. The composed default prompt may remain a fallback, but shipped
  Anthropic/Gemini/Copilot-family prompts must be data assets with drift tests.
- **Bundled skill resource contract:** keep the existing skill frontmatter key
  `resources` and its aliases. For V1, it is a comma- or newline-separated list of
  relative file paths under the skill root; directories and globs are out of scope.
  Initial caps: max 5 files per skill load, max 64 KiB per file, max 200 KiB total
  loaded bytes, max path depth 4 under the skill root. Absolute paths, `..`, and
  symlink escapes are rejected before reading.
- **First-run readiness predicate:** onboarding appears only when the selected
  provider has no usable stored credential, no resolvable `apiKeyEnv`, and no inline
  `apiKey`. A stored OAuth credential is usable when unexpired or refreshable; if
  refresh fails, auth is not usable and onboarding/auth recovery appears. Mock/test
  providers configured for deterministic lanes count as usable for those lanes.
  Skipping onboarding records no persistent skip flag; it only bypasses that TUI
  launch and never creates or mutates credentials.
- **Onboarding screen inventory:** WS6 snapshots cover start/splash, provider pick,
  auth-method pick, Codex browser/device login, Copilot public/enterprise device
  login, API-key entry, login success, login error/timeout, skip confirmation, and
  first-prompt success. Each snapshot asserts labels, grouping, focus, key hints,
  redaction, and harness branding substitution.

### 3.6 Skill hardening: bundled resources are deferred

[`docs/extension-strategy.md`](extension-strategy.md) (Markdown skills) and
[`docs/roadmap-v1.md`](roadmap-v1.md) (Skill depth) state bundled
references/assets "remain deferred metadata/follow-up, not runtime-loaded in this
slice." The README notes skill loading already rejects symlink-unsafe skills.
Hardening = make bundled-resource loading real with documented limits, and add
explicit symlink-escape / path-traversal tests across configured roots.

### 3.7 Onboarding: doc-only today

The checked first-run roadmap items describe documentation (copying
`harness.jsonc`), not an interactive flow. opencode's interactive provider/auth
selection lives in its TUI provider dialog and `cli/cmd/providers.ts`. Adapt the
*UX* (provider pick → login method → first prompt → visible success), not the
code or branding.

---

## 4. Workstreams

Each workstream lists required outcomes and the red/green tests that prove them.
You choose the implementation. Deterministic tests must not hit the network.

### WS1 — OpenAI-compatible prompt-cache parity

Required outcomes:

- OpenAI-compatible requests set a stable, per-session `prompt_cache_key`
  (clamped to the provider max, mirroring `clampOpenAIPromptCacheKey`), derived
  from the explicit request-context session id so repeated turns in one session
  share the key. If no stable session id exists, omit the key rather than inventing
  an unstable one.
- The composed system prompt keeps volatile fields (today's date, git branch) at
  the **tail** of the stable prefix (or relocates them out of the cached system
  prefix) so a branch switch or date rollover does not invalidate the whole
  prefix. The stable identity/instructions remain first, and the ordering is
  asserted by tests rather than left to snapshot coincidence.
- Cache read/write token counts are surfaced in an operator-visible TUI status
  area, derived from the existing event fields (no new provider calls). The status
  text must distinguish read vs write/cache-creation tokens so a user can tell
  whether the cache is being reused.
- The `cacheRetention` config key (`short`/`long`/`none`) follows §3.5 exactly:
  default `short`, `none` omits cache fields, `short` sends the clamped key, and
  `long` adds provider-supported long retention only where allowed. This rounds the
  OpenAI-compatible path to pi-mono parity; the Anthropic `cache_control`/TTL half
  of pi-mono's behavior is honestly deferred to the post-V1 native Anthropic
  transport (§9), not silently dropped.

Tests:

- Red→green provider test: a built request for an OpenAI-compatible model includes
  `prompt_cache_key` equal to the clamped session key; two requests in the same
  session use the same key; two different sessions use different keys; the key is
  omitted/empty-safe when no session id exists or retention is `none`. "breaks if:"
  the request builder drops, cross-contaminates, or destabilizes the key.
- Prompt-composition test: volatile env fields appear after the stable prefix
  content; assert ordering. "breaks if:" date/branch move ahead of stable text.
- TUI/view-model test: cache token counts from a fixture event render in the
  status surface with separate read/write labels. "breaks if:" the surface stops
  reflecting `cache_*_tokens` or collapses read/write semantics.

### WS2 — Model family/capability resolution seam

Required outcomes:

- Family/capability resolution flows through the §3.5 seam keyed on model metadata
  (provider + model id/variant + capability flags), not only raw
  `model_id.contains(...)` scans. Prompt selection, cache-retention eligibility,
  vision/header behavior, reasoning/thinking settings, and context budgeting
  consume that seam instead of re-parsing model strings locally.
- Parity target: the seam must carry the *behavior* OMO's `model-core` exposes that
  the harness actually consumes, not a thin family-name lookup. At minimum:
  (a) family detection (`model-family-detectors`), (b) capability flags the runtime
  branches on — reasoning/thinking support, tool-call support, vision, long-cache
  support, parameter compatibility (`model-capabilities`,
  `model-settings-compatibility`), (c) context-limit resolution
  (`context-limit-resolver`) so compaction/budgeting use real per-model limits, and
  (d) a documented fallback chain (`fallback-chain-from-models`) for unknown or
  unavailable models. Port the *behavior* of these, reimplemented in Rust; do not
  port OMO's full bundled snapshot DB unless the harness needs it. Where the
  harness already has partial equivalents (e.g. `agent_catalog`, model resolution
  in `harness-core`), extend them rather than duplicating.
- Unknown models resolve to a documented default deterministically via the fallback
  chain, surfaced in doctor/readiness like the existing category fallback. The
  fallback result must include both family and capability defaults so downstream
  code never has to guess.
- The roadmap tension in §3.2 is reconciled: either the box's claim is now true
  (explicit seam + tests) with a citation, or it is reworded honestly.

Tests:

- Resolution table test over representative ids (gpt-5.x, gpt-5.x-codex, gpt-4*,
  claude*, gemini-*, kimi*, plus an unknown) asserting resolved family,
  capabilities, context limit, and default fallback. "breaks if:" a model maps to
  the wrong family/capabilities or the default path regresses.
- The existing dynamic-prompt golden/snapshot tests still pass (or are updated
  with cited justification).

### WS3 — Provider credential abstraction + secure store + refresh

Required outcomes:

- A credential abstraction yields a usable bearer/api-key for a provider and
  supports kinds: stored `oauth`, stored `api_key`, existing `apiKeyEnv`, and
  existing inline `apiKey`. Resolution follows the §3.5 precedence order. No new
  transport.
- Stored credentials persist in the §3.5 JSON credential store **outside**
  `harness.json`, in the platform data dir, with restrictive permissions (POSIX
  `0600`; on Windows an ACL restricting to the current user). The store is never
  written to `events.jsonl`, never included in support bundles except as a
  redaction-manifest entry, and never committed.
- Access tokens refresh automatically from the stored refresh token before/at
  expiry, using single-flight behavior per provider so concurrent requests do not
  stampede the token endpoint. Refresh failures map to the existing provider error
  categories (`invalid_credentials`, `transport_failure`, etc.) and stay
  operator-visible.
- All credential material and sensitive metadata listed in §3.4 route through the
  existing redaction path before any human-readable output, event projection, or
  support export can observe them.

Tests:

- Store round-trip test with a mocked clock: save/load `oauth` and `api_key`
  credentials, assert credential precedence, assert permissions are restrictive,
  assert atomic replacement, and assert the stored file is excluded from
  event/bundle surfaces. "breaks if:" creds land in events/bundle, precedence
  changes, partial writes survive, or perms loosen.
- Refresh test with a mocked token endpoint: expired access triggers exactly one
  refresh, persists the new tokens, and a concurrent second request reuses the
  in-flight refresh rather than double-refreshing. "breaks if:" refresh storms or
  stale tokens are reused past expiry.
- Redaction test: a credential value fed through the redaction path is masked in
  doctor output, event projections, and support export. "breaks if:" a token
  appears unmasked anywhere.

### WS4 — Codex (ChatGPT) OAuth provider

Required outcomes (OpenAI-compatible execution + decoration, per §3.3):

- PKCE loopback browser flow: generate verifier/challenge (S256), start the
  loopback callback server on the documented port when available, build the
  authorize URL with the documented params, exchange the code for tokens, validate
  `state`, time out safely, render success/error callback pages, and store the
  oauth credential.
- Headless device-code flow as an alternative method, with polling and final token
  exchange.
- Request decoration: strip inbound auth, set the bearer, set the account-id header
  from JWT claims, rewrite the request to the Codex endpoint, add originator /
  user-agent / session headers, and source session/request ids from the explicit
  request context. Confirm the live behavior against the cited reference before
  finalizing header names.
- The provider is selectable from config and login via the §3.5 `codex` auth
  provider id, and its model set reflects the allowed gpt-5.x family.

Tests (all mocked — no live OpenAI):

- PKCE test: verifier length/charset and challenge = base64url(SHA-256(verifier)).
- Loopback callback test: a simulated redirect with the correct `state` resolves to
  a token exchange against a mocked `/oauth/token`; a wrong/blank `state` is
  rejected as CSRF; missing code and timeout paths error cleanly with no credential
  stored. "breaks if:" state validation, timeout handling, or exchange shape
  regresses.
- Device-flow test: a mocked usercode→poll→exchange sequence (including a pending
  poll) yields a stored credential.
- Decoration test: a built request has the bearer, account-id header, session
  header, request-context metadata, and rewritten endpoint; no inbound
  Authorization survives. "breaks if:" the endpoint rewrite, request context, or
  header set changes.

### WS5 — GitHub Copilot OAuth provider

Required outcomes (per §3.3):

- GitHub device-code flow for the §3.5 `github-copilot` auth provider id, with
  public and enterprise deployment options (deployment-type selection + enterprise
  URL validation + domain normalization), polling that honors `authorization_pending`
  / `slow_down` with a safety margin, and credential storage. Before implementation,
  re-read the cited opencode Copilot plugin and tests and make a recorded decision:
  direct GitHub token as Copilot bearer, or GitHub→Copilot token exchange if the
  reference proves it is needed.
- Request decoration with the documented Copilot headers (`x-initiator`,
  `Openai-Intent`, `Copilot-Vision-Request` when images are present, user-agent),
  and the correct API base for public vs enterprise. `x-initiator` and the vision
  header must come from explicit request context, not from model-name or prompt-text
  heuristics.
- Model list resolution from the provider (or a sane fallback when offline),
  surfaced through the existing catalog/model surfaces.

Tests (all mocked — no live GitHub):

- Device-flow state-machine test: pending → slow_down (interval increase) →
  success, with the safety margin applied; timeout, access_denied/expired_token, and
  malformed responses fail cleanly with no credential stored.
- Enterprise normalization/validation test for representative URL/domain inputs.
- Decoration test: public vs enterprise base URL selection and the required header
  set; `x-initiator` reflects agent vs user origin from request context; vision
  header only on image requests. "breaks if:" base selection, request context, or
  headers regress.

### WS6 — First-run onboarding (exact opencode parity) + always-available `auth` UX

This workstream carries the two non-negotiable parity targets from §0.6.

Required outcomes — onboarding must **function and look exactly like opencode's**:

- The first-run onboarding flow must be a faithful, visual-parity reimplementation
  of opencode's onboarding: the same screen sequence, layout, framing, ordering of
  choices, focus/selection behavior, key hints, empty/loading/error states, and
  overall visual design — built natively in Ratatui, with harness identity
  (name/logo/colors) substituted for opencode's name/logo only. This is *visual +
  behavioral* parity, not a loose "inspired by" adaptation, and not a copy of
  opencode source code.
- Derive the exact screens from the opencode references: the start/splash screen
  (`inspirations/opencode/packages/opencode/src/cli/cmd/run/splash.ts`), the
  provider dialog and model dialog
  (`inspirations/opencode/packages/opencode/src/cli/cmd/tui/component/dialog-provider.tsx`,
  `.../dialog-model.tsx`), and the account/auth command
  (`inspirations/opencode/packages/opencode/src/cli/cmd/account.ts`). Compare the
  result side by side against the parity screenshots in
  `inspirations/screenshots opencode ui parity/Opencode/` (start screen, command
  menus) and the images in `inspirations/opencode-ui-images/`. If a screen differs
  from the opencode reference in a way that is not pure branding substitution, it
  is not done.
- Flow: provider selection → provider-declared auth method (browser/device/api key)
  → login (WS3–WS5) → first prompt → visible success signal. The flow follows the
  §3.5 readiness
  predicate, is skippable for the current launch, and must never block an expert
  user who already configured `harness.json` or a usable environment credential.
  Skipping records no credential and routes to the existing configured-provider
  path.

Required outcomes — `auth` UX must be available **at any time**, not just first run:

- `harness auth login [provider]`, `harness auth logout [provider]`, and
  `harness auth list` CLI commands exist and run at any time, independent of
  onboarding. `login` drives the WS3–WS5 flows (browser/device/api-key method),
  re-running `login` replaces that provider's active stored credential per §3.5,
  `list` shows configured providers and redacted auth status, and `logout` removes
  only stored credentials without editing config or environment.
- The same auth flows are reachable any time from inside the running TUI (a slash
  command and/or `Ctrl+p` command-palette entry such as `/login` or `/auth`), using
  the centralized command-palette metadata seam rather than ad-hoc key handling, so
  a user can add, switch, refresh, or remove a provider mid-session without
  restarting. Onboarding is one entry point into these flows, not a separate
  implementation.
- Doctor reports per-provider auth status (kind, presence, expiry where known)
  separately from provider transport health, without printing secrets.
- Skill listing/selection UX in the TUI matches the opencode skill surface
  (naming, grouping, description display, selection behavior) at visual parity, the
  same standard as the onboarding screens above.

Tests:

- In-process CLI tests (using `CliIo`/`CliDeps` per the harness test pattern) for
  `auth list`/`logout`/`login` with a mocked auth backend, proving they run outside
  onboarding and never print a secret. "breaks if:" auth becomes first-run-only or
  a token reaches stdout.
- Deterministic PTY/snapshot tests for every §3.5 onboarding screen (start/splash,
  provider pick, auth-method pick, Codex browser/device login, Copilot
  public/enterprise device login, API-key entry, success, login error/timeout, skip
  confirmation, first-prompt success) asserting layout/content/focus/key-hint parity
  with the reference screens, and that onboarding is skipped when a credential
  already exists. "breaks if:" a screen drifts from the reference layout/behavior or
  onboarding blocks a pre-configured user.
- TUI test proving the auth flow is invokable mid-session from the command palette
  / slash command and routes to the same backend as the CLI. "breaks if:" in-TUI
  auth diverges from the CLI flow or is unavailable after startup.
- Doctor test asserting per-provider auth status lines with redacted values.

### WS7 — Non-GPT family prompt parity, data-sourced (depends on WS2)

Required outcomes:

- Non-GPT family prompts (at minimum Anthropic and Gemini, plus any families a
  Copilot model exposes) are brought to OMO-parity quality: branding stripped,
  unsupported-tool claims removed, the shared prompt skeleton honored, and behavior
  mapped to real harness seams.
- Model-family prompt bodies are sourced from the §3.5 data asset path rather than
  hardcoded Rust `const &str`. The WS2 seam selects the asset, and missing assets
  fail closed to the documented default prompt with a doctor/readiness warning.
- A drift test fails if a referenced family-prompt asset is missing, empty, or
  contains forbidden upstream branding markers, and golden snapshots cover the
  composed prompt for each family.

Tests:

- Golden/snapshot tests for each shipped family's composed prompt (branding-free,
  unsupported-tool claims absent, skeleton sections present). "breaks if:" a
  family prompt loses a required section, claims unavailable tools, or reintroduces
  source branding.
- Drift test for missing/empty family-prompt assets. "breaks if:" an asset
  referenced by the seam is absent.

### WS8 — Skill hardening

Required outcomes:

- Bundled skill references/assets load via progressive disclosure using the §3.5
  `resources` contract and caps, instead of remaining deferred-only metadata. The
  loaded content is summarized/capped like other skill content, preserves the
  parent-visible child summary redaction/capping contract, and respects coordinator
  permissions.
- Skill discovery has explicit symlink-escape and path-traversal tests across all
  configured project/global roots and bundled-resource path shapes, proving a skill
  cannot read or inject content from outside its root.
- Docs in [`docs/extension-strategy.md`](extension-strategy.md) and the skill
  authoring guide are updated to match the new behavior and limits.

Tests:

- Bundled-resource load test: a skill with a bundled reference loads it within the
  §3.5 caps and surfaces it through the normal skill path with summary caps
  applied. "breaks if:" bundled content loads unbounded, bypasses permissions, or
  skips the normal redaction/capping path.
- Escape tests: a symlinked, absolute, or `..`-traversing bundled path is rejected
  for every configured skill root. "breaks if:" traversal/symlink/absolute-path
  escape is permitted.

---

## 5. Acceptance criteria

Cache and prompts:

- [x] OpenAI-compatible requests set a stable, clamped, per-session
  `prompt_cache_key` from `ProviderRequestContext`, and omit it for missing session
  ids or `cacheRetention = none`, proven by a provider test.
- [x] The composed system prompt keeps volatile env fields at the prefix tail,
  proven by a composition-order test.
- [x] Cache read/write tokens are surfaced separately in a TUI status surface,
  proven by a view-model/render test.
- [x] The `cacheRetention` config setting follows §3.5 exactly: default `short`,
  `none` omits cache fields, `long` adds only provider-supported long-retention
  fields, and the deferred Anthropic half is documented rather than dropped.
- [x] Model family/capability resolution uses an explicit tested seam carrying
  OMO `model-core` behavior depth (family detection, capability flags,
  context-limit resolution, fallback chain), not only substring scans, with a
  default-fallback test.
- [x] The roadmap substring-heuristic/preset claim is reconciled honestly with a
  citation.
- [x] Non-GPT family prompts meet the skeleton + branding-free +
  unsupported-tool-claims-free bar, sourced from data assets, with golden snapshots
  and a drift test.

Provider auth:

- [x] A credential abstraction supports stored `oauth`, stored `api_key`,
  `apiKeyEnv`, and inline `apiKey` with the §3.5 precedence order and without a new
  transport, proven by tests.
- [x] Stored credentials persist outside `harness.json` using the §3.5 JSON schema
  and restrictive permissions; `auth logout` removes only stored credentials; and
  credentials never appear in events/bundles/commits, proven by a store test and the
  secret-scan gate.
- [x] Access tokens auto-refresh with single-flight behavior and map failures to
  existing error categories, proven by a mocked-endpoint refresh test.
- [x] Codex OAuth (PKCE loopback + device-code), timeout/error handling, account-id
  redaction, and request decoration work, proven by mocked-flow and decoration
  tests.
- [x] GitHub Copilot OAuth (device-code, public + enterprise), the recorded
  GitHub-token vs Copilot-token decision, and request decoration work, proven by
  mocked-flow and decoration tests.
- [x] `harness auth login/logout/list` exist, run at any time (not just first run),
  accept the §3.5 auth-provider ids, support re-login/provider replacement under the
  one-active-credential rule, preserve config/env credentials on logout, and never
  print secrets, proven by in-process CLI tests.
- [x] The same auth flows are invokable mid-session from the TUI command
  palette/slash command via the centralized command metadata seam, proven by a TUI
  test routing to the same backend as the CLI.
- [x] The first-run onboarding flow functions and looks at visual parity with the
  §3.5 screen inventory and opencode's onboarding, with harness branding substituted
  only, proven by PTY/snapshot tests compared against the reference screens; it is
  skippable for the current launch and never blocks a pre-configured user.
- [x] TUI skill listing/selection UX is at visual parity with the opencode skill
  surface, proven by a snapshot test.
- [x] Doctor reports per-provider auth status (kind/presence/expiry) with redacted
  values, proven by a doctor test.

Skills:

- [x] Bundled skill resources load via the §3.5 `resources` grammar and caps within
  the normal redaction/summary path, proven by a load test.
- [x] Symlink-escape / path-traversal / absolute-path escape across skill roots is
  rejected, proven by escape tests.

Cross-cutting:

- [x] Every roadmap box touched by this PRD is updated honestly with a citation.
- [x] `docs/config.md`, generated schemas, and examples document the §3.5 public
  config keys and no extra public aliases ship without tests.
- [x] No deterministic test performs a real network call to any provider.
- [x] `docs/pre-v1-enhancements-progress.md` records evidence and a "breaks if:"
  line for every new test.
- [x] The live-OAuth end-to-end step is recorded as a manual env-gated signoff
  item, not faked and not part of the autonomous done-condition.

---

## 6. Verification gates

### 6.1 Targeted gates (run the narrowest lane that proves each change)

- [x] `cargo test -p harness-providers` (cache key, request context,
  Codex/Copilot decoration, credential precedence/refresh, mocked flows).
- [x] `cargo test -p harness` for dynamic-prompt composition, model-resolution
  seam and capability defaults, `auth` CLI commands, doctor auth status, and
  family-prompt golden/drift tests.
- [x] `cargo test -p harness-tools` for skill bundled-resource cap/redaction tests
  and escape tests (and `native_tool_parity_matrix_test` if any tool surface
  changed).
- [x] `cargo test -p harness-tui` for onboarding/status/auth-command-palette
  view-models, plus `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test
  pty_e2e` for the onboarding PTY screens.
- [x] `scripts/test-lanes.sh simulation` (secret-scan / simulation invariants) to
  prove no credential leaks.
- [x] `cargo run -p harness -- --config configs/harness.example.jsonc doctor`
  shows per-provider auth status with redacted values.

### 6.2 Workspace gates (all must pass at the end-state)

- [x] `cargo fmt --all -- --check`
- [x] `cargo check --workspace`
- [x] `cargo test --workspace --all-features`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`

### 6.3 Verification rules

- OAuth/transport claims require mocked-endpoint or cassette tests covering success,
  pending/slow-down, timeout, denied/error, refresh, and decoration paths; never
  live network in deterministic lanes.
- Secret-safety claims require the simulation/secret-scan gate to pass.
- Prompt claims require golden/snapshot + drift tests that also reject upstream
  branding and unsupported-tool claims.
- Permission/path-safety claims require tests exercising project root, global root,
  symlink, `..`, absolute-path, and bundled-resource selector shapes.
- A green readiness/doctor report does not by itself prove a live login worked;
  that stays a manual signoff item.

---

## 7. Progress and evidence requirements

Create and maintain `docs/pre-v1-enhancements-progress.md`. For every acceptance
criterion in §5 and every roadmap box this PRD touches, record:

- The exact criterion / checkbox text.
- The changed files.
- The evidence type (test, lane, command, docs-reference check, or documented
  limitation) and the exact command run.
- The observed result.
- The source citation proving the claim is now true (or justifying a reword).
- For every new test, a one-line "breaks if:" statement.

For roadmap claims, also update [`docs/claim-evidence-matrix.md`](claim-evidence-matrix.md)
consistent with the existing readiness-evidence posture. Do not invent a parallel
evidence system; reuse these two documents.

---

## 8. Suggested sequencing and commit strategy

Dependency graph: WS1, WS2, WS8 are independent and can start immediately. WS3 is a
prerequisite for WS4 and WS5. WS6 depends on WS3–WS5. WS7 depends on WS2. The §3.5
request-context contract is a shared dependency for WS1, WS4, and WS5. Use small,
reviewable commits; each commit passes its targeted gates before the next.

Recommended order (reorder independents freely if one is blocked):

1. **WS1 cache parity** — smallest, highest leverage, no dependencies.
2. **WS2 model-resolution seam** — unblocks WS7 and reconciles a checked box.
3. **WS8 skill hardening** — independent, bounded.
4. **WS3 credential framework + store + refresh** — foundation for auth.
5. **WS4 Codex OAuth** then **WS5 Copilot OAuth** — on top of WS3.
6. **WS7 non-GPT prompt parity** — on top of WS2.
7. **WS6 onboarding + auth CLI/TUI UX** — on top of WS3–WS5, last because it ties
   the auth providers into the operator experience.

After the final workstream, run all §6.2 workspace gates, the §6.1 targeted gates
for every changed area, update the roadmap boxes honestly, and confirm the §0.1
end-state holds in full before declaring completion.

---

## 9. Out of scope (post-V1 — building these is scope violation, not progress)

- OS-level execution sandbox for build/plan (Landlock+seccomp / Seatbelt);
  Windows has no good primitive and the operator permission layer already exists.
- Native Anthropic transport with `cache_control` ephemeral breakpoints / TTL
  gating (coupled to provider-transport work not started; Copilot's Anthropic
  models route through the OpenAI-compatible/Responses shim and do not require it).
- `previous_response_id` server-side context reuse (needs replay-safety design for
  server-held state).
- Any provider beyond Codex and GitHub Copilot, including regional/Chinese
  providers; those are deferred.
- Standardized external `.mcp.json`-style MCP import.
- Harness logo / brand redesign (human-owned design task, not an autonomous-agent
  deliverable).
- TUI reference-image comparison.
- Reopening unrelated roadmap percentages, denominators, or unchecked future work.
- Cosmetic refactors and backward-compat shims without a concrete runtime need.

---

## 10. Reference map

Harness seams to respect (re-read before editing):

- `crates/harness-providers/src/openai.rs`, `crates/harness-providers/src/lib.rs`,
  `crates/harness-providers/AGENTS.md` — transport, request build,
  `ProviderRequestContext`, credential injection, cache telemetry.
- `crates/harness/src/dynamic_prompt.rs` — prompt composition, `provider_prompt`
  substring selection, environment block ordering.
- `crates/harness-core/src/event.rs` — cache token fields already present.
- `crates/harness-core/src/config/` + `docs/config.md` + `configs/*.json{,c}` —
  public config contract for adding an OAuth credential kind and auth settings.
- `crates/harness-core/src/redact.rs` — redaction path all credentials and
  sensitive metadata must use.
- `crates/harness/src/doctor.rs` — per-provider auth status reporting.
- `crates/harness-tools/src/skill_catalog.rs` — skill discovery/loading for WS8.
- `crates/harness-tui/` — onboarding flow and cache-status surface.

Inspiration references (read-only; copy behavior, never code/branding):

- Caching (authority: pi-mono):
  `inspirations/pi-mono/packages/ai/src/providers/openai-prompt-cache.ts`,
  `.../openai-responses.ts`, `.../openai-completions.ts`,
  `inspirations/pi-mono/packages/ai/src/oauth.ts`.
- OAuth flows (authority: opencode):
  `inspirations/opencode/packages/opencode/src/plugin/openai/codex.ts`,
  `.../plugin/github-copilot/copilot.ts`,
  `.../provider/auth.ts`, `.../auth/index.ts`, `.../cli/cmd/account.ts`.
- Onboarding + auth UX visual parity (authority: opencode): the screen sources
  `inspirations/opencode/packages/opencode/src/cli/cmd/run/splash.ts`,
  `.../cli/cmd/tui/component/dialog-provider.tsx`,
  `.../cli/cmd/tui/component/dialog-model.tsx`, plus the parity image sets
  `inspirations/screenshots opencode ui parity/Opencode/` (and the matching
  `.../Harness project/` for current-state comparison) and
  `inspirations/opencode-ui-images/`. These images are the visual acceptance
  reference for WS6.
- Model resolution (authority: OMO):
  `inspirations/oh-my-openagent/packages/model-core/src/` — `model-family-detectors`,
  `model-capabilities`, `model-settings-compatibility`, `context-limit-resolver`,
  `fallback-chain-from-models`, `variant-resolver`, `model-resolution-pipeline`.
