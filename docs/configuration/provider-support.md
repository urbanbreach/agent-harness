# Provider support

Harness V1 executes through the OpenAI-compatible provider path. Larger provider catalogs are metadata/reference unless the configured provider is an implemented OpenAI-compatible transport.

## Execution path

Provider requests flow through configured provider/model ids, the coordinator, and the `harness-providers` stream interface. Deterministic tests use mock/faux providers by default; live lanes are env-gated.

## Codex context profiles

Catalog-derived GPT-5.6 models on the built-in `openai-codex` provider use a 369,384-token maximum input profile. With the default 16,384-token compaction reserve, this exposes the Codex default usable context budget of 353,000 tokens. This applies to the Luna, Terra, and Sol tiers; the nonexistent unsuffixed `gpt-5.6` alias is not exposed. Other OpenAI-compatible providers retain their configured or discovered API limits.

## Codex subscription model availability

The built-in `openai-codex` catalog exposes GPT-5.4 and newer non-Pro models. Pro models are excluded because Codex subscriptions cannot use them. Models older than GPT-5.4 are also excluded, with `gpt-5.3-codex-spark` retained as the sole legacy exception.

## GPT-6 Astra

`gpt-6-astra` is available in the bundled Codex catalog and the shipped example
configuration. Select `openai-codex/gpt-6-astra` in `/model`, or set it as the
top-level `model` in `harness.jsonc`. The existing default model is unchanged.
The supported reasoning variants are `low`, `medium`, `high`, `xhigh`, and `max`;
`none`, `minimal`, and Codex's multi-agent `ultra` mode are not offered.
Codex requests without an explicit reasoning effort default to `low` and retain
the encrypted reasoning-content request option used by the existing Responses
transport. Explicit reasoning and verbosity settings take precedence.

The [OpenAI API model profile](https://developers.openai.com/api/docs/models/gpt-6-astra)
specifies 1,050,000 context tokens, 922,000 maximum input tokens, and 128,000 maximum
output tokens. Catalog discovery preserves these limits; Astra has no hardcoded
272k capacity override. The shipped example and workspace configuration instead
limit the working context through model configuration:

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

## Known limits

The runtime does not implement new provider protocols in this slice. Doctor validates local configuration and credential presence but does not prove authentication because it makes no provider call.

Optional local free live targets (for example Ollama) are **deferred** as a non-CI residual path.
They are not a CI default, not part of `signoff-live`, and not required for quality gates.
Document or script them only as maintainer-opt-in dogfood.

## Fallback policy

OpenAI-compatible `auto` mode may fall back from Responses API to Chat Completions when that transport path is unsupported. A configured `model_profile` fallback chain retains each target's resolved variant, reasoning settings, and model limits when a provider failure advances to the next model. Failures remain visible to the operator.

## Credentials

Use config/env-backed provider credentials. Missing credentials are reported without printing secret values. Invalid credentials and rate limits require live prompt evidence because doctor stays offline.

## Model catalog refresh

The bundled model catalog is refreshed from `https://models.dev/api.json` using a five-minute cache. Harness accepts both the direct models.dev provider map and the generated catalog shape, serves a valid stale cache immediately, and refreshes stale data in the background with an atomic, mode-`0600` cache write. Set `HARNESS_DISABLE_MODELS_FETCH=1` to keep the embedded catalog only; `HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache location.

For the built-in `openai-codex` provider, refreshed OpenAI model metadata is merged into the configured Codex model list without replacing explicit entries. This lets newly published GPT models appear in `/model` while preserving local variants and provider settings. Unknown live entries receive conservative metadata and the existing Codex model-id reasoning policy; a provider-specific model endpoint is not required for this catalog path.

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

## Surfacing

Provider categories are persisted in `ProviderRequestFinished.metadata.provider_error_category` with `provider_error_remediation`. Headless `prompt` failures include the serialized category plus provider message in stderr, and the TUI activity/runtime state shows the category with remediation so the operator can retry without reading raw provider payloads.

## Model fallback policy

OpenAI-compatible `auto` mode can fall back from the Responses API to Chat Completions when the configured transport reports that the Responses path is unsupported. Eligible provider failures may also advance through an explicitly configured `model_profile.fallback` chain; each switch applies the next typed target atomically, including its model ref, variant settings, and resolved limits.
