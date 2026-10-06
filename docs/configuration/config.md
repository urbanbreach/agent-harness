# Config reference

Use `harness.json` or `harness.jsonc` for runtime settings. Use `tui.json` or
`tui.jsonc` for keyboard settings. The generated schemas define the accepted keys:

- [Runtime schema](../../configs/config.json)
- [TUI schema](../../configs/tui.json)

Start with the [starter](#minimal-starter), then check [config precedence](#discovery-and-precedence).
The reference tables cover [runtime keys](#runtime-top-level-keys),
[keybindings](#tui-default-bindings), [permissions](#permission-policy),
[compaction](#provider-context-compaction-expectations), and [retries](#provider-retry-policy).

## Minimal starter

Copy [`configs/harness.example.jsonc`](../../configs/harness.example.jsonc).
It defines a Codex OAuth provider, a default model, the parent and named subagents,
and an optional disabled MCP server. This shorter example omits extra model and
formatter entries:

```jsonc
{
  "$schema": "./config.json",
  "provider": {
    "openai-codex": {
      "type": "openai_compatible",
      "name": "OpenAI Codex",
      "options": {
        "authProvider": "codex",
        "baseURL": "https://api.openai.com/v1",
        "apiKeyEnv": ["OPENAI_API_KEY"],
        "timeoutMs": 1800000,
        "cacheRetention": "short"
      },
      "models": {
        "gpt-5.5": {
          "name": "GPT 5.5",
          "metadata": { "supportsToolCalls": true },
          "limit": { "context": 272000, "input": 272000, "output": 128000 },
          "variants": {
            "low": { "name": "Low", "metadata": { "reasoningEffort": "low" } },
            "medium": { "name": "Medium", "metadata": { "reasoningEffort": "medium" } },
            "high": { "name": "High", "metadata": { "reasoningEffort": "high" } },
            "xhigh": { "name": "XHigh", "metadata": { "reasoningEffort": "xhigh" } }
          }
        },
        "gpt-5.4-mini": {
          "name": "GPT 5.4 Mini",
          "metadata": { "supportsToolCalls": true },
          "limit": { "context": 272000, "input": 272000, "output": 128000 },
          "variants": {
            "low": { "name": "Low", "metadata": { "reasoningEffort": "low" } },
            "medium": { "name": "Medium", "metadata": { "reasoningEffort": "medium" } },
            "high": { "name": "High", "metadata": { "reasoningEffort": "high" } }
          }
        }
      }
    }
  },
  "model": "openai-codex/gpt-5.4-mini",
  "agent": {
    "default": { "variant": "high" }
  },
  "permission": "allow",
  "mcp": {
    "cargo-mcp": {
      "transport": "stdio",
      "command": ["cargo-mcp", "serve"],
      "enabled": false
    }
  }
}
```

Only set values you need to override. `agent.default` configures the interactive
parent. Native children use the bundled definitions and `subagents` settings below.
Keep larger model catalogs, tool lists, background-task knobs, and
compaction defaults out of day-to-day configs unless a project needs a deliberate
override.

Each `variants` entry is a named model preset; for OpenAI-compatible reasoning
models, set `metadata.reasoningEffort` so the TUI can display and select variants
like `low`, `medium`, or `high`. Use additional variant fields only for
non-standard names or per-variant limits, modalities, or options.

OpenAI-compatible providers accept `cacheRetention` either beside the provider
fields or under `options`. The default is `short`: the runtime sends a stable,
clamped, per-session `prompt_cache_key` when a session id is available. Set
`cacheRetention: "none"` to omit cache-affinity request fields. Set
`cacheRetention: "long"` only when you want provider-supported extended
retention; the current transport emits `prompt_cache_retention: "24h"` only for
direct `api.openai.com` OpenAI-compatible requests and otherwise falls back to
the stable key.

OpenAI-compatible providers may also set `authProvider` to `codex` or
`github-copilot`. That opt-in keeps the OpenAI-compatible transport while letting
the runtime resolve credentials from the secure credential store before falling
back to `apiKeyEnv` and inline `apiKey`. Stored credentials live outside
`harness.json{,c}` under the platform data directory at
`credentials/{authProvider}.json`, are atomically replaced, and use restrictive
file permissions: POSIX `0600`, and on Windows a protected owner-only DACL.

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

Definitions resolve nearest project, builtin, user/compatibility, bundled,
enabled plugin, then session CLI fallback. Only project definitions shadow
builtins. Qualified plugin names use `plugin:name`; bare plugin names must be
unambiguous. Agent Markdown files use YAML frontmatter. Project discovery walks
from the current parent directory to the worktree root. It checks
`.agent-harness/agents`, `.harness/agents`, and the compatibility layout
`.claude/agents`. User roots, bundled roots and plugin directories are
explicit discovery inputs.

Inline roles/personas override trusted project `.toml` files, then user files,
then bundled files. Untrusted project role/persona files are skipped. A role
alone never creates a callable type. Relative prompt paths use the preset
file's source directory, or the current parent directory for inline presets.
Persona errors abort resolution; a missing role prompt emits a warning and
continues. Type-specific roles win over persona-named roles.

Runtime model/effort overrides win over role then persona defaults. Valid model
overrides precede per-type model pins, definition model and current parent model;
unknown internal pins warn and fall through. Fresh public models require a
catalog validator; resume ignores that argument and the actor pins the source
model. Capability modes intersect runtime, role and definition ceilings.
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
| `sonic` | Mechanical edits or data collection with task tools, medium effort. |

All inherit the parent model unless pinned in `subagents.models`. `small_model`,
when configured, supplies the default for `scout` and `sonic`; per-type pins win.
Every child loses ask-user,
feedback and workflow tools; parent operator allow/deny restrictions still apply.

For runtime integration, `HarnessConfig.subagents.resolve_with_lookup` returns
`SubagentRuntimeConfig` using explicit CLI, feature, remote, managed requirement
and environment lookup inputs. `discover_subagent_definitions` captures read-only
definitions, presets and prompt-file outcomes under explicit cwd/trust/root/plugin
inputs. `resolve_subagent_definition` is pure: its
`SubagentDefinitionContext` supplies the current parent model/effort/maxTurns,
allowlist, catalog, latched selection, tool inventory, operator restrictions,
permission ceiling, injected child depth, MCP and skill snapshots. It returns
the resolved definition, prompts, model, tools and inheritance metadata.
The coordinator owns authorization, catalog readiness, source-model resume
pinning, worktree creation, scheduling and dispatch. Optional type schemas list
at most 64 names (128 bytes each), with normalized 200-byte descriptions; this
presentation bound does not reject otherwise valid types.

## Model prompts

Harness selects a model Markdown template over the shared system prompt. Specific
model versions take precedence over family defaults; catalog `metadata.family`
supports aliases. `harness models --json` includes `resolution.prompt_preset`.
The system prompt and eval tool description receive the selected eval guidance.

Edit `.agent-harness/prompts/models/glm-5.3.md` or
`.agent-harness/prompts/models/gpt-6.1-sol.md` to customize those models without
rebuilding. Project files override `$XDG_CONFIG_HOME/harness/prompts` or
`~/.config/harness/prompts`, followed by bundled defaults. Shared `system.md`,
`personality.md`, `subagent.md`, and `eval/*.md` files use the same precedence.
Templates reload on turns, tool iterations and model changes. Invalid selected
files fail instead of silently falling back.

Native children use their actual model with the root agent's prompt locations,
then add their role and completion rules. A nonempty
`agent.<name>.system_prompt` remains a literal system-body override. Project
instructions and command rules remain appended. See the
[editable prompt guide](../../.agent-harness/prompts/README.md) for filenames,
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

The copied `configs/harness.example.jsonc` targets Codex OAuth by default through
the `openai-codex` provider id. It keeps credentials out of config by
using `authProvider: "codex"` plus `apiKeyEnv` fallback. A typical non-OAuth
OpenAI-compatible setup still uses:

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

Use `harness auth list [--json]` to inspect configured `codex` and
`github-copilot` auth providers with redacted status. Run `harness auth login`
for the standalone auth picker: provider order is OpenAI, then GitHub
Copilot; OpenAI offers `ChatGPT Pro/Plus (browser)`, `ChatGPT Pro/Plus
(headless)`, and `Manually enter API Key`; GitHub Copilot prompts for
GitHub.com vs GitHub Enterprise before device-code login. Explicit commands still
bypass the picker: `harness auth login <provider> --method device|browser|api-key`
stores or replaces the active stored credential for that auth-provider id. The
`--method` value also accepts the matching reference implementation labels, such as `ChatGPT
Pro/Plus (browser)`, `ChatGPT Pro/Plus (headless)`, `Manually enter API Key`,
and `Login with GitHub Copilot`. Codex supports device, browser, and API-key
stdin login; browser login can also complete from an SSH session by pasting the
final localhost callback URL into the terminal if the remote loopback callback is
not reachable from the desktop browser. GitHub Copilot supports device-code login
for V1. Use
`harness auth logout <provider>` to delete only the stored credential file;
config and environment fallbacks are not edited. The TUI exposes the same auth
entry point through `/auth` (`/login`) and the `Auth` command-palette row.

Codex OAuth follows the ChatGPT PKCE/device-code reference flow and decorates the
existing OpenAI-compatible transport with the Codex endpoint, bearer token, and
account/session headers. GitHub Copilot OAuth follows the reference implementation Copilot
device-code reference: the GitHub device `access_token` is stored as the active
OAuth credential and sent directly as the Copilot bearer; no separate
GitHub-to-Copilot token exchange is performed in the deterministic V1 path.
Copilot Enterprise credentials store the normalized enterprise domain so request
decoration can select `https://copilot-api.<domain>` while public Copilot uses
`https://api.githubcopilot.com`.

`harness doctor` keeps secret values redacted. For `apiKeyEnv` fallbacks, doctor checks that the named environment variable is present. For `authProvider`
entries, doctor checks stored credential presence before environment or inline fallbacks, and doctor does not prove live provider authentication or transport health; use a live prompt or signoff-live lane when you need transport and credential proof.

## Public contract summary

| Area | Canonical shape | Notes |
| --- | --- | --- |
| Runtime config file | `harness.json` / `harness.jsonc` | Shared defaults live under the matching XDG harness directory. |
| TUI config file | `tui.json` / `tui.jsonc` | Runtime and TUI settings are intentionally split. |
| Core runtime keys | `provider`, `model`, `small_model`, `agent`, `permission`, `mcp`, `skills`, `instructions`, plus Harness runtime extensions | `agent` configures the `default` parent and custom profiles; `subagents` configures native child definitions. |
| TUI settings | `keybinds`, `confirm_before_rewind` | Unsupported TUI-only fields fail validation. |
| Permission naming | `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`, plus safety kinds `read`, `external_directory`, and `doom_loop` | Legacy `shell` / `network` remain compatibility-only. `external_directory` and `doom_loop` default to ask; `read` defaults to allow with `.env` pattern asks. |
| Prompt assets | `.agent-harness/prompts/models/*.md`, shared templates and existing agent definitions | `AGENTS.md` is auto-discovered separately as project context. |

Runtime and TUI config stay separate. Runtime config controls providers,
models, the generic agent, permissions, MCP, skills, instructions, and compaction. TUI
config accepts `$schema`, `keybinds`, and `confirm_before_rewind`; use `tui.json` or `tui.jsonc`
for those settings instead of mixing them into runtime config.

## Runtime top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `agent` | Generic `default` parent and named subagent tuning. Alternate primary roles and category routes are rejected. |
| `autoshare` | Upstream-compatible sharing flag; inactive `false` is accepted, active sharing is rejected. |
| `command` | Upstream command configuration; accepted only when empty because the harness does not execute configured commands. |
| `disabled_providers` | Upstream-compatible provider filter; hides matching configured and authenticated built-in providers from runtime model catalogs. |
| `enabled_providers` | Upstream-compatible provider allow-list; when non-empty, only matching configured/authenticated built-in providers remain in runtime model catalogs. |
| `formatter` | Formatter registry. `false` disables formatters; `true` enables all 26 built-in formatters (the default when the key is omitted). An object accepts `enabled`, `experimentalOxfmt`, and named formatter entries such as `<name>: { disabled?, command?, environment?, extensions? }`. Built-in formatter names are `gofmt`, `mix`, `prettier`, `oxfmt`, `biome`, `zig`, `clang-format`, `ktlint`, `ruff`, `air`, `uv`, `rubocop`, `standardrb`, `htmlbeautifier`, `dart`, `ocamlformat`, `terraform`, `latexindent`, `gleam`, `shfmt`, `nixfmt`, `rustfmt`, `pint`, `ormolu`, `cljfmt`, `dfmt`. Formatters are selected by name, not by extension; each built-in formatter declares its own extensions, and an `extensions` override replaces the built-in list. `command` overrides discovery entirely; `environment` merges with the built-in environment (override wins). `$FILE` is substituted with the target file path. When several formatters match a file, they run sequentially in built-in registry declaration order, followed by any custom override-only formatters; failures surface as non-fatal warnings. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `lsp` | Upstream-compatible LSP setting; `false` disables harness LSP overrides, object values map to harness LSP servers when possible. |
| `mcp` | MCP server definitions keyed by server name. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback metadata; runtime profile resolution selects the primary target in V1. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. Supports scalar `allow`/`ask`/`deny` or per-tool pattern maps. Catch-all deny hides tools from the model; last matching pattern wins. |
| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime settings including startup approval mode, provider-context compaction settings, and provider retry policy. |
| `server` | Upstream server configuration; accepted only when empty because server commands are outside this runtime config. |
| `small_model` | Optional smaller model reference for coordinator-owned internal operations such as title generation. |
| `skills` | Shared skill discovery roots and permission overrides for skill loading. |

## Variable substitution

The harness resolves variable references in config values before parsing. The resolver makes one pass through all values. It does not expand nested
references such as `${VAR:-${OTHER}}` recursively.

| Syntax | Behavior |
| --- | --- |
| `{env:VAR}` | Environment variable substitution. Returns an empty string if `VAR` is missing from the environment. |
| `{file:path}` | File content substitution. The path is resolved relative to the config file directory; absolute paths are used as-is. |
| `${VAR}` | Shell-style environment variable. If `VAR` is missing from the environment, this produces a config error rather than expanding to an empty string. Use `${VAR:-}` for an explicit empty fallback. |
| `${VAR:-fallback}` | Environment variable with fallback value. If `VAR` is missing or empty, `fallback` is used. |

`apiKeyEnv` tries environment variables as credential fallbacks and redacts their
values. It is separate from `{env:VAR}` substitution.

## Effective config inspection

Print the merged runtime config after discovery, layer merge, and session-dir
overrides:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config show --effective
cargo run -p harness -- --config configs/harness.example.jsonc config sources
cargo run -p harness -- --config configs/harness.example.jsonc config explain model
```

### `config show --effective`

Output is a JSON envelope:

- `schema_version`: `harness-config-effective-v1`
- `redacted`: always `true` for this command
- `layers`: discovered config file paths in merge order
- `primary_path`: highest-precedence runtime config path when present (TUI-only
  paths remain listed under `layers`)
- `effective`: the merged config value after secret redaction

Secret-bearing fields (for example `apiKey`) are replaced with redaction
markers. `config show` without `--effective` exits with usage status `2`.

### `config sources`

Lists discovered layers in merge order (`harness-config-sources-v1`):

- `order`, `path`, `exists`, `kind` (`runtime` or `tui`), `primary`
- `merge_order` documents that later layers override earlier ones

### `config explain <path>`

Explains one dotted public path (for example `model` or
`provider.default.options.apiKey`) as `harness-config-explain-v1`:

- `found`, `effective` (redacted), `source_path` (last layer that defines the path)
- per-layer `defines_path` / redacted `value` rows for attribution
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

JSON objects merge key by key before defaults and required-model validation.
Omitted sections inherit the earlier value. Arrays replace earlier arrays, except
`instructions`, which accumulates in layer order. File permission references
resolve relative to the file that declares them.

Explicit JSON agent fields take precedence over markdown frontmatter. Empty or
default fields can fall back to frontmatter. Project markdown overrides a shipped
agent with the same name.

## Extension manifest descriptors

Typed extension manifests are not a runtime config key in V1. The descriptor
schema lives at
[`configs/extension-manifest.v1.schema.json`](../../configs/extension-manifest.v1.schema.json)
and is validated by `harness-core::extension_manifest::ExtensionManifestV1`.
The parser reads descriptors only. parsing a manifest records stable extension ids,
capability ids, disablement defaults, optional tool/hook/command/prompt/MCP
bundle/diagnostic/provider-decorator descriptors, public permission names for
tool descriptors, and static replay metadata. It does not discover manifests
from config, register tools, execute commands, launch MCP servers, invoke
provider decorators, load external code, or mutate sessions. Future executable
extension behavior must be configured through a new host design and still route
through coordinator permissions, artifact/redaction paths, and replay-safe
metadata.

## TUI top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `keybinds` | Supported TUI keybinding overrides. |
| `confirm_before_rewind` | Ask before rewinding a conversation (default `true`). The rewind panel’s “Yes, and don’t ask again” choice saves `false`. |

## TUI default bindings

`tui.json{,c}` `keybinds` overrides use the action ids below. The table records
the primary shipped binding for each default-bound action; some surfaces also
keep secondary aliases for compatibility. The input-only alias
`toggle_operator_sidebar` still maps to `open_status_dialog` so persisted user
configs keep working; it is not a canonical example or serialization key.

Set the special `leader` key to change the two-step leader prefix. The default
leader is `Ctrl+x`. Action values can be comma-separated to keep multiple
bindings, and `<leader>` expands to the configured leader key, for example
`"switch_model": "<leader>m, ctrl+m"`.

| Action | Primary binding | Purpose |
| --- | --- | --- |
| `palette` | `Ctrl+p` | Open the command palette. |
| `new_session` | `Ctrl+x n` | Start a fresh live session. |
| `resume_session` | `Ctrl+x l` | Continue a prior session. |
| `switch_model` | `Ctrl+x m` | Open the model switcher. |
| `open_status_dialog` | `Ctrl+x s` | Open the status dialog. |
| `open_lineage_browser` | `Ctrl+x g` | Open the lineage browser. |
| `compact_session` | `Ctrl+x c` | Request session compaction when available. |
| `help` | `?` | Open the shortcuts/help surface. |
| `quit` | `q` | Quit the TUI. |
| `agent_cycle` | `Tab` | Cycle to the next primary agent. |
| `agent_cycle_reverse` | `Shift-Tab` | Cycle to the previous primary agent. |
| `focus_next` | `Ctrl+Tab` | Move focus forward. |
| `focus_prev` | `Ctrl+Shift-Tab` | Move focus backward. |
| `toggle_terminal_panel` | `4` | Show or hide terminal output. |
| `toggle_follow` | ` ` | Toggle transcript follow mode. |
| `close_review_surface` | `1` | Return to the transcript-first session shell. |
| `session_background` | `Ctrl+b` | Move foreground subagents to the background. |
| `session_child_first` | `Ctrl+x ↓` | Jump to the first child session. |
| `session_child_cycle` | `→` | Cycle to the next child session. |
| `session_child_cycle_reverse` | `←` | Cycle to the previous child session. |
| `session_parent` | `↑` | Return to the parent session. |
| `diff_hunk_next` | `Alt+n` | Jump to the next diff hunk. |
| `diff_hunk_previous` | `Alt+p` | Jump to the previous diff hunk. |
| `move_down` | `j` | Move down in the active list. |
| `move_up` | `k` | Move up in the active list. |
| `submit_prompt` | `Enter` | Submit the prompt. |
| `insert_newline` | `Shift+Enter` | Insert a prompt newline. |
| `clear_prompt` | `Esc` | Clear the prompt. |
| `history_up` | `Up` | Recall the previous prompt history item. |
| `history_down` | `Down` | Recall the next prompt history item. |
| `cursor_left` | `Left` | Move the prompt cursor left. |
| `cursor_right` | `Right` | Move the prompt cursor right. |
| `backspace` | `Backspace` | Delete before the prompt cursor. |
| `delete` | `Del` | Delete after the prompt cursor. |
| `allow_permission` | `Ctrl+y` | Allow a pending permission request. |
| `yolo_mode` | `Ctrl+o` | Toggle YOLO for this session; opens confirmation when a permission prompt is active. |
| `deny_permission` | `Ctrl+n` | Deny a pending permission request. |
| `dismiss_modal` | `Esc` | Dismiss or reject the active modal. |
| `variant_cycle` | `Ctrl+t` | Cycle the active model variant/reasoning preset. |

TUI prompt history is runtime state, not config. Interactive startup and live
sessions load and append prompt history at `<session-dir>/tui/prompt-history.json`
using a versioned JSON schema, so submitted prompts survive process restarts while
unsent drafts stay in the active composer until submitted or discarded.

## Discovery and precedence

Runtime config discovery merges these layers from lowest to highest precedence:

```mermaid
flowchart LR
    Global[XDG global files] --> Env[HARNESS_CONFIG]
    Env --> Project[Project files]
    Project --> Agent[.agent-harness files]
    Agent --> Inline[HARNESS_CONFIG_CONTENT]
    Inline --> Result[Effective configuration]
```

Later layers override earlier values. Objects merge; most arrays replace the
earlier array. `instructions` accumulates. The ordered locations are:

1. `$XDG_CONFIG_HOME/harness/harness.jsonc` (fallback `~/.config/harness/harness.jsonc`)
2. `$XDG_CONFIG_HOME/harness/harness.json` (fallback `~/.config/harness/harness.json`)
3. `HARNESS_CONFIG` when set to a custom runtime config path
4. project `harness.jsonc` / `harness.json` files discovered while traversing upward to the nearest `.git` directory
5. project `.agent-harness/harness.jsonc` / `.agent-harness/harness.json` files discovered during the same traversal
6. `HARNESS_CONFIG_CONTENT` as the final runtime overlay

Additional compatibility input still loads from `$XDG_CONFIG_HOME/harness/config.jsonc` and from the older broad runtime shape when present.

TUI config discovery is separate and layered the same way:

1. `$XDG_CONFIG_HOME/harness/tui.jsonc` (fallback `~/.config/harness/tui.jsonc`)
2. `$XDG_CONFIG_HOME/harness/tui.json` (fallback `~/.config/harness/tui.json`)
3. `HARNESS_TUI_CONFIG` when set to a custom TUI config path
4. project `tui.jsonc` / `tui.json` files discovered while traversing upward to the nearest `.git` directory
5. project `.agent-harness/tui.jsonc` / `.agent-harness/tui.json` files discovered during the same traversal

When multiple layers exist, the harness merges them instead of replacing the
earlier config wholesale.

Discovery never auto-loads `configs/provider-catalog.reference.jsonc`. That
catalog reference must be passed with `--config` or read as documentation.

## Prompt and instruction discovery

Main profiles use the shared prompt unless overridden by inline
`agent.<name>.system_prompt` / `prompt` or discovered profile Markdown at
`.agent-harness/agents/<name>.md`. Native children resolve their definitions
through the separate discovery and override rules above. A same-name generic
profile does not replace a native child's role.

Project instructions are auto-discovered from `AGENTS.md`. Configured
`instructions` entries precede discovered project instructions. They and CLI
`--rules` are appended after the shared prompt and any child role instructions.
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
reported as `project`, user/XDG roots are reported as `global`, and the starter
skills checked into `.agent-harness/skills` are ordinary project-scope skills
when the current workspace is this repository.

The runtime config shape is:

```jsonc
{
  "skills": {
    "project_roots": [".agent-harness/skills", ".harness/skills"],
    "global_roots": ["~/.config/agent-harness/skills"],
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

V1 root precedence is deterministic:

1. Project/workspace roots from the current workspace up to the nearest `.git`
   ancestor. At each ancestor, Harness-owned roots (`.agent-harness/skills`, then
   `.harness/skills`) are searched before other non-compatibility project roots;
   roots in the same class keep their configured order.
2. Non-compatibility global roots. Harness-owned global roots such as
   `~/.config/agent-harness/skills` are searched before other global roots in the
   same class.
3. Explicitly configured project compatibility roots, from the current workspace
   up to the nearest `.git` ancestor, in configured order.
4. Explicitly configured global compatibility roots, in configured order.

External editor, assistant, and agent compatibility roots are adapter work, not
default V1 discovery. The harness does not search `.external-editor/skills`,
`.assistant/skills`, `.agents/skills`, user-level `.external-editor`,
user-level `.assistant`, or user-level `.agents` roots unless the operator
explicitly lists those paths in `skills.project_roots` or `skills.global_roots`.
When they are listed, they are imported after Harness-owned and other
non-compatibility roots, even if the compatibility path appears earlier in the
config array. Therefore `.external-editor/skills/foo/SKILL.md`,
`.assistant/skills/foo/SKILL.md`, or `.agents/skills/foo/SKILL.md` cannot shadow
`.agent-harness/skills/foo/SKILL.md`, `.harness/skills/foo/SKILL.md`, or a
configured `~/.config/agent-harness/skills/foo/SKILL.md`. If only compatibility
roots contain `foo`, configured project compatibility roots win before
configured global compatibility roots, and duplicate compatibility roots resolve
in their configured order.

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
resources: bundled-reference-not-loaded
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

The operator chose to retain shared permission checks: explicit
preloads do not bypass shared skill permissions or a disabled skill tool.
These checks apply during startup as well as ordinary skill calls.

`allowed_tools` and related skill metadata are descriptive/restrictive contract
metadata only. They never grant runtime tools, override the generic toolset, or
bypass coordinator permission checks. Doctor JSON and support exports consume the
same compact catalog metadata, report loadable/denied/disabled/malformed/shadowed
counts, and keep full skill bodies out of readiness surfaces.

`harness doctor` validates the operator-facing runtime without making provider or
MCP network calls. It checks provider/model metadata, credential availability
without printing key values, the generic prompt and tool ids, permissions, skill
roots and permission posture, session-directory readiness, and configured MCP
server state. Use `--json` for machine-readable output.

### Generic agent and subagents

Harness materializes one interactive profile named `default`. Child definitions
resolve through the configured CLI, project, user, plugin, and bundled sources.
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
workspace-relative path, a trailing `/` path prefix such as `docs/`, or the
`*` catch-all. Task selectors match the requested subagent name;
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

## Deprecated compatibility behavior

The loader still accepts the previous broad harness-native shape for migration:

- `providers`, `agents`, `permissions`
- `runtime`, `integrations`, `ui`
- `hooks`, `skills`, `lsp`, `logging`, `hashline_edit`
- compatibility aliases such as `categories`, `profiles`, `backgroundTask`, `paths`, and `deterministic`
- compatibility permission names such as `shell` and `network`
- compatibility config path `$XDG_CONFIG_HOME/harness/config.jsonc`

Those deprecated compatibility aliases, keys, and paths are compatibility inputs,
not the canonical public contract. New configs, examples, docs, and
schema-driven validation should use the harness-centered runtime/TUI split shown
above. If a canonical key and compatibility alias both appear with conflicting
values, config loading rejects the file instead of silently choosing one.

## Validation behavior

- Unsupported top-level areas are limited to active unsupported product features and unknown keys.
- Unsupported compatibility top-level areas that would trigger product side effects (`server`, `command`, `autoshare`) are rejected when active; inactive forms such as empty maps/lists are accepted. Compatibility-only keys (`plugin`, `share`, `autoupdate`, `enterprise`) are accepted in any form but have no effect.
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
