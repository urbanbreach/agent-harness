# Config reference

Config files are optional. Run `harness` and connect with `/login`,
`harness auth login <provider>`, or a provider API key environment variable.
Use `harness.json` or `harness.jsonc` for runtime overrides and `tui.json` or
`tui.jsonc` for keyboard settings. The generated schemas define the accepted keys:

- [Runtime schema](../../configs/config.json)
- [TUI schema](../../configs/tui.json)

For optional overrides, start with the [starter](#minimal-starter) and check [config precedence](#discovery-and-precedence).
The reference tables cover [runtime keys](#runtime-top-level-keys),
[keybindings](#tui-default-bindings), [permissions](#permission-policy),
[compaction](#provider-context-compaction-expectations), and [retries](#provider-retry-policy).

## Minimal starter

Put personal defaults in `~/.harness/harness.jsonc` (or under `HARNESS_HOME`)
and project policy in `<project>/harness.jsonc`. Only set values you need to
override. First sign-in can create a minimal personal config; see
[discovery and precedence](#discovery-and-precedence). For example:

```jsonc
{
  "$schema": "https://github.com/urbanbreach/agent-harness/releases/latest/download/harness.schema.json",
  "permission": {
    "edit": "ask"
  }
}
```

[`configs/harness.example.jsonc`](../../configs/harness.example.jsonc) adds
comments and an optional custom OpenAI-compatible endpoint. It does not pin a
model or define providers by default. `agent.default` configures the interactive
parent; native children use bundled definitions and the `subagents` settings
below.

Each `variants` entry is a named model preset; for reasoning models, set
`metadata.reasoningEffort` so the TUI can display and select variants
like `low`, `medium`, or `high`. Use additional variant fields only for
non-standard names or per-variant limits, modalities, or options.

For Anthropic Messages, an effort preset enables adaptive thinking on Claude 4.6
and newer, or a bounded thinking budget on older models. Explicit
`options.thinking` takes precedence over the generated thinking configuration.

HTTP provider `options.timeoutMs` limits idle reads, including the wait for
response headers. Each successful read resets the limit, so a progressing stream
can run longer. Connection setup has a separate limit of at most 30 seconds.

The live TUI shows generation speed and session cost next to context usage in
the row above the transcript, for example `49.6 tok/s · $0.047 · 9.9K / 192K 5%`.
Speed updates while the model streams. It counts streamed text, reasoning and
tool input at roughly four bytes per token, weights the last minute or so of
streaming most, and corrects itself to the provider's billed count when each
request ends. The clock only runs while a request is open, so the reading holds
through tool runs, and it stays blank until about 200 tokens and four seconds of
streaming are in. A resumed session starts from its last request's speed. Cost
adds up every request in the session, subagents included, priced by model.
Catalog models take their prices from models.dev. For a custom model, set `cost`
in USD per million tokens:

```jsonc
"models": {
  "my-model": {
    "cost": { "input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 3.75 }
  }
}
```

Prices are rounded to millionths of a USD per million tokens and must fit
0..4294.967295 at that precision. Cache reads and writes without their own
price are billed at the input rate. The
cost reads `$0.047+` when some usage came from a model with no price, and
`$0.047 (sub)` for a Codex or Copilot sign-in, where the bill isn't per token.
Narrow terminals drop speed first, then cost.

OpenAI-compatible providers accept `cacheRetention` either beside the provider
fields or under `options`. The default is `short`: the runtime sends a stable,
per-session `prompt_cache_key` when a session id is available. Set
`cacheRetention: "none"` to omit cache-affinity request fields. Set
`cacheRetention: "long"` only when you want provider-supported extended
retention; the current transport emits `prompt_cache_retention: "24h"` only for
direct `api.openai.com` OpenAI-compatible requests and otherwise falls back to
the stable key.

OpenAI-compatible providers may also set `authProvider` to `codex` or
`github-copilot`. That opt-in keeps the OpenAI-compatible transport while letting
the runtime resolve credentials from the secure credential store before falling
back to `apiKeyEnv` and inline `apiKey`. Stored credentials live outside
`harness.json{,c}` at `<home>/credentials/{authProvider}.json`, where `<home>`
is `HARNESS_HOME` or `~/.harness`. They are atomically replaced and use restrictive
file permissions: POSIX `0600`.

## Public subagents

`subagents` configures the public `spawn_subagent` feature independently of the
generic `agent` profiles. `subagents.enabled` defaults to true, including when
the table only tunes limits. CLI enablement overrides `HARNESS_SUBAGENTS`
(`true`/`1` or `false`/`0`), which overrides local configuration. Remote settings
cannot disable this feature.

Depth, ordinary child concurrency, sampling concurrency and queue policy resolve
environment, config, remote, then defaults. The defaults are depth 2, concurrency
32, sampling equal to resolved concurrency (capped at 512), and `queue`.
Use `HARNESS_SUBAGENTS_MAX_DEPTH`, `HARNESS_MAX_CONCURRENT_SUBAGENTS`,
`HARNESS_SUBAGENT_SAMPLING_LIMIT`, and `HARNESS_SUBAGENT_LIMIT_BEHAVIOR` to override them.
Invalid environment integers fall through, nonpositive environment counts fall
through, configured counts clamp to at least 1, and depth clamps to 1..u32::MAX.
Queue policy accepts case-insensitive `queue` or `fail`; invalid tiers fall through.

```jsonc
{
  "subagents": {
    "max_depth": 2,
    "max_concurrent": 16,
    "sampling_limit": 4,
    "models": { "scout": "local:fast" },
    "toggle": { "security-reviewer": false },
    "roles": {
      "scout": { "default_capability_mode": "read-only", "reasoning_effort": "low" }
    },
    "personas": {
      "reviewer": { "instructions": "Review the assigned changes.", "model": "local:review" }
    }
  },
  "features": {
    "active_agent_messages": false,
    "subagent_model_inheritance": false,
    "subagent_worktree_snapshot": false
  }
}
```

The three features default off. Managed requirements override environment,
effective layered config, remote settings, then defaults. Their environment
names are `HARNESS_ACTIVE_AGENT_MESSAGES`, `HARNESS_SUBAGENT_MODEL_INHERITANCE`, and
`HARNESS_SUBAGENT_WORKTREE_SNAPSHOT`. Model inheritance hides public model selection
only for a complete, nonempty picker catalog whose explicit families are all
exactly `xai` after whitespace trimming. Empty, provisional, unknown, third-party
and mixed catalogs keep selection available. This policy is latched by the
constructing actor; later config changes do not reclassify a running parent.

Definitions are Markdown files with YAML frontmatter. Discovery checks
`<project>/.harness/agents/*.md` from the current parent directory to the nearest
Git root, then `<home>/agents/*.md`, then built-ins. The nearest project
definition wins; project and user definitions override built-ins with the same
name. Outside a Git repository, only the current directory is searched.
Other assistant layouts, plugin directories, CLI definitions and separate
`bundled/` roots are not discovery inputs.

Inline roles/personas override trusted project `.harness/roles/*.toml` and
`.harness/personas/*.toml`, then `<home>/roles/*.toml` and
`<home>/personas/*.toml`. Project presets follow the same nearest-ancestor
search. Untrusted project role/persona files are skipped. A role
alone never creates a callable type. Relative prompt paths use the preset
file's source directory, or the current parent directory for inline presets.
Persona errors abort resolution; a missing role prompt emits a warning and
continues. Type-specific roles win over persona-named roles.

Per-call model, effort and variant overrides win over role then persona defaults.
Valid model overrides precede per-type model pins, definition model and current
parent model; unknown internal pins warn and fall through. Fresh public models
require a catalog validator; resume ignores that argument and the actor pins the
source model. The optional frontmatter `variant` selects a model variant such as
`low`, `high` or `max`. Variant precedence is per-call, role, persona, definition,
then parent; a variant embedded in a model reference belongs to that reference
source. Unknown variants are ignored with a runtime warning. Capability modes
intersect runtime, role and definition ceilings.
Definition worktree isolation promotes resolved `none`, including explicit
`none`. Definition `maxTurns` overrides the parent maximum. Definitions control
MCP inheritance (`all`, `none`, `{"named":[...]}`, `{"except":[...]}`), skill
inheritance and explicit skill preloads. The bundled agents are:

| Agent | Default behavior |
| --- | --- |
| `task` | General worker with inherited tools and MCP access. |
| `scout` | Read/search/web research, medium effort, no edits, shell, eval, MCP, or spawning. |
| `reviewer` | Code review with read/search/LSP, web search, and read-only shell instructions. No eval. May spawn only `scout`. |
| `security-reviewer` | Local security review with read/search/LSP, no shell, eval, network, MCP, or spawning. |
| `clerk` | Mechanical edits or data collection with task tools, medium effort. |

`task` inherits the parent model. `scout` and `clerk` use `@smol`; `reviewer` and
`security-reviewer` use `@slow`. Unset roles inherit the parent model.
`subagents.models` pins override definition defaults. Every child loses ask-user,
feedback and workflow tools; parent operator allow/deny restrictions still apply.

For runtime integration, `HarnessConfig.subagents.resolve_with_lookup` returns
`SubagentRuntimeConfig` using explicit CLI, feature, remote, managed requirement
and environment lookup inputs. `discover_subagent_definitions` captures read-only
definitions, presets and prompt-file outcomes under explicit cwd, trust and
user-root inputs. `resolve_subagent_definition` is pure: its
`SubagentDefinitionContext` supplies the current parent model/effort/maxTurns,
allowlist, catalog, latched selection, tool inventory, operator restrictions,
permission ceiling, injected child depth, MCP and skill snapshots. It returns
the resolved definition, prompts, model, tools and inheritance metadata.
The coordinator owns authorization, catalog readiness, source-model resume
pinning, worktree creation, scheduling and dispatch. Optional type schemas list
at most 64 names (128 bytes each), with normalized 200-byte descriptions; this
presentation bound does not reject otherwise valid types.

### Choosing subagent models

Every subagent runs on the parent model until you configure something else.
Harness never assigns per-agent models by itself, and none of the settings below
are filled in for you. Edit `harness.jsonc` (user or project) by hand, or ask the
agent to make the change.

The most direct setup is one role per agent. A role named after an agent type
applies every time that type is spawned:

```jsonc
{
  "subagents": {
    "roles": {
      "task": { "model": "anthropic/claude-sonnet-5-5", "variant": "medium", "reasoning_effort": "medium" },
      "scout": { "model": "mistral/zai-glm-5-3", "variant": "low", "reasoning_effort": "low" },
      "reviewer": { "model": "openai-codex/gpt-6.1-sol", "variant": "high", "reasoning_effort": "high" }
    }
  }
}
```

Set `reasoning_effort` next to `variant`. Without it, the agent's built-in effort
(medium for `scout` and `clerk`) or the parent's effort is sent instead and can
override the variant's effort. Agents you leave out keep their defaults.
`harness models` lists valid `provider/model` references and their variants;
`harness config validate` checks the file.

Two shorter forms set only the model: `subagents.models.<agent>` pins one agent,
and [`model_roles`](#model-roles) fills the shared `@smol` and `@slow` slots used
by `scout`, `clerk` and the reviewers.

## Model roles

`model_roles` (alias `modelRoles`) defines optional `smol` and `slow` model
references in `provider/model[/variant]` form:

```jsonc
{
  "model_roles": {
    "smol": "openai/gpt-5.4-mini",
    "slow": "openai-codex/gpt-6-astra/high"
  },
  "subagents": { "models": { "reviewer": "@slow" } }
}
```

Use `@smol` or `@slow` in an agent definition's `model`, a `subagents.models`
value, or a per-call model argument. An unset role resolves to the parent model.
Other `@names` are validation errors. Role values must be concrete model
references, not other role selectors. These roles select subagent models; they
do not select a separate title-generation model. A variant in a role reference
does not set the subagent's effort; use `subagents.roles` for that (see
[choosing subagent models](#choosing-subagent-models)).

## Model prompts

Each model has its own complete system prompt template, tuned for that model and
written for Harness's tools and runtime. Specific model
versions take precedence over family defaults; catalog `metadata.family`
supports aliases. `harness models --json` includes `resolution.prompt_preset`.
The selected eval routing guidance reaches the model once: in the system prompt,
or in the eval tool description when a template or a literal prompt leaves it out.

Edit `.harness/prompts/models/glm-5.3.md` or
`.harness/prompts/models/gpt-6.1-sol.md` to customize those models without
rebuilding. Project files override `<home>/prompts`, followed by bundled
defaults. The shared
`partials/*.md`, `subagent.md`, and `eval/*.md` files use the same precedence.
Templates reload on turns, tool iterations and model changes. Invalid selected
files fail instead of silently falling back.

Native children use their actual model with the root agent's prompt locations,
then add their role and completion rules. A nonempty
`agent.<name>.system_prompt` remains a literal system-body override. Project
instructions and command rules remain appended. See the
[editable prompt guide](../../crates/harness-core/prompts/README.md) for filenames,
inheritance, limits and model-specific behavior.

The larger provider catalog lives in `configs/provider-catalog.reference.jsonc`.
That file is a reference and validation fixture for provider and model metadata,
including variants and larger model lists. It is not auto-loaded by config
discovery. Validate it explicitly when you want to check the catalog:

```bash
cargo run -p harness -- --config configs/provider-catalog.reference.jsonc config validate
```

You can also update the checked-in generated provider catalog from a saved file or the public models.dev dataset:

```bash
cargo run -p harness -- models generate
```

`models generate` updates the bundled catalog. By default it fetches `https://models.dev/api.json`, filters to
non-deprecated tool-call-capable models, and writes
`configs/provider-catalog.generated.json`. The harness binary embeds that file
with `include_str!`, so `models generated` can print the static registry without
network access. Use
`--input <file>` or `--stdin` for deterministic runs from a saved API response,
`--provider <id>` to restrict output, `--include-non-tool` /
`--include-deprecated` to broaden the catalog. `models generate` always emits
low/medium/high reasoning presets for models that advertise reasoning support;
`models probe` uses `--emit-reasoning-variants` when you want the same presets in
scratch output to stdout or `--output`. Committed updates should go through
`models generate`.
Review generated provider `baseURL` values before merging; models.dev describes
many providers, while Harness implements OpenAI-compatible and Anthropic
transports.

### First-run provider authentication

No config is needed to connect a provider. Run `harness` and use `/login`,
run `harness auth login <provider>`, or export an API key such as
`OPENAI_API_KEY` or `ANTHROPIC_API_KEY`. The login picker includes OpenAI ChatGPT
Plus/Pro or an API key, GitHub Copilot device login, Anthropic API key, Claude
Pro/Max subscription, Google, OpenCode Zen/Go, OpenRouter, and more.

When the loaded config has no provider entries, Harness discovers stored
credentials and provider API key environment variables against the embedded
models.dev catalog. This also applies to a config that only sets permissions or
other defaults. With only `ANTHROPIC_API_KEY`, Harness selects a Claude Sonnet.

Defining any `provider` entry makes the catalog curated: only configured
providers plus signed-in Codex, GitHub Copilot, and Claude subscription are
included. To use a custom OpenAI-compatible endpoint, define it explicitly:

```jsonc
{
  "provider": {
    "default": {
      "type": "openai_compatible",
      "options": {
        "baseURL": "https://api.openai.com/v1",
        "apiKeyEnv": ["OPENAI_API_KEY"],
        "cacheRetention": "short"
      }
    }
  }
}
```

For the V1 built-in OAuth-backed providers, add `authProvider` and leave
credentials out of config:

```jsonc
{
  "provider": {
    "openai-codex": {
      "type": "openai_compatible",
      "options": {
        "authProvider": "codex",
        "baseURL": "https://api.openai.com/v1",
        "apiKeyEnv": ["OPENAI_API_KEY"],
        "cacheRetention": "short"
      }
    },
    "github-copilot": {
      "type": "openai_compatible",
      "options": {
        "authProvider": "github-copilot",
        "baseURL": "https://api.githubcopilot.com",
        "cacheRetention": "short"
      }
    }
  }
}
```

Credential resolution order is stored OAuth, stored API key, `apiKeyEnv`, then
inline `apiKey`. Logout/auth-management commands remove only stored credential
files, so existing `apiKeyEnv` or inline fallbacks remain valid. Support exports
include a redaction manifest entry for credential-store files, never credential
file contents.

Use `harness auth list --json` to inspect credential status without exposing
secrets. `harness auth login` opens the standalone provider picker.
`harness auth login <provider>` selects a provider directly; use `--method`
for its supported method, such as `device`, `browser`, or `api-key`. Codex
supports browser, device, and API-key login; `--api-key-stdin` reads the key from
stdin. Browser login from SSH can also finish by pasting the final localhost
callback URL when the browser cannot reach the remote loopback listener.
GitHub Copilot uses device login and supports GitHub Enterprise with
`--enterprise-url`.

`harness auth logout <provider>` removes only the stored credential, not
config or environment fallbacks. In the TUI, `/login` and the `Login` palette
row open the picker. On first run in live mode, with no provider and no saved
project sessions, it opens automatically. Esc returns to the welcome screen
with `No provider connected. Use /login.`; returning users see only that banner.
The disconnected footer reads `Provider not connected`. `/auth` and `/connect`
are removed.

Codex OAuth follows the ChatGPT PKCE/device-code reference flow and decorates the
existing OpenAI-compatible transport with the Codex endpoint, bearer token, and
account/session headers. GitHub Copilot OAuth follows the reference implementation Copilot
device-code reference: the GitHub device `access_token` is stored as the active
OAuth credential and sent directly as the Copilot bearer; no separate
GitHub-to-Copilot token exchange is performed in the deterministic V1 path.
Copilot Enterprise credentials store the normalized enterprise domain so request
decoration can select `https://copilot-api.<domain>` while public Copilot uses
`https://api.githubcopilot.com`.

`harness doctor` checks readiness offline and redacts secret values. It checks
stored credentials, configured `apiKeyEnv` fallbacks, and automatically
discovered catalog credentials. It cannot prove live authentication or endpoint
health. When no provider is connected, the provider failure message is:

> No provider connected. Run `harness auth login <provider>`, use /login in the TUI, or set a provider API key such as OPENAI_API_KEY or ANTHROPIC_API_KEY.

Use a live prompt when you need to check account and transport access.

## Public contract summary

| Area | Canonical shape | Notes |
| --- | --- | --- |
| Runtime config file | `harness.json` / `harness.jsonc` | Personal defaults live in `<home>`, normally `~/.harness`. |
| TUI config file | `tui.json` / `tui.jsonc` | Runtime and TUI settings are intentionally split. |
| Core runtime keys | `provider`, `model`, `model_roles`, `agent`, `permission`, `mcp`, `skills`, `instructions`, plus Harness runtime extensions | `agent` configures the `default` parent and custom profiles; `subagents` configures native child definitions. |
| TUI settings | `keybinds`, `confirm_before_rewind` | Unsupported TUI-only fields fail validation. |
| Permission naming | `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`, plus safety kinds `read`, `external_directory`, and `doom_loop` | Legacy `shell` / `network` remain compatibility-only. `external_directory` and `doom_loop` default to ask; `read` defaults to allow with `.env` pattern asks. |
| Prompt assets | `.harness/prompts/models/*.md`, shared templates and existing agent definitions | `AGENTS.md` is auto-discovered separately as project context. |

Runtime and TUI config stay separate. Runtime config controls providers,
models, the generic agent, permissions, MCP, skills, instructions, and compaction. TUI
config accepts `$schema`, `keybinds`, and `confirm_before_rewind`; use `tui.json` or `tui.jsonc`
for those settings instead of mixing them into runtime config.

## Runtime top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `agent` | Generic `default` parent and named subagent tuning. Alternate primary roles and category routes are rejected. |
| `disabled_providers` | Upstream-compatible provider filter; hides matching configured and authenticated built-in providers from runtime model catalogs. |
| `enabled_providers` | Upstream-compatible provider allow-list; when non-empty, only matching configured/authenticated built-in providers remain in runtime model catalogs. |
| `formatter` | Boolean enablement or an object with `enabled` (default `true`), `experimental_oxfmt` (default `false`) and named formatter overrides. Overrides accept `disabled`, `command`, `environment` and `extensions`. |
| `instructions` | Inline instructions or file paths appended after the system prompt. |
| `lsp` | `false` disables LSP. An object accepts `disabled` and named `servers`. |
| `mcp` | MCP server definitions keyed by server name. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback metadata; runtime profile resolution selects the primary target in V1. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. Supports scalar `allow`/`ask`/`deny` or per-tool pattern maps. Catch-all deny hides tools from the model; last matching pattern wins. |
| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime settings including [session storage](#session-storage), startup approval mode, provider-context compaction settings, provider retry policy, and [behavior guidance](#runtime-behavior-guidance). |
| `model_roles` / `modelRoles` | Optional `smol` and `slow` model references for subagent role selectors. |
| `skills` | Shared skill discovery roots and permission overrides for skill loading. |
| `edit` | Model-facing edit mode and per-model overrides. |
| `eval` | Eval languages, routed tools, deadlines, output limits, memory and sandbox settings. |
| `features` | Subagent messaging, model inheritance and worktree snapshot switches. |
| `subagents` | Child enablement, limits, model pins, type toggles, roles and personas. |
| `hooks` | Lifecycle command hooks. |
| `integrations` | Remote search settings and the alternative `mcp.servers` shape. |
| `logging` | Log level and optional log file. |
| `ui` | Runtime presentation defaults and transcript caps; use TUI config for keybindings. |

### Nested key reference

These tables give file-contract defaults before catalog data, bundled profiles or
runtime resolution supply values. `null` means unset. Aliases on one row are
alternative spellings, not separate settings. Do not put two spellings in one
object. Empty object defaults mean that the nested fields use their own defaults.

### Provider fields

The `type` field is required. HTTP provider `options` accepts only the transport fields listed below, not arbitrary request options. Put request options such as `thinking` under a model or variant. Nested non-null scalar options override sibling provider fields; a nonempty nested `apiKeyEnv` replaces the sibling list, and nested headers override matching sibling headers. Provider maps default to `{}`.

`provider.<id>` with `openai_compatible`

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `apiKey` / `api_key` | string | `""` |
| `apiKeyEnv` / `apiKeyEnvironment` / `api_key_env` | array | `[]` |
| `apiMode` / `api_mode` | `"responses"`, `"chat_completions"`, `"auto"` | `"auto"` |
| `authProvider` / `auth_provider` | string or null | `null` |
| `baseURL` / `baseUrl` / `base_url` | string | `"https://api.openai.com/v1"` |
| `cacheRetention` / `cache_retention` | `"none"`, `"short"`, `"long"` | `"short"` |
| `headers` | object | `{}` |
| `models` | object | `{}` |
| `name` | string or null | `null` |
| `options` | object | `{"apiKey":null,"apiKeyEnv":[],"apiMode":null,"authProvider":null,"baseURL":null,"cacheRetention":null,"headers":{},"name":null,"timeoutMs":null}` |
| `timeoutMs` / `timeout_ms` | integer | `60000` |
| `type` | `"openai_compatible"` | required |

`provider.<id>` with `anthropic_messages`

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `apiKey` / `api_key` | string | `""` |
| `apiKeyEnv` / `apiKeyEnvironment` / `api_key_env` | array | `[]` |
| `baseURL` / `baseUrl` / `base_url` | string | `"https://api.anthropic.com/v1"` |
| `headers` | object | `{}` |
| `models` | object | `{}` |
| `name` | string or null | `null` |
| `options` | object | `{"apiKey":null,"apiKeyEnv":[],"baseURL":null,"headers":{},"name":null,"timeoutMs":null}` |
| `timeoutMs` / `timeout_ms` | integer | `60000` |
| `type` | `"anthropic_messages"` | required |

`provider.<id>` with `anthropic_subscription`

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `appendSystemPrompt` | boolean or null | `null` |
| `enabled` | boolean or null | `null` |
| `models` | object | `{}` |
| `name` | string or null | `null` |
| `pinnedAccount` | string or null | `null` |
| `resumeMode` | `"auto"`, `"off"` or null | `null` |
| `settingSources` | array or null | `null` |
| `strictMcpConfig` | boolean or null | `null` |
| `systemPromptFile` | string or null | `null` |
| `systemPromptMode` | `"preset-append"`, `"full"`, `"override"` or null | `null` |
| `tokenInjection` | `"oauth-slots"`, `"config-dir"`, `"ambient"` or null | `null` |
| `type` | `"anthropic_subscription"` | required |

### Model fields

These fields live under `provider.<id>.models.<model>`. Model options are arbitrary JSON. Variant limits and options override the base model. Unset limits are unknown, not zero-capacity limits. `limit.context`, `limit.input` and `limit.output` take precedence over the older context and token-limit fields. Cost requires nonnegative `input` and `output` prices in USD per million tokens; omitted cache prices use the input rate.

Base model

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `cost` | object or null | `null` |
| `displayName` / `display_name` / `name` | string | `""` |
| `limit` | object | `{"context":null,"input":null,"output":null}` |
| `maxInputTokens` / `max_input_tokens` | integer or null | `null` |
| `maxOutputTokens` / `max_output_tokens` | integer or null | `null` |
| `metadata` | object | `{"context_window_tokens":null,"family":null,"release_stage":null,"supports_reasoning_summaries":null,"supports_tool_calls":null}` |
| `modalities` | object | `{"input":[],"output":[]}` |
| `options` | object | `{}` |
| `variants` | object | `{}` |

Model limits (`limit`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `context` | integer or null | `null` |
| `input` | integer or null | `null` |
| `output` | integer or null | `null` |

Model modalities (`modalities`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `input` | array | `[]` |
| `output` | array | `[]` |

Model metadata (`metadata`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `contextWindowTokens` / `context_window_tokens` | integer or null | `null` |
| `family` | string or null | `null` |
| `releaseStage` / `release_stage` | `"stable"`, `"preview"`, `"deprecated"` or null | `null` |
| `supportsReasoningSummaries` / `supports_reasoning_summaries` | boolean or null | `null` |
| `supportsToolCalls` / `supports_tool_calls` | boolean or null | `null` |

Model price (`cost`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `cacheRead` / `cache_read` | number or null | see text |
| `cacheWrite` / `cache_write` | number or null | see text |
| `input` | number | required |
| `output` | number | required |

Named variant (`variants.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `contextWindowTokens` / `context_window_tokens` | integer or null | `null` |
| `disabled` | boolean | `false` |
| `displayName` / `display_name` / `name` | string or null | `null` |
| `limit` | object | `{"context":null,"input":null,"output":null}` |
| `maxInputTokens` / `max_input_tokens` | integer or null | `null` |
| `maxOutputTokens` / `max_output_tokens` | integer or null | `null` |
| `metadata` | object | `{"description":null,"reasoning_effort":null,"recommended_for":null,"text_verbosity":null}` |
| `modalities` | object | `{"input":[],"output":[]}` |
| `options` | object | `{}` |

Variant metadata (`variants.<name>.metadata`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `description` | string or null | `null` |
| `reasoningEffort` / `reasoning_effort` | `"none"`, `"minimal"`, `"low"`, `"medium"`, `"high"`, `"max"`, `"xhigh"` or null | `null` |
| `recommendedFor` / `recommended_for` | string or null | `null` |
| `textVerbosity` / `text_verbosity` | `"low"`, `"medium"`, `"high"` or null | `null` |

Named selector (`model_profile.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `fallback` | array | `[]` |
| `model` | string | `""` |
| `variant` | string or null | `null` |

Selector fallback entry (`model_profile.<name>.fallback[]`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `model` | string | `""` |
| `variant` | string or null | `null` |

### Agent profile fields

Fields under `agent.<name>` start with the values below. The shipped `default` profile is then merged with your overrides. `model_ref_explicit` is loader bookkeeping: normalization derives it from whether a model was supplied. Do not set it to select a model. An empty `tools` list uses the runtime toolset; an explicit list restricts it.

Profile

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `color` | string or null | `null` |
| `description` | string | `""` |
| `hidden` | boolean | `false` |
| `maxIters` / `maxSteps` / `max_iters` / `steps` | integer or null | `null` |
| `mode` | `"primary"`, `"subagent"`, `"all"` | `"all"` |
| `model` / `modelRef` / `model_ref` | string | `""` |
| `model_ref_explicit` | boolean | `false` |
| `name` | string or null | `null` |
| `options` | object | `{}` |
| `permission` / `permissions` | `"allow"`, `"ask"`, `"deny"` or JSON value or object | see text |
| `prompt` / `systemPrompt` / `system_prompt` | string or null | `null` |
| `temperature` | number or null | `null` |
| `toolFailureMode` / `tool_failure_mode` | `"fail_turn"`, `"continue_as_tool_message"` | `"continue_as_tool_message"` |
| `tools` | array or object | see text |
| `topP` / `top_p` | number or null | `null` |
| `variant` | string or null | `null` |

### Subagent preset fields

`subagents.models`, `toggle`, `roles` and `personas` default to empty maps. Optional limits default to `null` in the file contract; their resolved defaults and environment precedence are described under public subagents. Roles use optional strings for model, effort, variant, prompt file, capability mode and isolation. Personas use optional strings and lists of input/output descriptors.

Subagent configuration

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `enabled` | boolean | `true` |
| `limit_behavior` | string or null | `null` |
| `max_concurrent` | integer or null | `null` |
| `max_depth` | integer or null | `null` |
| `models` | object | `{}` |
| `personas` | object | `{}` |
| `roles` | object | `{}` |
| `sampling_limit` | integer or null | `null` |
| `toggle` | object | `{}` |

Feature switches (`features`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `active_agent_messages` | boolean or null | `null` |
| `subagent_model_inheritance` | boolean or null | `null` |
| `subagent_worktree_snapshot` | boolean or null | `null` |

Model roles (`model_roles`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `slow` | string or null | `null` |
| `smol` | string or null | `null` |

Role (`subagents.roles.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `default_capability_mode` | string or null | `null` |
| `default_isolation` | string or null | `null` |
| `description` | string | `""` |
| `model` | string or null | `null` |
| `prompt_file` | string or null | `null` |
| `reasoning_effort` | string or null | `null` |
| `variant` | string or null | `null` |

Persona (`subagents.personas.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `default_isolation` | string or null | `null` |
| `description` | string or null | `null` |
| `inputs` | array | `[]` |
| `instructions` | string or null | `null` |
| `instructions_file` | string or null | `null` |
| `model` | string or null | `null` |
| `outputs` | array | `[]` |
| `reasoning_effort` | string or null | `null` |
| `variant` | string or null | `null` |

Persona input or output entry

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `description` | string | required |
| `io_type` | string | `"file"` |
| `name` | string | required |
| `required` | boolean | `false` |

### Other runtime settings

Concurrency counts and approval timeouts must be positive. `runtime.prompt.wait_timeout_ms: null` waits until the turn ends. Deterministic settings default off. Compaction, behavior and provider retries have their own sections below.

Background tasks (`runtime.background_tasks`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `defaultConcurrency` / `default_concurrency` | integer | `4` |
| `messageStalenessTimeoutMs` / `message_staleness_timeout_ms` | integer | `10000` |
| `modelConcurrency` / `model_concurrency` | integer | `2` |
| `providerConcurrency` / `provider_concurrency` | integer | `4` |
| `staleTimeoutMs` / `stale_timeout_ms` | integer | `30000` |

Approval timeout (`runtime.permissions`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `askTimeoutMs` / `ask_timeout_ms` | integer | `30000` |

Prompt timeout (`runtime.prompt`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `waitTimeoutMs` / `wait_timeout_ms` | integer or null | `null` |

Deterministic mode (`runtime.deterministic`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `enabled` | boolean | `false` |
| `seed` | integer | `0` |

Presentation (`ui`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `defaultProfile` / `default_profile` | string or null | `null` |
| `keybindings` | object | `{}` |
| `maxEventsInMemory` / `max_events_in_memory` | integer | `25000` |
| `maxTranscriptCharsInMemory` / `max_transcript_chars_in_memory` | integer | `200000` |

Logging (`logging`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `file` | string or null | `null` |
| `level` | string | `"info"` |

### Eval settings

All four eval deadlines are seconds in 1..86400. Detached-cell capacity and pool width are 1..256. Languages must be a nonempty unique list of `js`, `py`, `rb` or `jl`. Routed IDs must be unique, contain no whitespace, and contain at most 256 entries of 1..256 bytes each; `eval` and `question` stay direct. Output limits allow zero, with maxima of 51200 bytes and 16384 columns. Memory values use MiB and allow zero, with a maximum of 1048576; nonzero thresholds must satisfy watermark <= notice <= ceiling. Sandbox limits are 1..4096 MiB and 1..86400 seconds.

Eval (`eval`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `cell_timeout_seconds` | integer | `30` |
| `foreground_window_seconds` | integer | `60` |
| `hard_limit_seconds` | integer | `1800` |
| `languages` | array | `["js","py"]` |
| `max_detached_cells` | integer | `15` |
| `memory` | object | `{"ceiling_mb":2048,"gc_watermark_mb":256,"notice_mb":1024,"retained_images_mb":256,"retained_results_mb":32}` |
| `output_head_bytes` | integer | `20480` |
| `output_max_columns` | integer | `768` |
| `parallel_pool_width` | integer | `4` |
| `route_tools` | array | `[]` |
| `run_budget_seconds` | integer | `300` |
| `sandbox` | object | `{"enabled":false,"memory_limit_mb":64,"timeout_seconds":300}` |
| `status_events` | boolean | `true` |

Memory (`eval.memory`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `ceiling_mb` | integer | `2048` |
| `gc_watermark_mb` | integer | `256` |
| `notice_mb` | integer | `1024` |
| `retained_images_mb` | integer | `256` |
| `retained_results_mb` | integer | `32` |

Sandbox (`eval.sandbox`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `enabled` | boolean | `false` |
| `memory_limit_mb` | integer | `64` |
| `timeout_seconds` | integer | `300` |

### Integration and extension settings

`mcp` is a server-name map, equivalent to `integrations.mcp.servers`. Do not set both shapes in one config. Each server requires `transport` and its command or endpoint. Hook entries require `event` and `command`; timeout must be 1..300000 ms and cwd must be relative without parent traversal. See [hooks](../operations/hooks.md) for execution policy.

Remote search (`integrations.remote_search`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `authToken` / `auth_token` | string or null | `null` |
| `endpoint` | string | `"https://mcp.exa.ai/mcp"` |
| `maxRetries` / `max_retries` | integer | `1` |
| `requireAuth` / `require_auth` | boolean | `false` |
| `retryBackoffMs` / `retry_backoff_ms` | integer | `250` |
| `timeoutSecs` / `timeout_secs` | integer | `30` |

MCP server with `stdio` transport

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `command` | array | required |
| `cwd` | string or null | `null` |
| `enabled` | boolean | `true` |
| `env` / `environment` | object | `{}` |
| `timeout` / `timeoutSecs` / `timeout_secs` | integer | `30` |
| `transport` | `"stdio"` | required |

MCP server with `http` transport

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `enabled` | boolean | `true` |
| `endpoint` / `url` | string | see text |
| `headers` | object | `{}` |
| `timeout` / `timeoutSecs` / `timeout_secs` | integer | `30` |
| `transport` | `"http"`, `"streamable_http"` | required |

Hooks (`hooks`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `lifecycle` | array | `[]` |

Lifecycle hook (`hooks.lifecycle[]`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `command` | array | required |
| `critical` | boolean | `false` |
| `cwd` | string or null | `null` |
| `env` | object | `{}` |
| `event` | `"run_started"`, `"run_finished"`, `"run_failed"`, `"agent_turn_started"`, `"agent_turn_finished"`, `"tool_call_started"`, `"tool_call_finished"`, `"provider_request_started"`, `"provider_request_finished"`, `"compaction_requested"`, `"compaction_written"`, `"compaction_applied"`, `"compaction_failed"`, `"subagent_spawned"`, `"subagent_finished"`, `"permission_requested"`, `"permission_resolved"` | required |
| `id` / `name` | string or null | `null` |
| `timeoutMs` / `timeout_ms` | integer | `5000` |

LSP (`lsp`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `disabled` | boolean | `false` |
| `servers` | object | `{}` |

LSP server (`lsp.servers.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `command` | array or null | `null` |
| `disabled` | boolean | `false` |
| `env` | object | `{}` |
| `extensions` | array or null | `null` |
| `initialization` | JSON value | `null` |

Formatter override (`formatter.<name>`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `command` | array or null | `null` |
| `disabled` | boolean | `false` |
| `environment` | object or null | `null` |
| `extensions` | array or null | `null` |

Formatter registry (`formatter`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `enabled` | boolean | `true` |
| `experimentalOxfmt` / `experimental_oxfmt` | boolean | `false` |
| `languages` | object | see text |

`formatter.languages` is a compatibility map from an extension to a formatter
override. Normalization creates a named override with that one extension.
Named overrides default to enabled, with command, environment and extensions
unset so built-in settings can apply.

Skill discovery (`skills`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `disabled` / `disabledIds` | array | `[]` |
| `globalRoots` / `global_roots` | array | `["~/.harness/skills","~/.agents/skills"]` |
| `paths` / `projectRoots` / `project_roots` | array | `[".harness/skills",".agents/skills"]` |
| `permissions` | object | `{}` |
| `urls` | array | `[]` |
| `walkToGitRoot` / `walk_to_git_root` | boolean | `true` |

Shell allowlist (`permission.shell_allowlist`)

| Key (aliases on the same row) | Type | Default when omitted |
| --- | --- | --- |
| `cwdRoots` / `cwd_roots` | array | `[]` |
| `executables` | array | `[]` |
| `mode` / `policyMode` / `policy_mode` | `"permission_patterns"`, `"legacy_executables"` | `"permission_patterns"` |

For `anthropic_subscription`, all optional fields default to unset in config.
`systemPromptMode` accepts `preset-append`, `full` or `override`; the resolved
default is `full`. An explicit mode overrides `appendSystemPrompt`; otherwise
`true` selects `full` and `false` selects `preset-append`. `resumeMode` accepts
`auto` or `off`; `settingSources` accepts `user`, `project` and `local`;
`tokenInjection` accepts `oauth-slots`, `config-dir` and `ambient`. Snake_case
aliases for the camelCase subscription fields are accepted by the loader.
Environment overrides use `HARNESS_CLAUDE_SDK_OAUTH_` with suffixes `ENABLED`,
`SYSTEM_PROMPT_MODE`, `SYSTEM_PROMPT_FILE`, `RESUME`, `TOKEN_INJECTION`,
`SETTING_SOURCES` and `PINNED_ACCOUNT`. Valid environment values win over config.
## Session storage

`runtime.session_dir` defaults to an empty string, which means automatic managed
storage at `<data-dir>/sessions/<project-key>`. Leave it empty or omit it to use
that default. `--session-dir DIR` takes precedence over a configured directory.
A nonempty relative override resolves against the selected project directory
(`--cwd`), preserving existing semantics; it is not relative to the config file.
An absolute override is used as supplied.

```jsonc
{
  "runtime": {
    "session_dir": ""
  }
}
```

`<data-dir>` is the Harness home: a nonempty `HARNESS_HOME` used as-is, otherwise
`$HOME/.harness`. The project key is `v2-` followed by the full BLAKE3 hex digest
of the canonical absolute UTF-8 project path. Its fixed length supports long
paths, and symlink aliases share the same storage.

Runtime data uses `<data-dir>/projects/<project-key>` and managed worktrees use
`<data-dir>/worktrees/<project-key>`, independently of the session override.
Authored project agents, skills, prompts and configuration stay in their existing
locations, as do `.harness/permission-grants.json` workspace grants. Data
directory selection does not change the [config discovery order](#discovery-and-precedence).

Existing project-local histories and legacy `--path--` directories are preserved
but not migrated automatically. Use
`--session-dir <old-session-dir>` to access them explicitly.
See [saved sessions](../operations/sessions.md) and the
[storage migration instructions](../architecture/sessions-and-replay.md#legacy-storage).

## Variable substitution

The harness resolves variable references in string values after parsing JSON and before typed deserialization. The resolver makes one pass through all values. It does not expand nested
references such as `${VAR:-${OTHER}}` recursively.

| Syntax | Behavior |
| --- | --- |
| `{env:VAR}` | Environment variable substitution. Returns an empty string if `VAR` is missing from the environment. |
| `{file:path}` | File content substitution with trailing whitespace removed. The path is resolved relative to the config file directory; absolute paths are used as-is. |
| `${VAR}` | Shell-style environment variable. If `VAR` is missing from the environment, this produces a config error rather than expanding to an empty string. Use `${VAR:-}` for an explicit empty fallback. |
| `${VAR:-fallback}` | Environment variable with fallback value. If `VAR` is missing or empty, `fallback` is used. |

`apiKeyEnv` tries environment variables as credential fallbacks and redacts their
values. It is separate from `{env:VAR}` substitution.

## Effective config inspection

Print the merged runtime config after discovery, layer merge, and session-dir
overrides:

```bash
harness config validate
harness config show --effective
harness config sources
harness config explain model
```

### `config show --effective`

These inspection commands work even when no files exist. In that case,
`harness config validate` exits successfully and prints:

```text
config valid: no configuration files; using built-in defaults
```

`--config <missing path>` still errors; an explicit path must exist.

Effective output is a JSON envelope:

- `schema_version`: `harness-config-effective-v1`
- `redacted`: always `true` for this command
- `layers`: discovered config file paths in merge order
- `primary_path`: highest-precedence runtime config path, or `null` when no
  runtime file exists (TUI-only paths remain listed under `layers`)
- `effective`: the merged config value after secret redaction

With no runtime files, `effective` reports built-in defaults. Secret-bearing
fields (for example `apiKey`) are replaced with redaction markers.
`config show` without `--effective` exits with usage status `2`.

### `config sources`

Lists discovered layers in merge order (`harness-config-sources-v1`):

- `order`, `path`, `exists`, `kind` (`runtime` or `tui`), `primary`
- `merge_order` documents that later layers override earlier ones
- `searched` always lists every candidate runtime config path in merge order,
  including paths that do not exist

When no runtime layers are found, `note` is:

> No configuration files found. Harness uses built-in defaults and providers connected through /login, `harness auth login`, or provider API key environment variables.

### `config explain <path>`

Explains one dotted public path (for example `model` or
`provider.default.options.apiKey`) as `harness-config-explain-v1`:

- `found`, `effective` (redacted), `source_path` (last layer that defines the path)
- per-layer `defines_path` / redacted `value` rows for attribution
- `primary_path`: runtime config path, or `null` with no runtime files
- with no files, values come from built-in defaults and `source_path` is `null`
- empty path exits with usage status `2`

### Settings registry

Typed per-setting metadata lives in
`harness_core::config::settings_registry` and complements the public runtime/TUI
contracts (it does not merge them). List the registry without loading a config
file or printing secret values:

```bash
cargo run -p harness -- config settings
```

Output is `harness-settings-registry-v1`:

- `setting_count` plus `settings[]` entries with `setting_id`, `schema_id`,
  `surface` (`runtime`|`tui`), `sensitivity` (`public`|`redacted`|`secret`),
  and `metadata_only`
- No default values or secret material are emitted
- `metadata_only: true` marks product stubs (for example worktree parent path
  defaults) that are not yet public `harness.json` / `tui.json` keys

Library entry points: `settings_registry()`, `setting_definition()`,
`settings_registry_json()`, `is_metadata_only_setting()`.

### YOLO on startup

Start an interactive session in YOLO mode with `--yolo`:

```bash
harness --yolo
cargo run -p harness -- --yolo
harness tui --yolo
```

The TUI keeps `YOLO` visible in the composer while enabled.
The mode is saved in that session's journal and restored when
you resume it. Starting a different session without the flag uses its configured
default. The flag does not edit your project or global configuration.

Set `runtime.yolo` in your runtime config to start new and resumed runs
with ordinary tool permissions automatically approved. It defaults to `false`.

```json
{
  "runtime": {
    "yolo": true
  }
}
```

In the TUI, open `/settings` and select YOLO on startup on the
Runtime tab. Enter toggles the saved preference in the bound runtime config;
reset restores `false`. Restart the harness to apply the saved preference.

Use Ctrl+O, `/yolo`, or YOLO mode in the
command palette to toggle the current session. `/toggles` also exposes the active
mode. These session toggles are remembered on resume and do not change the saved
startup preference. Passing `--yolo` again or setting `runtime.yolo`
to `true` enables the mode even if it was previously turned off in that session.
Questions, sensitive requests, and explicit permission denials keep their existing
checks. Replay does not enable or change approval mode.

## Config layering

Later config layers override earlier ones. See the full
[discovery order](#discovery-and-precedence) below.

JSON objects merge key by key before defaults and model validation.
Omitted sections inherit the earlier value. Arrays replace earlier arrays, except
`instructions`, which accumulates in layer order. File substitutions resolve relative to the file that declares them.

JSON `agent` settings tune runtime profiles. Native child definitions use the
separate `subagents` configuration and Markdown discovery described above;
project and user Markdown definitions can replace a built-in with the same name.

## TUI top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `keybinds` | Supported TUI keybinding overrides. |
| `confirm_before_rewind` | Ask before rewinding a conversation (default `true`). The rewind panel's "Yes, and don't ask again" choice saves `false`. |

## TUI default bindings

`keybinds` maps action IDs to one key or one leader sequence. A valid override
replaces all existing bindings for that action. Unknown action IDs and invalid
key strings are ignored by the keymap. Comma-separated lists and a configurable
`leader` field are not supported. The leader is fixed at `Ctrl+x`.

Use `ctrl+`, `shift+` and `alt+` modifiers (or their initial-capital forms),
a character, an arrow name, `tab`, `backtab`, `enter`, `esc`, `space`,
`backspace`, `delete`, `home`, `end`, `pageup`, `pagedown`, `insert`, or
`f1` through `f12`. Leader sequences accept `<leader>`, `ctrl+x ` or `ctrl+x+`
before the second key. For example:

```jsonc
{ "keybinds": { "open_model_switcher": "<leader>m", "toggle_tasks": "alt+g" } }
```

Bindings depend on focus. Prompt arrows edit the composer or recall history;
session arrows navigate children and parents. Permission-modal `Ctrl+y` allows
a request, while composer `Ctrl+y` redoes an edit. In multiline mode, `Enter`
inserts a newline and `Alt+Enter` submits or steers.

| Action | Shipped bindings |
| --- | --- |
| `palette` | Ctrl+p, ? |
| `help` | Ctrl+x, Ctrl+. |
| `open_model_switcher` | Ctrl+x m |
| `open_status_dialog` | Ctrl+x s, F2, Ctrl+, |
| `open_lineage_browser` | Ctrl+x g |
| `open_theme_dialog` | Ctrl+x t |
| `copy_message` | Ctrl+x y |
| `export_session` | Ctrl+x x |
| `quit` | q, Ctrl+q |
| `focus_next` | Tab |
| `focus_prev` | Ctrl+Shift+Tab |
| `variant_cycle` | Shift-Tab |
| `toggle_terminal_panel` | 4 |
| `toggle_prompt_focus` | Space, i |
| `close_review_surface` | 1 |
| `reload` | r |
| `session_background` | Ctrl+b |
| `session_child_first` | Ctrl+x Down |
| `session_child_cycle` | Right |
| `session_child_cycle_reverse` | Left |
| `session_parent` | Up |
| `diff_hunk_next` | Alt+n, Alt+] |
| `diff_hunk_previous` | Alt+p, Alt+[ |
| `move_down` | j |
| `move_up` | k |
| `submit_prompt` | Enter |
| `insert_newline` | Shift+Enter, Ctrl+j, Alt+Enter |
| `interject_prompt` | Alt+i, Ctrl+Alt+Enter |
| `cancel_and_replace_prompt` | Ctrl+Enter, Ctrl+Shift+Enter, Alt+r |
| `toggle_multiline` | Alt+m |
| `clear_prompt` | Esc |
| `history_up` | Up |
| `history_down` | Down |
| `cursor_left` | Left |
| `cursor_right` | Right |
| `backspace` | Backspace |
| `delete` | Delete |
| `select_char_left` | Shift+Left |
| `select_char_right` | Shift+Right |
| `select_word_left` | Ctrl+Shift+Left, Alt+Shift+b |
| `select_word_right` | Ctrl+Shift+Right, Alt+Shift+f |
| `select_line` | Shift+Home, Shift+End |
| `select_all` | Ctrl+a, Ctrl+Shift+a |
| `move_word_left` | Ctrl+Left, Alt+b |
| `move_word_right` | Ctrl+Right, Alt+f |
| `move_line_start` | Home |
| `move_line_end` | End |
| `move_buffer_start` | Ctrl+Home |
| `move_buffer_end` | Ctrl+End |
| `delete_word_forward` | Alt+d, Ctrl+Delete |
| `delete_word_backward` | Ctrl+w, Alt+Backspace, Ctrl+Backspace |
| `delete_line` | Ctrl+Shift+k |
| `kill_to_line_start` | Ctrl+u |
| `kill_to_line_end` | Ctrl+k |
| `undo` | Ctrl+z, Ctrl+- |
| `redo` | Ctrl+y, Ctrl+Shift+z |
| `allow_permission` | Ctrl+y |
| `yolo_mode` | Ctrl+o |
| `deny_permission` | Ctrl+n |
| `dismiss_modal` | Esc, Ctrl+c |
| `toggle_todos` | Ctrl+t |
| `toggle_tasks` | Ctrl+g |
| `half_page_down` | Ctrl+d |
| `first_message` | Ctrl+Home |
| `last_message` | Ctrl+Alt+g, Ctrl+Alt+End |
| `next_message` | Ctrl+Alt+n |
| `previous_message` | Ctrl+Alt+p |
| `prompt_stash` | Ctrl+s, Alt+s |

These actions are also accepted as override IDs but have no shipped binding:
`toggle_follow`, `scroll_up`, `scroll_down`, `open_event_log`,
`revert_workspace`, `toggle_scrollbar`, `open_error_details`, `prompt_stash_pop`,
`prompt_stash_list`, `open_session_history`, `open_settings`, `open_view_plan`,
`open_memory_browser` and `open_worktree_picker`.

Input aliases are `toggle_operator_sidebar` for `open_status_dialog`,
`resume_session` for `open_session_history`, `settings` for `open_settings`,
`view_plan` and `view-plan` for `open_view_plan`, `memory` for
`open_memory_browser`, and `worktree` and `switch_worktree` for
`open_worktree_picker`. Palette command IDs such as `new_session`,
`switch_model` and `compact_session` are not keybinding action IDs.

TUI prompt history is runtime state, not config. Interactive startup and live
sessions load and append prompt history at `<session-dir>/tui/prompt-history.json`.
Submitted prompts survive restarts; unsent drafts remain in the active composer.

## Discovery and precedence

With no files, Harness uses built-in defaults. Workspace `AGENTS.md` instructions
and TUI keybinding layers still load. Runtime config discovery merges optional
layers from lowest to highest precedence:

```mermaid
flowchart LR
    Global[Personal config in Harness home] --> Env[HARNESS_CONFIG]
    Env --> Project[Project layers from git root down]
    Project --> Inline[HARNESS_CONFIG_CONTENT]
    Inline --> Result[Effective configuration]
```

An explicit `--config PATH` replaces runtime file discovery with that one file.
`HARNESS_CONFIG_CONTENT` still applies afterward, and TUI files still use their
own discovery. Relative explicit paths use the selected current directory.

Later layers override earlier values. Objects merge; most arrays replace the
earlier array. `instructions` accumulates. The ordered locations are:

1. `<home>/harness.jsonc` or `<home>/harness.json`, first existing wins
2. `HARNESS_CONFIG` when set to a custom runtime config path
3. Project layers from the git root down to the current directory. At each
   directory, load `harness.jsonc` or `harness.json`, then
   `.harness/harness.jsonc` or `.harness/harness.json`.
4. `HARNESS_CONFIG_CONTENT` as the final runtime overlay

`<home>` is a nonempty `HARNESS_HOME` used as-is, otherwise `$HOME/.harness`.
The `.jsonc` file wins over `.json` at each location; the two are not separate
layers. `harness config sources` lists candidate paths in `searched`, including
absent files, in this merge order.

After `harness auth login` or TUI sign-in stores a credential successfully,
Harness creates `<home>/harness.jsonc` only when `HARNESS_CONFIG` and
`HARNESS_CONFIG_CONTENT` are unset and neither user runtime config file exists.
It uses create-new semantics and POSIX mode `0600`; an existing file is never
overwritten. Successful creation prints `wrote starter config: <path>` on stdout.

The starter includes the schema URL
`https://github.com/urbanbreach/agent-harness/releases/latest/download/harness.schema.json`,
a comment explaining that Harness wrote it after the first sign-in and never
overwrites it, and a comment pointing to `harness models` for choices. It sets
`"model": "<provider>/<model>"` to the default for the provider just signed in,
or omits the key if no default can be determined. It has no `provider` entries,
so automatic provider discovery stays enabled. Settings changes can also write the bound runtime or TUI config. Startup does not
write a starter config.

A config with no `provider` entries may still pin a `provider/model`; validation
defers that lookup to runtime discovery. A connected pin is preserved. If its
provider is disconnected, the TUI selects another connected provider's default
or opens the disconnected `/login` state. A noninteractive run with no connection
reports the pinned provider and the `harness auth login` command.

TUI config discovery is separate:

1. `<home>/tui.jsonc` or `<home>/tui.json`, first existing wins
2. `HARNESS_TUI_CONFIG` when set to a custom TUI config path
3. Project layers from the git root down. At each directory, load `tui.jsonc` or
   `tui.json`, then `.harness/tui.jsonc` or `.harness/tui.json`.

When multiple layers exist, the harness merges them instead of replacing the
earlier config wholesale.

Discovery never auto-loads `configs/provider-catalog.reference.jsonc`. That
catalog reference must be passed with `--config` or read as documentation.

## Prompt and instruction discovery

Main profiles use the shared prompt unless overridden by inline
`agent.<name>.system_prompt` / `prompt`. Markdown files under
`.harness/agents/` and `<home>/agents/` define native children, using the
separate discovery and override rules above. A same-name generic profile does
not replace a native child's role.

Instructions load in this order: `<home>/AGENTS.md`, configured `instructions`
entries, then project directories from the Git root to the working directory.
In each project directory, only the first existing file of `AGENTS.md`, then
`CLAUDE.md`, loads. The same first-file rule applies to on-demand per-directory
loading after filesystem tools. Instructions and CLI `--rules` are appended
after the shared prompt and any child role instructions.
An explicit CLI `--system-prompt-override` replaces the main prompt completely;
`--rules` can still append instructions to it.

The system prompt precedes the live user message. Skill metadata is supplied
separately, and definition-listed skill bodies are loaded through startup
permission checks before the child runs. Loading a skill does not expand the
child's tool permissions.

## Skill discovery and V1 skill contract

Markdown skills are local instruction bundles discovered from configured roots.
They do not fetch remote URLs, start MCP servers, register tools, or change
coordinator permissions during discovery. The V1 source scopes emitted by the
skill catalog are `project` and `global`: configured project/workspace roots are
reported as `project` and user roots as `global`. Harness ships no skill pack.
Add your own project skills under `.harness/skills/<skill-name>/SKILL.md`
or personal skills under `<home>/skills/<skill-name>/SKILL.md`.

The runtime config shape is:

```jsonc
{
  "skills": {
    "project_roots": [".harness/skills", ".agents/skills"],
    "global_roots": ["~/.harness/skills", "~/.agents/skills"],
    "disabled": ["skill:project:old-skill", "experimental-*"],
    "walk_to_git_root": true,
    "permissions": {
      "*": "allow",
      "experimental-*": "ask",
      "internal-*": "deny"
    },
    "urls": []
  }
}
```

`project_roots` and `global_roots` accept filesystem paths. Relative project
roots are resolved against the current workspace and, when `walk_to_git_root` is
true, each ancestor up to the nearest `.git` directory. Relative global roots are
resolved from the current workspace, while `~` expands to the operator home
directory. Entries inside each root are sorted by directory name. The first skill
name wins. Later entries with the same name are reported as `shadowed` with an
actionable reason.

Root precedence is deterministic:

1. Non-compatibility project roots, from the current workspace to the nearest Git
   root. At each directory, Harness roots such as `.harness/skills` come first;
   other roots retain their configured order.
2. Non-compatibility global roots, with Harness roots first.
3. Project compatibility roots, including the default `.agents/skills`, from
   the current workspace to the nearest Git root, in configured order.
4. Global compatibility roots, including the default `~/.agents/skills`, in
   configured order.

The default project roots are `.harness/skills` and `.agents/skills`. The default
global roots are `~/.harness/skills` and `~/.agents/skills`. Both expand
`~` from `$HOME`, not `HARNESS_HOME`. These compatibility roots rank below Harness roots, so duplicate
skills remain visible as `shadowed` rather than replacing the Harness copy.
Other assistant roots, including `.claude/skills`, are not searched by default;
add them explicitly to `skills.project_roots` or `skills.global_roots` if needed.

`permissions` is a skill-loading policy keyed by exact names or simple `*`
patterns. `allow` loads immediately, `ask` requests operator confirmation before
activation, and `deny` keeps the skill catalog-visible but unloadable. `disabled`
uses the same name/pattern matching and also accepts stable ids such as
`skill:project:rust-best-practices`; disabled skills are catalog-visible but
cannot be activated through `skill` or subagent definition preloads.
`urls` is accepted as inert/deferred metadata only; V1 discovery never fetches
remote skills.

A skill directory must contain `SKILL.md` with V1 frontmatter:

```markdown
---
name: rust-best-practices
description: Baseline Rust guidance for this workspace.
argument_hint: optional short usage hint
allowed_tools: read, grep
mcp: deferred-local-metadata
resources: references/usage.md, references/checklist.md
---

# Skill body
```

Required fields are `name` and `description`. `name` must match the directory
name and `^[a-z0-9]+(-[a-z0-9]+)*$`; `description` must be 1-1024 characters.
Optional V1 fields are `argument_hint` / `argumentHint`, `allowed_tools` /
`allowedTools` / `expected_tools` / `expectedTools`, `mcp` / `deferred_mcp` /
`deferredMcp`, `resources` / `deferred_resources` / `deferredResources`, and a
string-to-string `metadata` map. `license` and `compatibility` are accepted as
non-runtime metadata. Unsupported public fields make that skill `malformed`
without hiding other valid skills in the same catalog.

Catalog-time metadata includes stable id, name, description, source scope, root
path, file location, loadability, permission mode, status, optional V1 metadata,
`body_loaded: false`, and no full `SKILL.md` body. Full bodies are loaded only
when the `skill` tool activates a loadable skill, including startup preloads
listed in a subagent definition's `skills` field. Startup loads use the child's
skill catalog and the same coordinator tool availability, trust, permission,
and cancellation checks as an ordinary skill call. Missing, denied, disabled,
malformed, or symlink-unsafe preloads are skipped without exposing their bodies.
An `ask` decision waits for operator approval before the child samples.
Successful preload bodies are request-only system instructions, cached for the
child's lifetime and omitted from the ordinary available-skills listing.

Add project skills under a configured root. Describe their purpose, when to use
them, when not to use them, and execution policy. Link longer references rather
than loading them during discovery. Explicit preloads still obey shared skill
permissions and a disabled skill tool.

`allowed_tools` and related skill metadata are descriptive/restrictive contract
metadata only. They never grant runtime tools, override the generic toolset, or
bypass coordinator permission checks. Doctor JSON and support exports consume the
same compact catalog metadata, report loadable/denied/disabled/malformed/shadowed
counts, and keep full skill bodies out of readiness surfaces.

Declared resources remain deferred metadata. Activation reads only the
`SKILL.md` body, with a 1 MiB limit on the file and combined instructions.
`$ARGUMENTS` expands from the skill call's `arguments` string; expanded output
must also fit within 1 MiB.

`harness doctor` validates the operator-facing runtime without making provider or
MCP network calls. It checks provider/model metadata, credential availability
without printing key values, the generic prompt and tool ids, permissions, skill
roots and permission posture, session-directory readiness, and configured MCP
server state. Use `--json` for machine-readable output.

### Generic agent and subagents

Harness materializes one interactive profile named `default`. Child definitions
resolve from nearest project Markdown, then user Markdown, then built-ins.
The coordinator owns their scheduling, permission checks, cancellation, and
append-only lifecycle history.

`spawn_subagent` requires `prompt` and `description`. It defaults to the
`task` definition and `background: true`. A background call returns a
`subagent_id`; `background: false` waits for completion or the configured
foreground timeout, after which the child continues in the background.
`isolation: "worktree"` creates an isolated worktree. `cwd` selects an existing
working directory and cannot be combined with worktree isolation. `resume_from`
continues a completed child's finalized conversation, subject to ownership,
state availability, and context-window checks.

`get_command_or_subagent_output` reads results for `task_ids` and optionally waits
up to `timeout_ms`. `wait_commands_or_subagents` waits for any or all selected
children and background commands. `kill_command_or_subagent` cancels the selected
child or command through the coordinator. `send_subagent_message` routes messages
between authorized agents and can wake a completed recipient when requested.
Hidden tool aliases support older tool callers, but new requests should use the
public names above. The removed `task(load_skills = [...])` interface is replaced
by the definition's `skills` list.

With `inherit_skills: true`, a child receives its actual spawner's startup
catalog. With inheritance disabled, discovery uses the child's effective working
directory and default discovery roots while keeping shared skill permissions.
`discover_skills: false` suppresses that local discovery. Same-identity wake keeps
the original catalog and preload cache. Finalized private state retains the
body-free catalog and preload names. A live wake after restart recovers that
catalog and loads preloads through the ordinary permission gates again. Replay
does not rediscover or load skill files. Older records without a catalog require
a fresh child.

`agent.<name>.system_prompt` replaces that shipped prompt. `tools` accepts either
a list of tool ids or a map of `{ tool_id: enabled }`; disabled map entries are
omitted. `max_iters` / `maxIters` / `steps` / `maxSteps` is optional. When unset,
the runtime does not add an agent-specific iteration cap; the selected agent
continues until the model stops, the user interrupts, or another runtime safety
limit applies. Set an iteration cap only when a profile needs an explicit
per-turn budget. Category routing and alternate primary profiles are rejected.

## Permission policy

To allow ordinary tools with the default safety exceptions, use:

```jsonc
{ "permission": "allow" }
```

`permission` accepts exactly `"ask"`, `"allow"`, or `"deny"`. Scalar `ask` and
`deny` apply to every public permission kind. Scalar `allow` keeps the safety exceptions: ordinary tools (`bash`, `edit`, `task`,
`webfetch`, `websearch`, `codesearch`, `lsp`, `read`) become allow, while
`external_directory` and `doom_loop` stay ask, base `question` stays deny, and
`read` keeps `.env` pattern asks. When `permission`
is omitted, the same allow-with-safety-exceptions defaults apply.

The V1 native tool catalog is documented in
[`docs/tools/native-tool-catalog.md`](../tools/native-tool-catalog.md). `task` controls delegation; `ast_grep_search` uses `codesearch`; `ast_grep_replace` uses `edit`; `session_list`, `session_read`,
`session_search`, and `session_info` are read-only replay/session inspectors with
no additional public permission bucket. Legacy broad `network` remains a
compatibility input for older network-capability tools; new docs and examples
should use `webfetch`, `websearch`, or `codesearch` when a specific public bucket
exists.

Per-tool scalar modes use the same values:

```jsonc
{
  "permission": {
    "bash": "ask",
    "edit": "deny",
    "webfetch": "allow"
  }
}
```

`bash`, `edit`, `task`, `read`, and `external_directory` also accept selector maps.
Put broad rules before exceptions. The last matching rule wins:

```jsonc
{
  "permission": {
    "bash": {
      "*": "deny",
      "git status": "allow",
      "cargo nextest run*": "ask"
    },
    "edit": {
      "*": "deny",
      "docs/**": "allow",
      "crates/harness-core/src/config.rs": "ask"
    },
    "task": {
      "*": "deny",
      "scout": "allow",
      "review-*": "ask"
    }
  }
}
```

Bash selectors are either an exact command string, a trailing `*` prefix such as
`cargo nextest run*`, or the `*` catch-all. Edit selectors are either an exact
workspace-relative path, glob patterns such as `docs/**`, or the `*` catch-all. Task selectors match the requested subagent name;
they accept exact names, `*` catch-all, and simple `*` glob patterns such as
`review-*`. Regex is not supported.

`shell_allowlist` remains supported inside `permission` for shell policy inputs.
It accepts `mode` values `permission_patterns` (the default) and
`legacy_executables`, plus the compatibility aliases `policy_mode` and
`policyMode`. Existing `executables` and `cwd_roots` entries still load, and
`cwdRoots` remains accepted as an alias for `cwd_roots`. In
`permission_patterns` mode, approved interpreter command modes such as
`python3 -c` and heredocs execute normally; environment-dump commands remain
blocked. Outside-workspace `cwd`/`workdir` values go through the
`external_directory` permission gate. Permission decisions allow a call, ask for approval, or deny it. They are not a sandbox
or security boundary. `legacy_executables` retains the stricter executable and
interpreter-mode checks for operators who select it explicitly. Approved
interpreter code can perform host I/O that lexical shell path scanning cannot
enumerate; use an enforced OS sandbox when that access must be confined.

## Edit formats

`edit.mode` selects the model-facing edit format for each agent's current model.
The default is `auto`. GPT, Codex and o-series
models get `apply_patch`, which replaces `edit` and `write`; Kimi, DeepSeek,
MiniMax, MiMo, StepFun and GLM Flash get `replace`, plain old/new string edits,
and every other model gets
`hashline`, line-and-hash anchored edits. Set `hashline`, `replace`, or
`apply_patch` to force one format. Hash anchors appear in file reads only in
hashline mode; the other modes read plain file text. If an agent lacks the chosen
format's tool, it falls back to the editing tool it has.

```jsonc
{
  "edit": {
    "mode": "auto",
    "model_variants": {
      "kimi": "replace",
      "gpt": "apply_patch"
    }
  }
}
```

`edit.model_variants` overrides the mode by case-insensitive model-name
substring. The longest matching key wins; equal lengths fall back to key order.
Values accept the same four modes, including `auto`. `modelVariants` is an input
alias for `model_variants`; new files should use snake_case. Settings exposes the
mode as a choice and the overrides as a JSON object. Saved changes take effect in
the next session.

Legacy `hashline_edit` and `hashlineEdit` booleans still load. `false` maps to
`edit.mode: "replace"`; `true` leaves automatic selection unchanged. A file
cannot contain both a legacy boolean and `edit`, or both legacy spellings.
Settings writes migrate legacy keys to the canonical shape.

## Deprecated compatibility behavior

The loader accepts these runtime sections and input aliases:

- `providers`, `agents`, `permissions`
- `runtime`, `integrations`, `ui`
- `hooks`, `skills`, `lsp`, `logging`, `hashline_edit`
- `providers`, `agents` and `permissions` as aliases for the singular public names
- compatibility permission names such as `shell` and `network`

Those deprecated compatibility aliases and keys are compatibility inputs,
not preferred spellings. New configs and examples should use the runtime/TUI split shown
above. If a canonical key and its alias both appear in one object, loading rejects the
file even when their values agree.

The native permission object is also accepted for compatibility. `defaults`
contains scalar permission modes for `edit`, `shell`, `network`, `question`,
`task`, `eval`, `webfetch`, `websearch`, `codesearch`, `lsp`, `read`,
`external_directory` and `doom_loop`. `rules` contains ordered arrays for
`shell`, `edit`, `task`, `read` and `external_directory`. Each rule requires
`mode` and `selector`; a selector is `"catch_all"`, `{ "exact": "..." }`,
`{ "prefix": "..." }` or `{ "glob": "..." }`. Rule arrays default empty
except the global read rules for environment files. Agent permissions inherit
when omitted and also accept `todowrite`. `permission["*"]` supplies a scalar
fallback for unconfigured kinds. Prefer the public scalar and selector-map forms.

## Validation behavior

- Unsupported top-level areas are limited to active unsupported product features and unknown keys.
- Removed command families have no runtime configuration keys. Top-level `plugin` and `autoupdate` keys are rejected as unknown fields.
- Unsupported TUI fields are rejected explicitly.
- `{env:VAR}` resolves to an empty string when `VAR` is unset.
- `{file:path}` is supported for string references and resolves relative to the config file when the config comes from disk.
- Legacy `${VAR}` and `${VAR:-fallback}` references remain accepted for compatibility.

## Provider context compaction expectations

Provider-context compaction consumes the same redacted request-budget snapshot
prepared for provider dispatch. Resolved model limits, the reserved output,
provider framing, tools, attachments, history, the pending prompt, and the
configured safety margin are accounted once before the snapshot reaches the
compaction path. Model variants still override base model limits during model
resolution; compaction does not reconstruct those limits from scalar metadata.

Known limits produce an estimated compaction threshold, and equality with that
threshold requires compaction. When all model limits are unknown, automatic
compaction has no threshold unless the explicit conservative fallback below is
enabled. Manual and provider-overflow compaction remain available without known
capacity. The preserved history allowance is the snapshot threshold minus
current non-history request components, capped by `keep_recent_tokens`.

Compaction settings live under `runtime.compaction`:

| Key | Default | Purpose |
| --- | --- | --- |
| `enabled` | `true` | Master switch for proactive, pre-prompt, overflow-retry, and manual compaction. When `false`, all compaction paths become no-ops. |
| `threshold_percent` | unset | Fixed percentage of the context window, an integer from 1 through 100. Used when no global token threshold is set. |
| `threshold_tokens` | unset | Fixed positive token count; overrides the global percentage when both are set. |
| `model_thresholds` | `{}` | Percentages or `{ "tokens": count }` keyed by canonical `provider:model` references; override the global threshold. |
| `agent_thresholds` | `{}` | Percentages or `{ "tokens": count }` keyed by agent profile names from `agent`; override model and global thresholds. |
| `reserveTokens` / `reserve_tokens` | `16384` | Safety margin subtracted from the usable context window before compaction is considered. |
| `keepRecentTokens` / `keep_recent_tokens` | `20000` | Target number of recent tokens to preserve verbatim after compaction. The latest complete turn is always preserved. |
| `splitOversizedTurns` / `split_oversized_turns` | `false` | Allows overflow compaction to split an oversized latest turn, summarizing the earlier portion while preserving a suffix as recent provider context. |
| `autoRetryOverflow` / `auto_retry_overflow` | `true` | Enables the one-shot overflow compaction retry after a provider context-window error. Set `false` to fail immediately. |
| `structuredSummaryContract` / `structured_summary_contract` | `true` | Requires summaries to carry the Harness sections `Goal`, `Constraints`, `Progress`, `Key Decisions`, `Next Steps`, and `Critical Context`. Set `false` only for legacy heading compatibility. |
| `estimatedTokenTriggers` / `estimated_token_triggers` | `true` | Enables the explicitly conservative automatic-compaction mode only when all model limits are unknown. This mode is labeled conservative and never claims exact model capacity or a percentage. |
| `suppress_auto_compaction` / `suppressAutoCompaction` | `false` | Suppress automatic pre-request compaction without disabling manual compaction. |
| `fallbackInputTokens` / `fallback_input_tokens` | `32768` | Non-exact conservative input cap used only by that all-limits-unknown mode. Set to `0` (or disable `estimatedTokenTriggers`) to leave capacity unknown and automatic pressure undecided. |

For example, merge this into a config that defines the `explore` agent profile:

```jsonc
{
  "runtime": {
    "compaction": {
      "threshold_percent": 70,
      "model_thresholds": { "openai:gpt-4o-mini": 80 },
      "agent_thresholds": { "explore": 55 }
    }
  }
}
```

For absolute amounts, use `threshold_tokens` globally and `{ "tokens": count }`
inside model/agent maps. Bare numbers inside those maps remain percentages:

```jsonc
{
  "runtime": {
    "compaction": {
      "threshold_tokens": 80000,
      "model_thresholds": { "openai:gpt-4o-mini": { "tokens": 64000 } },
      "agent_thresholds": { "explore": { "tokens": 32000 } }
    }
  }
}
```

Token amounts must be integers from 1 through 4,294,967,295. At the global scope,
`threshold_tokens` takes priority over `threshold_percent`; set it to `null` to
clear an inherited token amount. Precedence is agent profile, then selected model,
then global threshold, then the adaptive default, regardless of the unit. Profile
overrides apply to every instance of that profile. A
fixed override does not change after a high-yield compaction. Without an override,
the default is 45%, 50%, 55%, 60%, 70%, or 80% for windows ending at 16,000, 32,000,
64,000, 128,000, 512,000, or above 512,000 tokens, respectively. Saving more than
half the previous context lowers the next threshold by five percentage points.
Savings count replaced messages and the old summary minus the new summary;
retained messages and provider overhead do not count as savings.
Fractional token thresholds round up; compaction is required at equality.

These are soft thresholds. The model's input budget and reserve still require
compaction earlier when necessary, even with `threshold_percent: 100`. Background
preparation may start before the threshold but cannot commit solely because its
summary is ready. Unknown model capacity still follows the conservative fallback
setting; an override does not establish an unknown model's true limits. Model and
agent threshold maps merge by key across config layers.

On successful compaction, the coordinator appends a single `SessionCompaction` event to the event log and updates the in-memory provider context. The event carries the generated summary, token estimate before compaction, the sequence number of the first preserved event, replay-derived read/modified file lists, the trigger reason, and hook provenance. No separate checkpoint artifact is written; the summary lives entirely in the event and the in-memory `ProviderContext`. Resume reads the latest `SessionCompaction` for the agent and the committed events that follow it. New logs do not persist provider deltas.

Manual `/compact` summarizes older completed turns now, preserves the latest completed turn verbatim, and appends a `SessionCompaction` event. The success notice reports the active-context estimate delta when available, or says the estimate was unchanged. The default summary contract uses the Harness sections for goal, constraints, progress, key decisions, next steps, and critical context, with operational memory and source facts added as replay-derived context; it is still lossy. Sessions with only one completed turn no-op because there is no older turn to summarize.

Lifecycle hooks can veto compaction before its summary is committed. Hook output does not replace the summary. The journal writes `SessionCompaction`; older compaction event variants remain readable. See [hook execution](../operations/hooks.md).

If the provider rejects a request because it exceeds the context window, the coordinator may compact and retry once when the retry can prove it shrank the provider-visible payload. Estimated pre-prompt compaction uses the same `SessionCompaction` path before provider request construction. If a pre-prompt compaction cannot reduce the estimated active context, the coordinator records the failure and does not loop on the same turn.

Failed or aborted provider turns can be preserved in active context. Replay/debug projections keep the incomplete marker, failure stage, and redacted reason so a future provider call does not treat partial assistant output as a completed answer.

Operational memory is derived from persisted events, not from live filesystem scans. The `SessionCompaction` event records capped read-file and modified-file lists, and replay projections expose these facts so operators can see what context survived compaction.

TUI memory or transcript caps are separate presentation settings. They affect what the operator sees on screen, not the persisted provider context used for resume or overflow-retry compaction. The TUI distinguishes active context estimate from cumulative provider tokens spent: active context may decrease after `SessionCompaction`, while total spend remains cumulative and never decreases.

## Runtime behavior guidance

The coordinator adds short model-only reminders to a running turn when it can
see that the agent needs them. Each reminder is appended to the journal as a
`RuntimeReminder` event (with its kind, its exact text, and a source such as an
instruction file or command id) before the model sees it, so resume, replay, and
`harness sessions census` show exactly what the model was told. Settings live
under `runtime.behavior`:

| Key | Default | Purpose |
| --- | --- | --- |
| `todo_continuation.enabled` | `true` | When the main agent ends its turn while its todo list still has pending or in-progress items, remind it of those items and continue the turn. |
| `todo_continuation.max_reminders` | `3` | Reminders per turn. A reminder is only repeated after the agent made tool calls since the previous one, so a deliberate stop ends the turn. |
| `loop_guard.enabled` | `true` | Detect the same tool call (same tool, same arguments) or the same cycle of two or three calls repeated in a row. A call to an exempt tool breaks the run, since a wait or an answer can change what the next call returns. |
| `loop_guard.threshold` | `4` | Repetitions that trigger one reminder; twice this many stops the turn with a loop-guard error. Must be between 2 and 100 while the guard is enabled. |
| `loop_guard.exempt_tools` | waits, output polling, `question` | Tools whose repeated identical calls are expected. |
| `stream_guard.enabled` | `true` | Stop a streamed response whose prose starts repeating the same passage or line, and retry it with a correction. Code blocks, tables, and other structured output are not judged. The stopped request finishes with reason `stream_guard`. |
| `stream_guard.max_retries` | `2` | Corrected retries per turn before the turn fails. |
| `directory_instructions.enabled` | `true` | When the agent reads or edits files under a directory with its own `AGENTS.md` (below the startup instructions), add that file to its context once; compaction and conversation rewind re-arm it. The file is only read when the read policy allows it without asking. |
| `directory_instructions.max_bytes` | `32768` | Longest instruction file added; longer files are truncated and marked. |
| `command_notifications.enabled` | `true` | Tell the agent when a background shell command it started finishes, unless it already saw the result. Also deliver [monitor events](../tools/monitor.md); when off, no monitor events are delivered. A command that finishes during the turn's last model request continues that turn. |
| `command_notifications.wake_idle` | `true` | Start a turn for an idle agent when a background command finishes or a monitor event arrives. When off, notices and monitor events wait for the agent's next turn. |
| `output_contract.max_retries` | `2` | Corrections sent to a subagent whose final answer does not match the `output_schema` its caller passed to `spawn_subagent`. |

While a turn runs, including while its tools
run, `Enter` (`Alt+Enter` in multiline mode) steers it: the message is journaled as
`SteeringAccepted` and joins the turn before its next model request, including a
message that arrives while the turn's last request streams. `Alt+i` or
`Ctrl+Alt+Enter` queues a follow-up that runs after the turn instead. When the
agent is idle, both submit a normal prompt; a draft with tags or attachments
always queues, since steering carries plain text only. Interrupting the turn with
`Ctrl+c` returns undelivered steering and plain queued follow-ups to the editor, in
the order they were sent, instead of running them; they are recorded as cancelled
with reason `returned to the editor` and left out of the transcript. If a turn ends
without taking its steering for another reason (it failed, or was cancelled from
the tasks pane or replaced), the steering starts the next turn. Pending steering is
not replayed after a crash; a stopping run cancels
it like any queued prompt.

Embedders that construct `CoordinatorConfig::new` directly start with every
guard off and opt in by setting `CoordinatorConfig::behavior`.

## Provider retry policy

Provider-request retries are bounded and automatic only for transient provider-side failures (`TransportFailure` and `RateLimited`). Retries happen before the provider response is committed to the session as a completed assistant turn. Each retry issues a fresh provider request id and records the attempt in `ProviderRequestStartedMetadata.retry`. To avoid masking cancellation, an operator or coordinator cancellation attempt wins over an in-flight retry and short-circuits the backoff.

Retry settings live under `runtime.provider_retry`:

| Key | Default | Purpose |
| --- | --- | --- |
| `maxRetries` / `max_retries` | `2` | Maximum automatic retry attempts for a single provider request. Set `0` to disable automatic retries entirely (equivalent to the pre-retry headless path). |
| `baseDelayMs` / `base_delay_ms` | `2000` | Initial retry delay in milliseconds. Exponential backoff doubles this value per attempt, clamped to `maxDelayMs`. |
| `maxDelayMs` / `max_delay_ms` | `30000` | Maximum retry delay in milliseconds. Backoff delays never exceed this value. |

```jsonc
{
  "runtime": {
    "provider_retry": {
      "max_retries": 2,
      "base_delay_ms": 2000,
      "max_delay_ms": 30000
    }
  }
}
```

When a provider response includes a `Retry-After` header, the harness records the value as `retry_after_ms` in the `Error` event metadata. Retry scheduling prefers the provider hint when present, falling back to exponential backoff. Partial provider stream failures and failures after the first committed content chunk are not retried; they are recorded as terminal provider errors instead. Old session logs that lack `ProviderRequestStartedMetadata.retry` replay identically because the coordinator derives retry state from persisted metadata and treats absent retry metadata as the first attempt.
