# Configuration

Agent Harness uses JSONC (JSON with comments) for configuration.

## Config Locations

Config files are resolved in this order (first match wins):

1. **Command line**: `--config /path/to/config.jsonc`
2. **Project local**: `./harness.jsonc` (in current working directory)
3. **User config**: `$XDG_CONFIG_HOME/harness/config.jsonc`
4. **Fallback**: `~/.config/harness/config.jsonc`

A valid config is required for normal interactive operation. Bare `harness` and the compatibility alias `harness tui` both fail closed with setup guidance when no config resolves. Use `--mock` to run with the built-in deterministic demo/mock provider without configuration.

## Launch Modes

| Invocation | Config required? | Behavior |
|---|---:|---|
| `harness` | Yes | Config-backed interactive TUI. If no config resolves, exit with guidance and suggest `--mock`. |
| `harness --config /path/to/config.jsonc` | Yes | Config-backed interactive TUI using the explicit config path. |
| `harness --profile worker` | Yes | Config-backed interactive TUI using the chosen profile override. |
| `harness --mock` | No | Explicit deterministic demo/mock interactive TUI. |
| `harness tui` | Yes | Compatibility alias for the config-backed interactive TUI. |
| `harness tui --mock` | No | Compatibility alias for explicit deterministic demo/mock interactive TUI. |
| `harness tui --scenario golden_path_interactive --deterministic` | No | Deterministic scenario path kept for PTY/live automation. |

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
      // CLIProxy subscription setups commonly use placeholder token fallback.
      "api_key": "${OPENAI_API_KEY:-sk-zerolimit}",

      // API mode: "responses" | "chat_completions" | "auto"
      // "responses" uses /v1/responses (OpenAI Responses API)
      // "chat_completions" uses /v1/chat/completions (standard)
      // "auto" tries responses first, falls back on 404/405
      "api_mode": "responses",

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

### UI Settings

Configure the TUI appearance and behavior:

```jsonc
{
  "ui": {
    // Default profile for interactive TUI mode
    "default_profile": "worker",

    // Theme selection (values depend on theme system)
    "theme": "opencode_dark"
  }
}
```

### Logging Settings

Control log output levels and destinations:

```jsonc
{
  "logging": {
    // Log level: "trace", "debug", "info", "warn", "error"
    "level": "info",

    // Optional log file path (defaults to stderr if not set)
    "file": ".agent-harness/harness.log",

    // Enable span events for async tracing
    "span_events": false
  }
}
```

### Keybindings

Customize keyboard shortcuts (optional):

```jsonc
{
  "ui": {
    "keybindings": {
      "quit": "q",
      "submit": "enter",
      "cancel": "esc",
      "next_tab": "tab",
      "prev_tab": "shift+tab",
      "focus_list": "1",
      "focus_details": "2",
      "focus_prompt": "3",
      "scroll_up": "k",
      "scroll_down": "j"
    }
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
      "api_key": "${REQUIRED_API_KEY}",
      "base_url": "${HARNESS_BASE_URL:-http://localhost:8317/v1}"
    }
  }
}
```

- `${VAR}` - Required, fails if not set
- `${VAR:-default}` - Optional, uses default if not set

When `base_url` targets local CLIProxy loopback (`127.0.0.1:8317` or `localhost:8317`) and `api_key` is `${OPENAI_API_KEY}` with no env value, Agent Harness automatically falls back to `sk-zerolimit` for subscription-backed local proxy setups.

## CLI Overrides

Some config options can be overridden via CLI flags:

```bash
# Override session directory
harness run --session-dir /tmp/test-sessions

# Override config path for the default interactive launch
harness --config /path/to/alternate.jsonc

# Compatibility alias for the interactive TUI
harness --config /path/to/alternate.jsonc tui
```

## Mock Mode

For testing without a real LLM backend, use the `--mock` flag. This bypasses config requirements and uses deterministic mock responses:

```bash
# Launch the interactive TUI with mock provider (no config required)
cargo run -p harness -- --mock

# Compatibility alias for explicit mock mode
cargo run -p harness -- tui --mock

# Run headless scenario with mock provider
cargo run -p harness -- run --scenario golden_path --mock --deterministic

# Single prompt with mock
cargo run -p harness -- prompt --text "Hello" --mock
```

Mock mode is useful for:
- UI development and testing
- CI/CD pipelines
- Deterministic regression testing
- Developing without API keys

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
| ui | UiConfig | No | TUI settings |
| logging | LoggingConfig | No | Logging settings |

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
| api_key | String | Yes | API key or ${ENV_VAR} (for local CLIProxy: use `${OPENAI_API_KEY:-sk-zerolimit}`) |
| api_mode | String | No | API mode: "responses", "chat_completions", or "auto" |
| timeout_ms | u64 | Yes | Request timeout |
| headers | Map<String, String> | No | Extra HTTP headers |
| models | Map<String, ModelConfig> | Yes | Model definitions |

### ApiMode

Enum values for `api_mode`:
- `"responses"` - Use `/v1/responses` endpoint (OpenAI Responses API with streaming)
- `"chat_completions"` - Use `/v1/chat/completions` (standard chat completions API)
- `"auto"` - Try responses first, automatically fall back to chat completions on 404/405 errors

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
error: environment variable `HARNESS_CONFIG_TEST_API_KEY_REQUIRED` not set
  --> harness.jsonc:8:20
```

## CLIproxyAPI Quickstart

Connect to a local CLIproxyAPI instance using the OpenAI Responses API:

```jsonc
{
  "providers": {
    "default": {
      "type": "openai_compatible",
      "base_url": "http://127.0.0.1:8317/v1",
      "api_key": "${OPENAI_API_KEY:-sk-zerolimit}",
      "api_mode": "responses",
      "models": {
        "gpt-5.3-codex": {
          "display_name": "GPT-5.3 Codex",
          "max_input_tokens": 128000,
          "max_output_tokens": 16384
        }
      }
    }
  }
}
```

Set your API key (optional for local subscription-backed CLIProxy setups that accept `sk-zerolimit`):

```bash
export OPENAI_API_KEY="your-api-key-here"
```

The default `base_url` for CLIproxyAPI is `http://127.0.0.1:8317/v1`. The `api_mode: "responses"` setting enables the OpenAI Responses API with streaming support.

## License Hygiene

This project draws behavioral inspiration from Oh My OpenCode and Oh My Pi:

- **Architecture patterns**: Event sourcing, hashline edits, permission models
- **User experience**: Terminal UI workflows, streaming output

No code, prompts, or proprietary implementations were copied. All code is original and independently authored.

**License notes**:
- MIT-licensed repositories (like Oh My OpenCode) are fine for inspiration
- Pi Agent Rust license is unclear; do not copy code from it

## Migration Notes

Agent Harness does **not** auto-import OpenCode configuration. You must create a harness-specific config file. Key differences from OpenCode:

- Different schema structure
- No auto-discovery of models
- Explicit category definitions required
- Hashline edits instead of standard file edits
