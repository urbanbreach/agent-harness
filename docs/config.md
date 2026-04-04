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
| `integrations` | Yes | `object` | External integration settings such as remote search. |
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
| `integrations` | `IntegrationsConfig` | Required. Holds remote-search settings. |
| `hooks` | `HooksConfig` | Optional. Defaults to `{ "lifecycle": [] }`. |
| `skills` | `SkillsConfig` | Optional. Defaults to the built-in project/global skill roots and permission map. |
| `lsp` | `LspConfig` | Optional. Defaults to `{ "disabled": false, "servers": {} }`. |
| `ui` | `UiConfig` | Optional. UI defaults and parity keybindings. |
| `logging` | `LoggingConfig` | Optional. Defaults to `level = "info"`. |

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
