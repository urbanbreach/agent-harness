# Harness configuration reference

Agent harness loads JSON5/JSONC config files from `harness.jsonc`, an explicit `--config` path, or `$XDG_CONFIG_HOME/harness/config.jsonc`.
Use `harness config validate` to check a file and `harness schema` to print the machine-readable `HarnessConfig` JSON schema.

## Top-level keys

`HarnessConfig` accepts the following public top-level keys:

| Key | Required | Type | Purpose |
| --- | --- | --- | --- |
| `$schema` | No | `string` | Optional schema URI for editor tooling. |
| `providers` | Yes | `object` | Model provider definitions and model catalogs. |
| `profiles` | Yes | `object` | Named agent profiles that reference providers/models and tools. |
| `permissions` | Yes | `object` | Default permission policy and shell allowlist. |
| `runtime` | Yes | `object` | Session paths, background-task limits, prompt timeouts, and deterministic mode. |
| `integrations` | Yes | `object` | External integration settings such as remote search and MCP servers. |
| `hooks` | No | `object` | Lifecycle shell hooks that run around coordinator events. |
| `skills` | No | `object` | Skill discovery roots and per-skill permission overrides. |
| `lsp` | No | `object` | Language-server enablement and per-server overrides. |
| `ui` | No | `object` | Interactive UI defaults, parity toggles, and keybindings. |
| `logging` | No | `object` | Log level and optional log file destination. |

## `HarnessConfig` schema reference

| Field | Schema | Notes |
| --- | --- | --- |
| `$schema` | `Option<String>` | Passed through for schema-aware editors. |
| `providers` | `BTreeMap<String, ProviderConfig>` | Required. Currently supports `openai_compatible` providers. |
| `profiles` | `BTreeMap<String, ProfileConfig>` | Required. Replaces the retired `categories` key. |
| `permissions` | `PermissionsConfig` | Required. Defines defaults plus `shell_allowlist`. |
| `runtime` | `RuntimeConfig` | Required. Holds session-dir, prompt, and background-task settings. |
| `integrations` | `IntegrationsConfig` | Required. Holds remote-search and MCP settings. |
| `hooks` | `HooksConfig` | Optional. Defaults to `{ "lifecycle": [] }`. |
| `skills` | `SkillsConfig` | Optional. Defaults to the built-in project/global skill roots and permission map. |
| `lsp` | `LspConfig` | Optional. Defaults to `{ "disabled": false, "servers": {} }`. |
| `ui` | `UiConfig` | Optional. UI defaults and parity keybindings. |
| `logging` | `LoggingConfig` | Optional. Defaults to `level = "info"`. |

## `providers`

`providers` is the public model-catalog surface. Each provider entry defines the transport plus the
set of model ids that profiles are allowed to reference.

### `providers.<name>`

| Field | Type | Notes |
| --- | --- | --- |
| `type` | `"openai_compatible"` | Current shipped provider type. |
| `base_url` / `baseUrl` | `string` | OpenAI-compatible endpoint. |
| `api_key` / `apiKey` | `string` | API key or `${ENV_VAR}` substitution. |
| `api_mode` / `apiMode` | `"auto" \| "responses" \| "chat_completions"` | Transport mode. |
| `timeout_ms` / `timeoutMs` | `integer` | Request timeout in milliseconds. |
| `headers` | `object<string, string>` | Optional extra HTTP headers. |
| `models` | `object<string, ModelConfig>` | Public model catalog for this provider. |

### `providers.<name>.models.<model>`

Model entries are keyed by the exact provider model id used at request time.

| Field | Type | Notes |
| --- | --- | --- |
| `display_name` / `displayName` | `string` | Friendly label shown in UI metadata. |
| `metadata` | `object` | Optional model metadata such as family and context window. |
| `max_input_tokens` / `maxInputTokens` | `integer \| null` | Optional input token cap. |
| `max_output_tokens` / `maxOutputTokens` | `integer \| null` | Optional output token cap. |
| `variants` | `object<string, ModelVariantConfig>` | Optional named presets layered onto the base model. |

### Model-selection example

The supported issue-#81 path is:

1. declare the allowed models under `providers.<provider>.models`
2. point a profile at one of them with `profiles.<name>.model_ref`
3. choose the default interactive profile with `ui.default_profile` or override it per run with `--profile`

```jsonc
{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "${OPENAI_API_KEY:-sk-zerolimit}",
      "models": {
        "gpt-5.4-mini": {
          "display_name": "GPT-5.4 Mini"
        },
        "gpt-5.4": {
          "display_name": "GPT-5.4"
        }
      }
    }
  },
  "profiles": {
    "plan": {
      "description": "Primary planning lane",
      "model_ref": "default:gpt-5.4-mini",
      "tools": []
    },
    "build": {
      "description": "Higher-capacity implementation lane",
      "model_ref": "default:gpt-5.4",
      "tools": []
    }
  },
  "ui": {
    "default_profile": "plan"
  }
}
```

With that config, `harness prompt --profile build --text "..."` uses `gpt-5.4`, while the blessed
default `plan -> build` path can keep `plan` on `gpt-5.4-mini`.

Broken references fail early during `harness config validate`; profiles cannot point at a provider
or model id that is missing from the configured catalog.

## `profiles`

`profiles` is the public agent-profile surface. Each profile names its tool lane and selects one
configured provider/model pair through `model_ref`.

### `profiles.<name>`

| Field | Type | Notes |
| --- | --- | --- |
| `description` | `string` | Required profile summary. |
| `system_prompt` / `systemPrompt` | `string \| null` | Optional custom system prompt. |
| `model_ref` / `modelRef` | `string` | Required `<provider>:<model>` reference into `providers.*.models`. |
| `variant` | `string \| null` | Optional named preset under the selected model. |
| `temperature` | `number \| null` | Optional provider temperature override. |
| `permissions` | `object \| null` | Optional per-profile permission overrides. |
| `tool_surface` / `toolSurface` | `"native" \| "compat"` | Tool-id surface for the profile. |
| `max_iters` / `maxIters` | `integer` | Per-profile iteration cap. |
| `tool_failure_mode` / `toolFailureMode` | enum | Runtime behavior for tool failures. |
| `plan_mode` / `planMode` | `bool` | Marks the profile as a planning lane. |
| `exit_target_profile` / `exitTargetProfile` | `string \| null` | Optional handoff profile for `plan.exit`. |
| `tools` | `array<string>` | Allowed tool ids for the profile. |

## `integrations`

`integrations` currently exposes the built-in remote-search bridge plus configured MCP servers.

### `integrations.remote_search`

`remote_search` configures the current runtime bridge used by native search tools.

| Field | Default | Meaning |
| --- | --- | --- |
| `endpoint` | `https://mcp.exa.ai/mcp` | MCP endpoint used for built-in remote search requests |
| `auth_token` | `null` | Optional bearer token sent to the endpoint |
| `require_auth` | `false` | Fail fast when no auth token is configured |
| `timeout_secs` | `30` | Per-request timeout |
| `max_retries` | `1` | Retry count for retryable failures |
| `retry_backoff_ms` | `250` | Delay between retries |

Environment overrides are also supported:

- `HARNESS_REMOTE_SEARCH_ENDPOINT` or `HARNESS_EXA_MCP_ENDPOINT`
- `HARNESS_REMOTE_SEARCH_AUTH_TOKEN`, `HARNESS_EXA_MCP_AUTH_TOKEN`, or `EXA_API_KEY`
- `HARNESS_REMOTE_SEARCH_REQUIRE_AUTH`
- `HARNESS_REMOTE_SEARCH_TIMEOUT_SECS`
- `HARNESS_REMOTE_SEARCH_MAX_RETRIES`
- `HARNESS_REMOTE_SEARCH_RETRY_BACKOFF_MS`

### `integrations.mcp`

`integrations.mcp.servers` registers configured MCP servers. Each server entry may use stdio or HTTP transport and is validated by the generated schema.

## `hooks`

`hooks.lifecycle` is an ordered list of lifecycle commands. Each entry supports:

- `id` / `name`: optional stable identifier
- `event`: required lifecycle event name
- `command`: required command array
- `cwd`: optional workspace-relative working directory
- `timeout_ms` / `timeoutMs`: optional timeout in milliseconds, default `5000`
- `critical`: optional boolean, default `false`
- `env`: optional string map

Supported `event` values:
`run_started`, `run_finished`, `run_failed`, `agent_turn_started`, `agent_turn_finished`, `tool_call_started`, `tool_call_finished`, `provider_request_started`, `provider_request_finished`, `subagent_spawned`, `subagent_finished`, `permission_requested`, `permission_resolved`.

### Compact hooks example

```jsonc
{
  "hooks": {
    "lifecycle": [
      {
        "id": "announce-run-start",
        "event": "run_started",
        "command": ["bash", "-lc", "printf 'run started\\n'"],
        "timeout_ms": 4000,
        "critical": false,
        "env": {
          "HARNESS_HOOK_SOURCE": "docs"
        }
      }
    ]
  }
}
```

## `skills`

`skills` controls where the runtime looks for installed skills and how permission overrides are applied.

| Field | Type | Default |
| --- | --- | --- |
| `project_roots` / `projectRoots` | `array<string>` | `[".opencode/skills", ".claude/skills", ".agents/skills"]` |
| `global_roots` / `globalRoots` | `array<string>` | `["~/.config/opencode/skills", "~/.claude/skills", "~/.agents/skills"]` |
| `walk_to_git_root` / `walkToGitRoot` | `bool` | `true` |
| `permissions` | `object<string, PermissionMode>` | `{ "*": "allow", "experimental-*": "ask", "internal-*": "deny" }` |

### Compact skills example

```jsonc
{
  "skills": {
    "project_roots": [".opencode/skills", ".agents/skills"],
    "global_roots": ["~/.config/opencode/skills"],
    "walk_to_git_root": true,
    "permissions": {
      "*": "allow",
      "experimental-*": "ask",
      "internal-*": "deny"
    }
  }
}
```

## `lsp`

`lsp` configures language-server integration.

| Field | Type | Notes |
| --- | --- | --- |
| `disabled` | `bool` | Disables all LSP integration when `true`. |
| `servers` | `object<string, LspServerConfig>` | Per-server overrides keyed by server id. |

Each `LspServerConfig` supports `disabled`, `command`, `extensions`, `env`, and `initialization`.
Built-in server ids are `rust` and `typescript`. Custom local servers must provide both `command` and `extensions`.

### Compact LSP example

```jsonc
{
  "lsp": {
    "disabled": false,
    "servers": {
      "rust": {
        "disabled": false,
        "command": ["rust-analyzer"],
        "extensions": [".rs"],
        "env": {
          "RUST_LOG": "warn"
        },
        "initialization": {
          "cargo": {
            "allFeatures": true
          }
        }
      },
      "custom-local": {
        "command": ["custom-local-lsp", "--stdio"],
        "extensions": [".foo", ".bar"],
        "initialization": {
          "feature": {
            "mode": "custom"
          }
        }
      }
    }
  }
}
```

## Drift check

The runtime schema is generated from `crates/harness-core/src/config.rs`.
When you update the public contract, compare this document against `harness schema` output so the prose and generated schema stay aligned.
