# Provider support

Harness supports OpenAI-compatible HTTP, Anthropic Messages, and Claude
subscription turns through a locally installed Claude Code binary. The provider
type selects the backend. A catalog model still needs valid credentials and
endpoint access.

| Provider ID | Execution path | Credential source |
| --- | --- | --- |
| `openai` | Responses or Chat Completions | Stored key or `OPENAI_API_KEY` |
| `openai-codex` | Codex Responses | Stored `codex` OAuth credential |
| `github-copilot` | Responses or Chat Completions; Claude uses Messages in automatic mode | Stored device-login credential or `GITHUB_TOKEN` |
| `anthropic` | Anthropic Messages | Stored key or `ANTHROPIC_API_KEY` |
| `anthropic-subscription` | Local Claude Code process | Stored account pool or `CLAUDE_CODE_OAUTH_TOKEN` slots |
| `google` | Google's OpenAI-compatible Chat Completions endpoint | Stored key, `GOOGLE_GENERATIVE_AI_API_KEY`, or `GEMINI_API_KEY` |
| `openrouter` | OpenAI-compatible HTTP | Stored key or `OPENROUTER_API_KEY` |
| `mistral` | Configured OpenAI-compatible endpoint | Stored key or configured `MISTRAL_API_KEY` |

These are examples, not a fixed provider allowlist. Automatic discovery uses
the embedded models.dev IDs, environment names, and endpoints. Entries without
an endpoint are skipped. The bundled Mistral entry has no endpoint or built-in
endpoint fallback, so configure its `baseURL` and models explicitly. There is
no native Google or Mistral SDK transport.

`harness providers protocols` reports the two HTTP protocol families, not
connected accounts or the Claude Code backend. `harness models` lists the
resolved catalog, variants, limits, and per-field limit provenance.

## Execution path

The coordinator sends requests through `harness-providers`, which normalizes
backend streams into common events. Deterministic tests use mock providers. Live
tests require explicit environment settings.

## Default model selection

A connected provider uses its first available curated default. Codex and Claude
subscription have their own lists. Codex prefers `gpt-6-astra`, then
`gpt-6-sol`, `gpt-6-luna`, and `gpt-5.5`. Claude subscription prefers
`claude-opus-5-5`, then `claude-sonnet-5-5` and `claude-fable-5-1`. The generic
list starts with `gpt-5.4-mini`, `gpt-5.5`, and `claude-sonnet-4-6`.
Google prefers `gemini-3.1-pro-preview`, then
`gemini-3-pro-preview`, then `gemini-2.5-pro`. OpenRouter prefers
`anthropic/claude-sonnet-4.6`, then `openai/gpt-5.5`, then
`anthropic/claude-sonnet-4.5`.

Otherwise, selection prefers declared tool-call support, then IDs without a
`:free` suffix or `nano`, `lite`, `mini` or `alpha` tokens, then the newest
models.dev release date, then alphabetical order. A custom provider with models
but no catalog metadata still gets a default.


## Codex context profiles

Codex limits come from configured or catalog metadata, as they do for other
OpenAI-compatible providers. Runtime discovery does not apply a separate
GPT-5.6 input-window override. Use `harness models --json` to inspect limits
and provenance, or define explicit model limits.

## Codex subscription model availability

Codex authentication accepts only these subscription models: `gpt-6.1-sol`,
`gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-sol`, `gpt-5.6-terra`,
`gpt-5.6-luna`, and `gpt-5.5`. The source allowlist records a verification
date of October 4, 2026. Other IDs are filtered from runtime catalogs and
rejected before network requests, including custom providers using
`authProvider: "codex"`. Separate API-key providers are unaffected. There is
no date-based retirement check; account access can still differ.

Sources: [model availability](https://learn.chatgpt.com/docs/models#deprecated-codex-models), [Spark retirement](https://learn.chatgpt.com/docs/changelog).

## GPT-6 Astra

`gpt-6-astra` is available in the bundled Codex catalog. Select
`openai-codex/gpt-6-astra` in `/model`, or set it as the top-level `model` in
an optional `harness.jsonc`. First sign-in may write a starter with the provider's default model.
The supported reasoning variants are `low`, `medium`, `high`, `xhigh`, and `max`.
The bundled variants do not include `none`, `minimal`, or `ultra`.
Codex requests without an explicit reasoning effort default to `low` and retain
the encrypted reasoning-content request option used by the existing Responses
transport. Explicit reasoning and verbosity settings take precedence.

The bundled API model metadata records 1,050,000 context tokens, 922,000 maximum input tokens, and 128,000 maximum
output tokens. Catalog discovery preserves these limits. To set a smaller
working context, add an explicit model limit override:

```jsonc
"limit": { "context": 1050000, "input": 288384, "output": 128000 }
```

The compaction threshold never exceeds the request's input budget, so this caps
it at 288,384 tokens: `min(1050000 - max(15% of 1050000, 16384), min(288384,
1050000 - 128000))`. A lower explicit compaction threshold takes precedence.
The 128,000-token output reserve remains intact. Setting total `context` to
272,000 would leave only 144,000 tokens of input after the output reserve.
Explicit local model entries are not overwritten by catalog refreshes.
Availability depends on the account and endpoint; `harness doctor` is offline.

## GPT-6.1 Sol

`gpt-6.1-sol` is available in the bundled catalog and the reference
configuration. Select `openai-codex/gpt-6.1-sol` in the model picker, or set it
as the top-level `model` in an optional `harness.jsonc`.

Supported reasoning variants are `low`, `medium`, `high`, `xhigh`, and `max`.
Without an explicit reasoning setting, Codex requests default to `medium` and
include encrypted reasoning content. Explicit settings take precedence.
Tool calling uses the Responses API; `none` and `minimal` are not supported.

Bundled API limits are 1,050,000 context tokens, 922,000 maximum input tokens,
and 128,000 maximum output tokens. A custom 288,384-token input limit caps the
compaction threshold at that input budget. A lower explicit compaction threshold
takes precedence.
Account and workspace rollout still determine access.
See the [model specifications](https://developers.openai.com/api/docs/models/gpt-6.1-sol)
and [Codex availability](https://learn.chatgpt.com/docs/models#gpt-61-sol).

## Known limits

Provider execution requires one of the implemented backends above. Doctor
checks local configuration, connected-provider discovery, and model limits.
It does not prove authentication because it makes no provider call.

A local OpenAI-compatible endpoint still needs a configured base URL and model
definitions. Local protocol fixtures do not establish live account access.

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

Runtime selection, `harness models list`, and doctor use embedded metadata and
read the existing Codex cache without fetching or writing it.

Harness downloads `https://models.dev/api.json` into the cache at these
moments:

- When the TUI starts and when `harness prompt` or `harness run` starts a
  session, if the cache is missing, unreadable or older than two hours. The
  download runs in the background and never delays startup; that run keeps the
  cache it read, or the embedded catalog when there was none, and the next run
  uses the new download. A `prompt` or `run` that succeeds waits for an
  unfinished download before exiting, at most the 10-second download timeout.
- After every successful `harness auth login`, including sign-in from the TUI,
  whatever the cache's age, so models and prices a new sign-in unlocks appear
  in the model picker right away.
- On `harness models refresh`, whatever the cache's age.

A download times out after 10 seconds and is written atomically with mode
`0600`; a failed one keeps the previous cache. It accepts the direct models.dev
map and the generated catalog shape. `harness models`, `harness doctor` and
replay never download.

`HARNESS_MODELS_URL` and `HARNESS_MODELS_PATH` override the source and cache
location. `HARNESS_DISABLE_MODELS_FETCH=1` (or `true`) turns every download off,
`harness models refresh` included, and selects embedded data. Mock TUI
initialization uses embedded data directly. `harness models probe` and
`harness models generate` are separate explicit operations: they fetch their
`--url` or the default models.dev URL unless `--input` or `--stdin` is
supplied. They do not use the cache-loader environment overrides.
`harness models generated` prints the bundled catalog without fetching.

Runtime resolution merges cached OpenAI metadata into providers whose
credential identity is `codex`, including custom IDs, without replacing
explicit model entries. Only allowlisted subscription IDs are added. Unknown
live entries are not assumed to be supported.

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
| OutputLimitExceeded | `output_limit_exceeded` | Generation reaches the output limit. | Ask the model to continue, or increase the output limit. |
| UnsupportedToolCall | `unsupported_tool_call` | Provider/model cannot process requested tool call shape. | Check the tool schema and model capabilities. |
| MalformedStream | `malformed_stream` | Stream payload is invalid or incomplete. | Retry and keep event/support bundle evidence. |
| TransportFailure | `transport_failure` | Timeout, DNS, connection, TLS, or socket failure. | Check network/baseURL/proxy. |
| Other | `other` | Anything not classified above. | Inspect sanitized provider message and support bundle. |

## Error reporting

The coordinator stores provider categories in
`ProviderRequestFinished.metadata.provider_error_category` with
`provider_error_remediation`. HTTP failures use sanitized text such as
`provider returned HTTP 401`, not raw response bodies. Keep the category and
support bundle when reporting an error.

## Model fallback policy

OpenAI-compatible `auto` mode falls back from Responses to Chat Completions
after HTTP 404 or 405, or HTTP 400 from a loopback endpoint, before streaming
starts. Codex cannot use this fallback. Eligible provider failures can also
advance through an explicitly configured `model_profile.<name>.fallback`
chain. A switch applies the next model, variant settings, and resolved limits.
