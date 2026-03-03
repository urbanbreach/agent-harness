# Configuration

Agent Harness uses JSONC (JSON with comments) for configuration.

## Config Locations

Config files are resolved in this order (first match wins):

1. **Command line**: `--config /path/to/config.jsonc`
2. **Project local**: `./harness.jsonc` (in current working directory)
3. **User config**: `$XDG_CONFIG_HOME/harness/config.jsonc`
4. **Fallback**: `~/.config/harness/config.jsonc`

## Generating Schema

Print the full JSON Schema:

```bash
cargo run -p harness -- schema > harness.schema.json
```

Validate a config file:

```bash
cargo run -p harness -- config validate --config my-config.jsonc
```

## Key Options

### Background Task Scheduler

```jsonc
{
  "backgroundTask": {
    "defaultConcurrency": 4,        // Max concurrent tasks globally
    "providerConcurrency": 4,       // Max concurrent provider calls
    "modelConcurrency": 2,          // Max concurrent calls per model
    "staleTimeoutMs": 30000,        // Timeout before task marked stale
    "messageStalenessTimeoutMs": 10000  // Timeout for message responses
  }
}
```

### Providers

Only `openai_compatible` type is supported:

```jsonc
{
  "providers": {
    "default": {
      "type": "openai_compatible",
      
      // CLIProxy-style base URL (no trailing slash)
      "base_url": "http://127.0.0.1:8317/v1",
      
      // Literal key or ${ENV_VAR} reference
      "api_key": "${OPENAI_API_KEY}",
      
      "timeout_ms": 60000,
      
      // Additional headers for all requests
      "headers": {
        "X-Client": "harness"
      },
      
      // Model definitions
      "models": {
        "gpt-4o-mini": {
          "display_name": "GPT-4o mini",
          "max_input_tokens": 128000,
          "max_output_tokens": 16384,
          "variants": {
            "deterministic": {
              "display_name": "Deterministic",
              "max_output_tokens": 4096
            }
          }
        }
      }
    }
  }
}
```

### Categories

Define agent profiles with model refs and permissions:

```jsonc
{
  "categories": {
    "deep": {
      "description": "Default deep execution profile",
      "model_ref": "default:gpt-4o-mini",  // provider:model
      "variant": "deterministic",           // optional model variant
      "temperature": 0.1,
      "permissions": {
        "edit": "ask",      // ask | allow | deny
        "shell": "ask",
        "network": "deny"
      },
      "tools": ["read", "edit.hashline_apply", "shell.run"]
    },
    
    "quick": {
      "description": "Fast responses, no tools",
      "model_ref": "default:gpt-4o-mini",
      "temperature": 0.7,
      "permissions": {
        "edit": "deny",
        "shell": "deny",
        "network": "deny"
      },
      "tools": []
    }
  }
}
```

### Global Permissions

Default permission policies and shell allowlist:

```jsonc
{
  "permissions": {
    "edit": "ask",        // Default for all edit operations
    "shell": "ask",       // Default for shell executions
    "network": "deny",    // Default for network operations
    
    "shell_allowlist": {
      // Allowed executables
      "executables": ["git", "cargo", "ls", "grep", "find"],
      
      // Allowed working directory roots
      "cwd_roots": [".", "./crates", "/tmp"]
    }
  }
}
```

### Paths

```jsonc
{
  "paths": {
    // Session storage directory (default: .agent-harness/sessions)
    "session_dir": ".agent-harness/sessions"
  }
}
```

### Deterministic Mode

For testing and reproducible runs:

```jsonc
{
  "deterministic": {
    "enabled": false,     // Set to true for deterministic runs
    "seed": 42           // Seed for deterministic ID generation
  }
}
```

## Complete Example

See [configs/harness.example.jsonc](../configs/harness.example.jsonc) for a fully annotated example.

## Environment Variable Substitution

String values can reference environment variables:

```jsonc
{
  "providers": {
    "default": {
      "api_key": "${OPENAI_API_KEY}",
      "base_url": "${HARNESS_BASE_URL:-http://localhost:8317/v1}"
    }
  }
}
```

- `${VAR}` - Required, fails if not set
- `${VAR:-default}` - Optional, uses default if not set

## CLI Overrides

Some config options can be overridden via CLI flags:

```bash
# Override session directory
harness run --session-dir /tmp/test-sessions

# Override config path
harness tui --config /path/to/alternate.jsonc
```

## JSON Schema Reference

The schema is generated from `HarnessConfig` in `harness-core`. Key types:

### HarnessConfig

Root configuration object:

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| backgroundTask | BackgroundTaskConfig | Yes | Scheduler settings |
| providers | Map<String, ProviderConfig> | Yes | Provider definitions |
| categories | Map<String, CategoryConfig> | Yes | Agent category profiles |
| permissions | PermissionsConfig | Yes | Global permission defaults |
| paths | PathsConfig | No | Path overrides |
| deterministic | DeterministicConfig | No | Determinism settings |

### PermissionLevel

Enum values:
- `"allow"` - Automatically permit the operation
- `"deny"` - Automatically reject the operation
- `"ask"` - Prompt user for permission (TUI) or default-deny (headless)

### ProviderConfig

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| type | String | Yes | Only "openai_compatible" supported |
| base_url | String | Yes | API endpoint URL |
| api_key | String | Yes | API key or ${ENV_VAR} |
| timeout_ms | u64 | Yes | Request timeout |
| headers | Map<String, String> | No | Extra HTTP headers |
| models | Map<String, ModelConfig> | Yes | Model definitions |

## Validation Errors

Common validation issues:

```
error: missing field `backgroundTask`
  --> harness.jsonc:1:1
```

```
error: unknown variant `openai`, expected `openai_compatible`
  --> harness.jsonc:5:15
```

```
error: environment variable `OPENAI_API_KEY` not set
  --> harness.jsonc:8:20
```

## Migration Notes

Agent Harness does **not** auto-import OpenCode configuration. You must create a harness-specific config file. Key differences from OpenCode:

- Different schema structure
- No auto-discovery of models
- Explicit category definitions required
- Hashline edits instead of standard file edits
