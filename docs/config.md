# Config reference

The harness public config contract uses harness-centered naming throughout.
Canonical runtime config discovery uses `harness.json` / `harness.jsonc`; TUI-only
settings use `tui.json` / `tui.jsonc`.

The generated JSON schemas are the source of truth:

- runtime: `configs/config.json`
- tui: `configs/tui.json`

## Minimal starter

Start with `configs/harness.example.jsonc`. It keeps the happy path small: one
Codex OAuth-backed OpenAI-compatible provider, two GPT-family model entries, explicit
tool-call capability metadata, Category scale through model profiles
with primary targets plus validated fallback metadata,
per-agent model choices for the shipped profiles, scalar permission mode, and
optional MCP. The full file is the canonical example; the excerpt below is
intentionally abridged but keeps the same provider/model/agent shape and the
fields that affect first-run behavior.

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
  "model_profile": {
    "category-visual-engineering": {
      "model": "openai-codex/gpt-5.5",
      "variant": "high",
      "fallback": [{ "model": "openai-codex/gpt-5.4-mini", "variant": "high" }]
    },
    "category-artistry": {
      "model": "openai-codex/gpt-5.5",
      "variant": "high",
      "fallback": [{ "model": "openai-codex/gpt-5.4-mini", "variant": "high" }]
    },
    "category-ultrabrain": {
      "model": "openai-codex/gpt-5.5",
      "variant": "xhigh",
      "fallback": [{ "model": "openai-codex/gpt-5.4-mini", "variant": "high" }]
    },
    "category-deep": {
      "model": "openai-codex/gpt-5.5",
      "variant": "medium",
      "fallback": [{ "model": "openai-codex/gpt-5.4-mini", "variant": "high" }]
    },
    "category-quick": {
      "model": "openai-codex/gpt-5.4-mini",
      "variant": "low",
      "fallback": [{ "model": "openai-codex/gpt-5.5", "variant": "low" }]
    },
    "category-unspecified-low": {
      "model": "openai-codex/gpt-5.4-mini",
      "variant": "medium",
      "fallback": [{ "model": "openai-codex/gpt-5.5", "variant": "medium" }]
    },
    "category-unspecified-high": {
      "model": "openai-codex/gpt-5.5",
      "variant": "high",
      "fallback": [{ "model": "openai-codex/gpt-5.4-mini", "variant": "high" }]
    },
    "category-writing": {
      "model": "openai-codex/gpt-5.4-mini",
      "variant": "medium",
      "fallback": [{ "model": "openai-codex/gpt-5.5", "variant": "medium" }]
    }
  },
  "agent": {
    "build": { "enable": true, "model": "openai-codex/gpt-5.4-mini", "variant": "high" },
    "plan": { "enable": true, "model": "openai-codex/gpt-5.5", "variant": "xhigh" },
    "general": { "enable": true, "model": "openai-codex/gpt-5.4-mini", "variant": "medium" },
    "explore": { "enable": true, "model": "openai-codex/gpt-5.4-mini", "variant": "low" },
    "visual-engineering": { "enable": true, "model": "category-visual-engineering" },
    "artistry": { "enable": true, "model": "category-artistry" },
    "ultrabrain": { "enable": true, "model": "category-ultrabrain" },
    "deep": { "enable": true, "model": "category-deep" },
    "quick": { "enable": true, "model": "category-quick" },
    "unspecified-low": { "enable": true, "model": "category-unspecified-low" },
    "unspecified-high": { "enable": true, "model": "category-unspecified-high" },
    "writing": { "enable": true, "model": "category-writing" },
    "title": { "enable": true, "hidden": true },
    "summary": { "enable": true, "hidden": true },
    "compaction": { "enable": true, "hidden": true }
  },
  "default_agent": "build",
  "permission": "ask",
  "mcp": {
    "cargo-mcp": {
      "transport": "stdio",
      "command": ["cargo-mcp", "serve"],
      "enabled": false
    }
  }
}
```

Only write the settings you want to own. The canonical example lists built-in
agents for discoverability and pins category routes through named model profiles
so doctor/TUI/task metadata agree. The starter adapts provider-specific reference
category defaults into the local GPT-family catalog; larger catalogs can retarget
the same `category-*` profile names to Gemini, Claude, Kimi, or other available
providers. Each agent still inherits the shipped description, prompt,
permissions, and tools unless you override those fields. Keep larger model
catalogs, agent tool lists, background-task knobs, and compaction defaults out of
day-to-day configs unless a project needs a deliberate override.

Reasoning-effort presets use the same explicit `variants` shape as the upstream
local-coding config style.
Each variant is a named model option preset; for OpenAI-compatible reasoning
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

## V1 model prompt tuning stance

Provider-family prompt selection is routed through the explicit model-resolution
seam in `harness_core::model_resolution`, which prefers catalog
`metadata.family` and falls back to a documented heuristic/default family. The
base prompt is composed through `crates/harness/src/dynamic_prompt.rs`, markdown
agent assets, and non-GPT family prompt bodies under
`.agent-harness/prompt-families/{family}.md` for `anthropic`, `gemini`, `kimi`,
and `trinity`. If a referenced family prompt asset is missing or empty, the
runtime fails closed to the documented default prompt and `doctor --json` reports
`model.prompt_family_asset.status = "fallback"` with the relative asset path and
warning. Model-specific differences in this slice are explicit catalog metadata
such as family, modalities, context/output limits, variants, reasoning support,
and data-backed family prompts, rather than scattered raw `model_id.contains(...)` checks.
If provider/model prompt presets are added later, they must be named
presets layered over the base prompt and covered by golden prompt tests.

The larger provider catalog lives in `configs/provider-catalog.reference.jsonc`.
That file is a reference and validation fixture for provider and model metadata,
including variants and larger model lists. It is not auto-loaded by config
discovery. Validate it explicitly when you want to check the catalog:

```bash
cargo run -p harness -- --config configs/provider-catalog.reference.jsonc config validate
```

You can also update the checked-in generated provider catalog from the public
models.dev capability dataset, similar to the reference generated model registry:

```bash
cargo run -p harness -- models generate
```

`models generate` is an explicit offline-maintenance command, not runtime
discovery. By default it fetches `https://models.dev/api.json`, filters to
non-deprecated tool-call-capable models, and writes
`configs/provider-catalog.generated.json`. The harness binary embeds that file
with `include_str!`, so `models generated` can print the static registry without
network access, matching the generate-then-bundle workflow. Use
`--input <file>` or `--stdin` for deterministic runs from a saved API response,
`--provider <id>` to restrict output, `--include-non-tool` /
`--include-deprecated` to broaden the catalog. `models generate` always emits
low/medium/high reasoning presets for models that advertise reasoning support;
`models probe` uses `--emit-reasoning-variants` when you want the same presets in
scratch output to stdout or `--output`. Committed updates should go through
`models generate`.
Review generated provider `baseURL` values before merging; models.dev describes
many providers, while the harness currently executes only OpenAI-compatible
transports.

### First-run provider authentication

The copied `configs/harness.example.jsonc` targets Codex OAuth by default through
the `openai-codex` provider id. It keeps credential material out of config by
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
credential material out of config:

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
| Core runtime keys | Upstream-compatible `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`, plus harness runtime extensions | Side-effectful upstream product areas are accepted only when inactive and rejected when active. |
| TUI surface | `keybinds` | Unsupported TUI-only fields fail validation. |
| Permission naming | `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp` | Legacy `shell` / `network` remain compatibility-only. |
| Prompt asset discovery | `.agent-harness/agents/*.md` | `AGENTS.md` is still auto-discovered separately. |

Runtime and TUI config stay separate. Runtime config controls providers,
models, agents, permissions, MCP, skills, instructions, and compaction. TUI
config stays limited to `$schema` plus `keybinds`; use `tui.json` or `tui.jsonc`
for those settings instead of mixing them into runtime config.

## Runtime top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `agent` | Optional agent overrides or custom agent definitions. |
| `autoshare` | Upstream-compatible sharing flag; inactive `false` is accepted, active sharing is rejected. |
| `command` | Upstream command configuration; accepted only when empty because the harness does not execute configured commands. |
| `default_agent` | Default interactive agent selected at startup; the shipped example keeps `build` as the default while `plan` remains selectable. |
| `disabled_providers` | Upstream-compatible provider filter; hides matching configured and authenticated built-in providers from runtime model catalogs. |
| `enabled_providers` | Upstream-compatible provider allow-list; when non-empty, only matching configured/authenticated built-in providers remain in runtime model catalogs. |

| `formatter` | Formatter registry. `false` disables formatters; `true` enables all 26 built-in formatters (the default when the key is omitted). An object accepts `enabled`, `experimentalOxfmt`, and named formatter entries such as `<name>: { disabled?, command?, environment?, extensions? }`. Built-in formatter names are `gofmt`, `mix`, `prettier`, `oxfmt`, `biome`, `zig`, `clang-format`, `ktlint`, `ruff`, `air`, `uv`, `rubocop`, `standardrb`, `htmlbeautifier`, `dart`, `ocamlformat`, `terraform`, `latexindent`, `gleam`, `shfmt`, `nixfmt`, `rustfmt`, `pint`, `ormolu`, `cljfmt`, `dfmt`. Formatters are selected by name, not by extension; each built-in formatter declares its own extensions, and an `extensions` override replaces the built-in list. `command` overrides discovery entirely; `environment` merges with the built-in environment (override wins). `$FILE` is substituted with the target file path. When several formatters match a file, they run sequentially in built-in registry declaration order, followed by any custom override-only formatters; failures surface as non-fatal warnings. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `lsp` | Upstream-compatible LSP setting; `false` disables harness LSP overrides, object values map to harness LSP servers when possible. |
| `mcp` | MCP server definitions keyed by server name. |
| `mode` | Deprecated upstream alias for `agent`; entries are translated as agent definitions. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback metadata; runtime profile resolution selects the primary target in V1. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. |

| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime knobs that are not provider/model/agent definitions, currently including provider-context compaction settings and provider retry policy. |
| `server` | Upstream server configuration; accepted only when empty because server commands are outside this runtime config. |

| `small_model` | Optional smaller model reference for custom secondary profiles. |
| `skills` | Shared skill discovery roots and permission overrides for skill loading. |

## Variable substitution

The harness resolves variable references in config values before parsing. A
single pass is applied to all config values via
`resolve_config_value_references_with_lookup()`. Nested references (e.g.,
`${VAR:-${OTHER}}`) are NOT expanded recursively — only one level of
substitution is performed.

| Syntax | Behavior |
| --- | --- |
| `{env:VAR}` | Environment variable substitution. Returns an empty string if `VAR` is missing from the environment. |
| `{file:path}` | File content substitution. The path is resolved relative to the config file directory; absolute paths are used as-is. |
| `${VAR}` | Shell-style environment variable. If `VAR` is missing from the environment, this produces a config error rather than expanding to an empty string. Use `${VAR:-}` for an explicit empty fallback. |
| `${VAR:-fallback}` | Environment variable with fallback value. If `VAR` is missing or empty, `fallback` is used. |

Note: `apiKeyEnv` in provider config is a separate mechanism (multi-env
fallback chain with credential redaction) — it is NOT the same as `{env:VAR}`.

## Config layering

The harness discovers and merges config from multiple sources. Later layers
override earlier ones, so project-local settings take precedence over global
defaults.

### Discovery order

1. **XDG global config** (`$XDG_CONFIG_HOME/harness/harness.jsonc`, fallback:
   `~/.config/harness/harness.jsonc`) — shared defaults across projects.
2. **Project local config** (`./harness.jsonc` or `./harness.json`) —
   project-specific overrides.
3. **Agent markdown files** (`.agent-harness/agents/*.md`) — agent definitions
   with JSON5 frontmatter. Frontmatter fields take effect when no JSON config
   override exists for the same field.

### Merge precedence

- Project local config overrides XDG global config.
- Agent markdown frontmatter overrides the JSON config `agent` section for
  fields that are not explicitly set in JSON config (empty or default values
  fall back to markdown).
- Markdown agent discovery is last-wins: project-level markdown files override
  shipped agents with the same name.

## Extension manifest descriptors

Typed extension manifests are not a runtime config key in V1. The descriptor
schema lives at
[`configs/extension-manifest.v1.schema.json`](../configs/extension-manifest.v1.schema.json)
and is validated by `harness-core::extension_manifest::ExtensionManifestV1`.
The seam is descriptor-only: parsing a manifest records stable extension ids,
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

## TUI default bindings

`tui.json{,c}` `keybinds` overrides use the action ids below. The table records
the primary shipped binding for each default-bound action; some surfaces also
keep secondary aliases for compatibility.

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
| `toggle_operator_sidebar` | `Ctrl+x b` | Show or hide the operator sidebar/drawer. |
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
| `deny_permission` | `Ctrl+n` | Deny a pending permission request. |
| `dismiss_modal` | `Esc` | Dismiss or reject the active modal. |
| `variant_cycle` | `Ctrl+t` | Cycle the active model variant/reasoning preset. |

TUI prompt history is runtime state, not config. Interactive startup and live
sessions load and append prompt history at `<session-dir>/tui/prompt-history.json`
using a versioned JSON schema, so submitted prompts survive process restarts while
unsent drafts stay in the active composer until submitted or discarded.

## Discovery and precedence

Runtime config discovery uses these layers, merged from lowest precedence to highest:

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

The runtime config stays focused on provider/model/agent selection. Prompt prose
and repository instructions still come from files:

1. inline `agent.<name>.system_prompt` / `agent.<name>.prompt`
2. discovered `.agent-harness/agents/<name>.md`
3. markdown frontmatter `system_prompt` / `prompt` in `.agent-harness/agents/<name>.md`

Project instructions are still auto-discovered from `AGENTS.md`. If
`instructions` is set in the runtime config, those entries are prepended ahead
of the discovered `AGENTS.md` content.

Provider requests keep the normal role boundary: the composed system prompt is
sent before the live user message, and child-task delegation context is embedded
inside the child user prompt before the task body. Within the composed Harness
system prompt, V1 precedence is fixed and tested as:

1. runtime agent prompt from config or `.agent-harness/agents/<name>.md`
2. generated environment/model context
3. task-delegation reminder
4. configured `instructions` entries, followed by discovered `AGENTS.md`
5. skill-tool guidance when the profile exposes `skill`

When `task` loads skills for a child session, the child user prompt starts with
delegation context, then loaded skill content, then optional command context, and
finally the requested task body.

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
cannot be activated through either `skill` or `task(load_skills = [...])`.
`urls` is accepted as inert/deferred metadata only; V1 discovery never fetches
remote skills.

A skill directory must contain `SKILL.md` with V1 frontmatter:

```markdown
---
name: rust-best-practices
description: Baseline Rust guidance for this workspace.
argument_hint: optional short usage hint
allowed_tools: read, grep
target_agent: build
target_category: deep
mcp: deferred-local-metadata
resources: bundled-reference-not-loaded
---

# Skill body
```

Required fields are `name` and `description`. `name` must match the directory
name and `^[a-z0-9]+(-[a-z0-9]+)*$`; `description` must be 1-1024 characters.
Optional V1 fields are `argument_hint` / `argumentHint`, `allowed_tools` /
`allowedTools` / `expected_tools` / `expectedTools`, `target_agent` /
`targetAgent`, `target_category` / `targetCategory`, `mcp` / `deferred_mcp` /
`deferredMcp`, `resources` / `deferred_resources` / `deferredResources`, and a
string-to-string `metadata` map. `license` and `compatibility` are accepted as
non-runtime metadata. Unsupported public fields make that skill `malformed`
without hiding other valid skills in the same catalog.

Catalog-time metadata includes stable id, name, description, source scope, root
path, file location, loadability, permission mode, status, optional V1 metadata,
`body_loaded: false`, and no full `SKILL.md` body. Full bodies are loaded only
when the `skill` tool activates a loadable skill or `task(load_skills = [...])`
resolves loadable skills before child spawn. Missing, denied, disabled,
malformed, and symlink-unsafe skills fail before activation or child spawn.

`allowed_tools` and related skill metadata are descriptive/restrictive contract
metadata only. They never grant runtime tools, override a profile toolset, or
bypass coordinator permission checks. Doctor JSON and support exports consume the
same compact catalog metadata, report loadable/denied/disabled/malformed/shadowed
counts, and keep full skill bodies out of readiness surfaces.

The shipped `plan` agent provides a stable planning mode, not an experimental
feature flag. It can read/search, ask questions, write only the active
workspace-relative `.agent-harness/plans/<run>.md` plan file, and call
`plan_exit` to ask whether to switch to `build`. The coordinator reminder tells
Plan whether that active plan file already exists: the first Plan turn creates
the file, while later turns should read and update the same path. The edit
boundary is enforced by per-agent permission rules, not just prompt text.

The shipped `build` agent exposes `plan_enter`, which asks whether to switch to
Plan before complex implementation work and schedules a coordinator-owned Plan
continuation when approved. To match the reference Plan workflow, the shipped
Plan profile exposes `bash` behind shell permission prompts; Plan instructions
and a coordinator-side shell guard still restrict bash to read-only inspection and
forbid edits, config changes, commits, or other mutations. Plan-mode delegation
remains restricted to the read-only `explore` profile by default; `general` and
user-defined write-capable subagents are rejected before spawn unless a future
profile deliberately adds parent-permission inheritance and tests for it.

selects an optional write-capable lead profile; when present, the coordinator
`role: "member"`; set `role: "research"` only for read-only profiles such as

`harness doctor` validates the operator-facing orchestration surface without
making provider or MCP network calls. It checks provider/model metadata,
provider credential availability without printing key values, configured agent
and model-profile references, shipped workflow profile availability, category
route coverage, profile tool ids, permissions, skill roots and permission
posture, session-directory readiness, and configured MCP server state.
Use `--json` for machine-readable output.

### Plan operator workflow

Use Plan when the operator wants a reviewed implementation plan before changing
project files. Harness ships Plan as a stable public runtime surface, not an
experimental compatibility flag, and the safety boundary is enforced by
coordinator permissions as well as prompt instructions.

1. Start in the primary `build` agent for normal implementation work.
2. Switch to the primary `plan` agent with the TUI primary-agent switcher, or let
   Build call `plan_enter` and approve the coordinator-owned switch when the work
   is complex enough to plan first.
3. Let Plan inspect the workspace with read/search/LSP tools and, when useful,
   delegate read-only codebase research only to `explore`. Plan cannot launch
   `general`, `build`, or user-defined writer subagents under the shipped policy.
4. Let Plan create or update only the active plan file at
   `.agent-harness/plans/<run>.md`. The first Plan turn is expected to create this
   file; later Plan turns should read and refine the same file after operator
   feedback or clarifying answers.
5. Review the plan file. If Plan needs information that read-only exploration
   cannot determine, answer its clarifying question and let it update the plan.
6. When the plan is ready, Plan calls `plan_exit`. Approving that prompt switches
   back to Build with the approved plan-file path in the continuation prompt;
   declining leaves the session in Plan so the plan can be revised further.

This differs intentionally from broader experimental Plan-style behavior:
Harness keeps `plan_exit` available in the shipped `plan` profile and keeps
Plan-spawned child work restricted to `explore` unless a future policy adds
tested parent-permission inheritance for write-capable subagents.

The shipped agent names are available without extra config: primary
`build` and `plan`, subagents `general`, `explore`,
`visual-engineering`, `artistry`, `ultrabrain`, `deep`, `quick`,
`unspecified-low`, `unspecified-high`, and `writing`, plus hidden `title`,
`summary`, and `compaction` profiles. `explore` is a read-only local codebase
search profile for `task(subagent_type: "explore")`. `general` is a broader
focused implementation/research profile for `task(subagent_type: "general")`.
The category profiles are category-based routing lanes for `task(category: "...")`:
the task tool selects the matching profile first and falls back to `general` only
when no matching category profile is configured. `visual-engineering` covers UI,
UX, layout, styling, animation, and design; `artistry` covers complex creative
problem-solving; `ultrabrain` covers hard logic, architecture, algorithms, and
deep debugging; `deep` covers autonomous research and end-to-end implementation;
`quick` covers small low-risk changes; `unspecified-low` and `unspecified-high`
cover uncategorized low-to-moderate and high-effort work; and `writing` covers
docs and prose. Shipped subagents intentionally omit or deny `task` by default so
they do not recursively redelegate unless a project opts into that tool. Named
category model profiles preserve category scale as primary targets plus
validated fallback metadata; automatic provider/model retry is not V1 runtime
behavior.
When a subagent profile does not configure its own `model`, task delegation
inherits the invoking parent turn's active model and model settings. If the
subagent profile has an explicit `model`, that configured model wins. The `task`
tool requires `run_in_background` and `load_skills` on every call; pass
`load_skills: []` when no skill context is needed. Listed skills are resolved in
request order before the child is spawned; duplicate names are loaded once at the
first occurrence. Missing, denied, disabled, malformed, or symlink-unsafe skills
fail the call before child spawn. Loaded skill content is injected into the child
prompt before optional command context and before the original task body, while
task output reports compact loaded-skill metadata without the full bodies.
`task(run_in_background: true)` returns a child `request_id`; use the
`background_output` tool with that `request_id` to inspect completion status or
the terminal result. Retrieval is event-replay based and does not advance the
child task. To stop an authorized non-terminal child request, call
`background_cancel` with the same `request_id` and an optional `reason`; the
coordinator records cancellation through the normal task lifecycle.
`background_output(cancel: true)` remains supported as compatibility.
Task and background-output results also include child runtime metadata such as
profile, category, model ref, toolset, redelegation capability, and exact
follow-up tool actions for status checks, waiting, cancellation, or continuation.

Agent `model` selects a provider/model target for that profile. `prompt` is the
public prompt alias for `system_prompt`. `tools` accepts either a list of tool ids
or a map of `{ tool_id: enabled }`; disabled map entries are omitted. `mode` may
be `primary`, `subagent`, or `all`; the default agent must not be `subagent`-only
or `hidden`. Agent `max_iters` / `maxIters` / `steps` / `maxSteps` is optional.
When unset, the runtime does not add a profile-specific iteration cap; the agent
continues until the model stops, the user interrupts, or another runtime safety
limit applies. Set an iteration cap only when a profile needs an explicit
per-turn budget. `name`, `top_p` / `topP`, `color`, and `options` are accepted as
agent metadata for consumers that need them. `enable: false` / `enabled: false`
or `disable: true` removes a configured or shipped agent from the resolved
runtime config; `enable: true` documents that a shipped default remains active.

## Permission policy

The canonical scalar form is:

```jsonc
{ "permission": "ask" }
```

`permission` accepts exactly `"ask"`, `"allow"`, or `"deny"`. A scalar applies to
all canonical public permission kinds: `bash`, `edit`, `question`, `task`,
`webfetch`, `websearch`, `codesearch`, and `lsp`.

The V1 native tool catalog is documented in
[`docs/native-tool-catalog.md`](native-tool-catalog.md). New control-plane tools
`task`; `ast_grep_search` uses `codesearch`; `ast_grep_replace` uses `edit`; `session_list`, `session_read`,
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

`bash`, `edit`, and `task` also support bounded selector maps. They are not a general
policy language:

```jsonc
{
  "permission": {
    "bash": {
      "git status": "allow",
      "cargo nextest run*": "ask",
      "*": "deny"
    },
    "edit": {
      "docs/**": "allow",
      "crates/harness-core/src/config.rs": "ask",
      "*": "deny"
    },
    "task": {
      "explore": "allow",
      "review-*": "ask",
      "*": "deny"
    }
  }
}
```

Bash selectors are either an exact command string, a trailing `*` prefix such as
`cargo nextest run*`, or the `*` catch-all. Edit selectors are either an exact
workspace-relative path, a trailing `/**` path prefix such as `docs/**`, or the
`*` catch-all. Task selectors match the requested subagent/profile/category name;
they accept exact names, `*` catch-all, and simple `*` glob patterns such as
`review-*`. Regex is not supported.

`shell_allowlist` remains supported inside `permission` for shell policy inputs.
It accepts `mode` values `permission_patterns` (the default) and
`legacy_executables`, plus the compatibility aliases `policy_mode` and
`policyMode`. Existing `executables` and `cwd_roots` entries still load, and
`cwdRoots` remains accepted as an alias for `cwd_roots`. In
`permission_patterns` mode, Harness still blocks environment-dump commands and
interpreter eval flags such as `python3 -c` before execution. Permission
decisions improve operator UX by deciding whether a tool call runs, asks, or is
denied. They are not a sandbox or security boundary.

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

Provider-context compaction uses the active profile/model limits when available,
especially:

- `context_window_tokens`
- `max_input_tokens`
- `max_output_tokens`

Model variants may also set `context_window_tokens`, `max_input_tokens`, and
`max_output_tokens`. Variant values override the base model metadata for picker
labels and compaction estimates, which lets one provider model expose multiple
operator-facing presets such as an extended-context GPT profile while
still using the same underlying provider model id.

The coordinator uses those values to decide when proactive compaction should summarize older provider-visible history and how much recent context to preserve verbatim. The preserved tail is governed by `keep_recent_tokens` and always keeps at least the latest complete turn when possible.

Public compaction knobs live under `runtime.compaction`:

| Key | Default | Purpose |
| --- | --- | --- |
| `enabled` | `true` | Master switch for proactive, pre-prompt, overflow-retry, and manual compaction. When `false`, all compaction paths become no-ops. |
| `reserveTokens` / `reserve_tokens` | `16384` | Safety margin subtracted from the usable context window before compaction is considered. |
| `keepRecentTokens` / `keep_recent_tokens` | `20000` | Target number of recent tokens to preserve verbatim after compaction. The latest complete turn is always preserved. |
| `splitOversizedTurns` / `split_oversized_turns` | `false` | Allows overflow compaction to split an oversized latest turn, summarizing the earlier portion while preserving a suffix as recent provider context. |
| `autoRetryOverflow` / `auto_retry_overflow` | `true` | Enables the one-shot overflow compaction retry after a provider context-window error. Set `false` to fail immediately. |
| `structuredSummaryContract` / `structured_summary_contract` | `true` | Requires summaries to carry the Harness sections `Goal`, `Constraints`, `Progress`, `Key Decisions`, `Next Steps`, and `Critical Context`. Set `false` only for legacy heading compatibility. |
| `estimatedTokenTriggers` / `estimated_token_triggers` | `true` | Allows proactive and pre-prompt compaction to use deterministic context estimates when provider usage or model metadata is absent. |
| `fallbackInputTokens` / `fallback_input_tokens` | `32768` | Input budget used for estimated trigger checks when the active model does not publish a context window or max input token limit. |

On successful compaction, the coordinator appends a single `SessionCompaction` event to the event log and updates the in-memory provider context. The event carries the generated summary, token estimate before compaction, the sequence number of the first preserved event, replay-derived read/modified file lists, the trigger reason, and hook provenance. No separate checkpoint artifact is written; the summary lives entirely in the event and the in-memory `ProviderContext`. Resume reconstructs provider context from the latest `SessionCompaction` event for the agent, then replays post-compaction deltas from `events.jsonl`; the event log itself stays append-only.

Manual `/compact` summarizes older completed turns now, preserves the latest completed turn verbatim, and appends a `SessionCompaction` event. The success notice reports the active-context estimate delta when available, or says the estimate was unchanged. The default summary contract uses the Harness sections for goal, constraints, progress, key decisions, next steps, and critical context, with operational memory and source facts added as replay-derived context; it is still lossy. Sessions with only one completed turn no-op because there is no older turn to summarize.

Lifecycle hooks may use `event = "compaction_requested"` to observe or cancel compaction. A critical hook failure cancels compaction and records `CompactionFailed` (deprecated; replaced by `SessionCompaction`). A successful hook can replace the summary by emitting output prefixed with `compaction_summary:`; hook overrides take precedence over the deterministic structured summary.

Overflow retry is related but distinct: if the provider rejects a request for context-window reasons, the coordinator may compact and retry once when the retry can prove it shrank the provider-visible payload. Estimated pre-prompt compaction uses the same `SessionCompaction` path before provider request construction. If a pre-prompt compaction cannot reduce the estimated active context, the coordinator records the failure and does not loop on the same turn.

Failed or aborted provider turns can be preserved in active context. Replay/debug projections keep the incomplete marker, failure stage, and redacted reason so a future provider call does not treat partial assistant output as a completed answer.

Operational memory is derived from persisted events, not from live filesystem scans. The `SessionCompaction` event records capped read-file and modified-file lists, and replay projections expose these facts so operators can see what context survived compaction.

TUI memory or transcript caps are separate presentation settings. They affect what the operator sees on screen, not the persisted provider context used for resume or overflow-retry compaction. The TUI distinguishes active context estimate from cumulative provider tokens spent: active context may decrease after `SessionCompaction`, while total spend remains cumulative and never decreases.

## Provider retry policy

Provider-request retries are bounded and automatic only for transient provider-side failures (`TransportFailure` and `RateLimited`). Retries happen before the provider response is committed to the session as a completed assistant turn. Each retry issues a fresh provider request id and records the attempt in `ProviderRequestStartedMetadata.retry`. To avoid masking cancellation, an operator or coordinator cancellation attempt wins over an in-flight retry and short-circuits the backoff.

Public retry knobs live under `runtime.provider_retry`:

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
