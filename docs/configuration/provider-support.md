# Provider support

Harness implements OpenAI-compatible and Anthropic transports. The configured
provider type selects the backend. Catalog entries describe models; a listed
model still needs a supported transport, valid credentials, and endpoint access.

## Execution path

The coordinator sends requests through `harness-providers`, which normalizes
backend streams into common events. Deterministic tests use mock providers. Live
tests require explicit environment settings.

## Default model selection

A connected provider uses its first available curated default. Codex and Claude
subscription have their own lists; the generic list includes
`claude-sonnet-4-6`. Google prefers `gemini-3.1-pro-preview`, then
`gemini-3-pro-preview`, then `gemini-2.5-pro`. OpenRouter prefers
`anthropic/claude-sonnet-4.6`, then `openai/gpt-5.5`, then
`anthropic/claude-sonnet-4.5`.

Otherwise, selection prefers declared tool-call support, then IDs without a
`:free` suffix or `nano`, `lite`, `mini` or `alpha` tokens, then the newest
models.dev release date, then alphabetical order. A custom provider with models
but no catalog metadata still gets a default.


## Codex context profiles

Catalog-derived GPT-5.6 models on the built-in `openai-codex` provider use a 369,384-token maximum input profile. With the default 16,384-token compaction reserve, this exposes the Codex default usable context budget of 353,000 tokens. This applies to the Luna, Terra, and Sol tiers; the nonexistent unsuffixed `gpt-5.6` alias is not exposed. Other OpenAI-compatible providers retain their configured or discovered API limits.

## Codex subscription model availability

Codex authentication accepts only the verified subscription models: `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, and `gpt-5.5` (verified October 4, 2026). GPT-5.5 remains available until October 14, 2026. Retired models, Pro models, and unknown aliases are filtered from runtime catalogs and rejected before requests reach the network, including for custom provider IDs using `authProvider: "codex"`. Separate API-key providers are unaffected. Account and workspace access can further restrict this list.

Sources: [model availability](https://learn.chatgpt.com/docs/models#deprecated-codex-models), [Spark retirement](https://learn.chatgpt.com/docs/changelog).

## GPT-6 Astra

`gpt-6-astra` is available in the bundled Codex catalog. Select
`openai-codex/gpt-6-astra` in `/model`, or set it as the top-level `model` in
an optional `harness.jsonc`. First sign-in may write a starter with the provider's default model.
The supported reasoning variants are `low`, `medium`, `high`, `xhigh`, and `max`;
`none`, `minimal`, and Codex's multi-agent `ultra` mode are not offered.
Codex requests without an explicit reasoning effort default to `low` and retain
the encrypted reasoning-content request option used by the existing Responses
transport. Explicit reasoning and verbosity settings take precedence.

The bundled API model metadata records 1,050,000 context tokens, 922,000 maximum input tokens, and 128,000 maximum
output tokens. Catalog discovery preserves these limits. To set a smaller
working context, add an explicit model limit override:

```jsonc
"limit": { "context": 1050000, "input": 288384, "output": 128000 }
```

With the default `runtime.compaction.reserveTokens` of 16,384, this gives a
272,000-token compaction threshold: `min(288384, 1050000 - 128000) - 16384`.
The extra 16,384 input tokens preserve the safety margin; the 128,000-token output
reserve remains intact. Setting total `context` to 272,000 would instead leave
only 127,616 tokens after both reserves. Explicit local model entries remain
authoritative and are not overwritten by catalog refreshes.
Availability still depends on the account and endpoint;
`doctor` is offline, so a real prompt is required to verify access.

## GPT-6.1 Sol

`gpt-6.1-sol` is available in the bundled catalog and the reference
configuration. Select `openai-codex/gpt-6.1-sol` in the model picker, or set it
as the top-level `model` in an optional `harness.jsonc`.

Supported reasoning variants are `low`, `medium`, `high`, `xhigh`, and `max`.
Without an explicit reasoning setting, Codex requests default to `medium` and
include encrypted reasoning content. Explicit settings take precedence.
Tool calling uses the Responses API; `none` and `minimal` are not supported.

Bundled API limits are 1,050,000 context tokens, 922,000 maximum input tokens,
and 128,000 maximum output tokens. A custom 288,384-token input limit gives a
272,000-token compaction threshold with the default 16,384-token reserve.
Account and workspace rollout still determine access.
See the [model specifications](https://developers.openai.com/api/docs/models/gpt-6.1-sol)
and [Codex availability](https://learn.chatgpt.com/docs/models#gpt-61-sol).

## Known limits

Provider execution requires one of the implemented backend families above. Doctor validates local configuration and credential presence but does not prove authentication because it makes no provider call.

Local targets such as Ollama are optional manual checks. They are outside
`signoff-live`, CI defaults, and the quality gates.

## Credentials

No config file is needed. Connect with `/login`, `harness auth login <provider>`,
or an exported provider API key such as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
Without provider entries, Harness discovers stored credentials and API key
environment variables from its embedded models.dev catalog. Defining any
`provider` entry makes the catalog curated: configured providers plus signed-in
Codex, GitHub Copilot, and Claude subscription. Missing credentials are reported
without printing secret values. Invalid credentials and rate limits require
a live prompt because `harness doctor` stays offline.

## Model catalog refresh

Ordinary live catalog initialization refreshes model metadata from `https://models.dev/api.json` using a five-minute cache. Harness accepts both the direct models.dev provider map and the generated catalog shape, serves a valid stale cache immediately, and refreshes stale data in the background with an atomic, mode-`0600` cache write. Set `HARNESS_DISABLE_MODELS_FETCH=1` to keep the embedded catalog only; `HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache location. Without a usable cache, initialization attempts a download and falls back to embedded metadata on failure. Mock TUI model-picker initialization uses the embedded catalog directly and never invokes the environment-backed cache/refresh loader.

For the built-in `openai-codex` provider, refreshed OpenAI model metadata is merged into the configured Codex model list without replacing explicit entries. This lets newly published GPT models appear in `/model` while preserving local variants and provider settings. Only models on the verified subscription allowlist are added; unknown live entries are not assumed to be supported.

## Resolved model limits

`ResolvedModelLimits` is the runtime authority for context, maximum input, and maximum output tokens. `max_input` means provider-visible input tokens before generated output; it is not a percentage or a request-budget calculation. Each field records whether it came from explicit configuration, the generated catalog, provider discovery, or a compatibility fallback, together with its source and optional verification date.

A selectable known model must provide positive context and output values, with output no larger than context. `max_input` is an independent optional physical provider cap; when present it must be positive and no larger than context, and when absent its value and provenance remain unknown. A custom model may omit all three fields, and Harness does not infer a window from the model family. Variants replace only the fields they explicitly set. `harness models` prints every field and its per-field provenance.

## Stable error categories

| Category | Event value | Meaning | Remediation |
|---|---|---|---|
| MissingCredentials | `missing_credentials` | No usable API key or credential reference is present. | Set `apiKey` or the configured env var. |
| InvalidCredentials | `invalid_credentials` | Provider rejects authentication. | Rotate/check credentials and endpoint. |
| RateLimited | `rate_limited` | Provider rate or quota limit. | Wait, reduce load, or change account/model. |
| ContextWindowExceeded | `context_window_exceeded` | Request exceeds model context. | Compact, shorten prompt, or pick a larger context model. |
| UnsupportedToolCall | `unsupported_tool_call` | Provider/model cannot process requested tool call shape. | Use a tool-capable model or reduce tool request. |
| MalformedStream | `malformed_stream` | Stream payload is invalid or incomplete. | Retry and keep event/support bundle evidence. |
| TransportFailure | `transport_failure` | Timeout, DNS, connection, TLS, or socket failure. | Check network/baseURL/proxy. |
| Other | `other` | Anything not classified above. | Inspect sanitized provider message and support bundle. |

## Error reporting

The coordinator stores provider categories in `ProviderRequestFinished.metadata.provider_error_category` with `provider_error_remediation`. Headless `prompt` failures include the serialized category plus provider message in stderr, and the TUI activity/runtime state shows the category with a suggested action.

## Model fallback policy

OpenAI-compatible `auto` mode can fall back from the Responses API to Chat Completions when the configured transport reports that the Responses path is unsupported. Eligible provider failures may also advance through an explicitly configured `model_profile.fallback` chain; each switch applies the next typed target atomically, including its model ref, variant settings, and resolved limits.
