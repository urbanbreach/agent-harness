# Config reference

This document tracks the top-level `HarnessConfig` surface that ships with the
harness. The generated JSON schema is the source of truth; this file mirrors the
same keys so config docs stay in lock-step with validation.

## Top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `agent` | Legacy single-profile map accepted alongside `agents`. |
| `agents` | Canonical agent/profile definitions. |
| `default_agent` | Default profile selected for interactive startup. |
| `hooks` | Lifecycle hook definitions. |
| `integrations` | External integration settings such as MCP and remote search. |
| `logging` | Logging level and optional log file override. |
| `lsp` | Language-server configuration. |
| `permissions` | Permission defaults and shell allowlist policy. |
| `providers` | Model/provider definitions. |
| `runtime` | Session/runtime behavior settings. |
| `skills` | Skill discovery roots and permissions. |
| `ui` | Interactive UI defaults and keybindings. |

## `HarnessConfig` schema reference

| Key | Notes |
| --- | --- |
| `$schema` | Optional string. |
| `agent` | Object keyed by profile name. |
| `agents` | Object keyed by profile name. |
| `default_agent` | Optional string. |
| `hooks` | Hook execution policy and lifecycle commands. |
| `integrations` | MCP servers, remote search, and related integrations. |
| `logging` | `level` plus optional `file`. |
| `lsp` | Built-in/custom LSP server configuration. |
| `permissions` | Default permission policy plus shell allowlist. |
| `providers` | Provider transport and model catalog definitions. |
| `runtime` | Background-task, session-dir, and deterministic settings. |
| `skills` | Skill search roots and permission rules. |
| `ui` | UI defaults such as the starting profile. |

## Example

```json
{
  "providers": {},
  "agents": {},
  "permissions": {
    "defaults": {},
    "shell_allowlist": {
      "executables": [],
      "cwd_roots": []
    }
  },
  "runtime": {
    "background_tasks": {
      "default_concurrency": 2,
      "provider_concurrency": 2,
      "model_concurrency": 2,
      "stale_timeout_ms": 30000,
      "message_staleness_timeout_ms": 10000
    },
    "session_dir": "./.harness/sessions",
    "deterministic": {
      "enabled": false,
      "seed": 42
    }
  },
  "integrations": {},
  "hooks": {
    "lifecycle": []
  },
  "skills": {
    "project_roots": [],
    "global_roots": [],
    "permissions": {}
  },
  "lsp": {
    "disabled": false,
    "servers": {}
  },
  "ui": {
    "default_profile": "build"
  },
  "logging": {
    "level": "info"
  }
}
```
