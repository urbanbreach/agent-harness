# Shipped config surface

This document describes the currently blessed config surface for the shipped Plan/Build path. It is intentionally narrower than the full long-term roadmap, but it still names the full top-level schema so docs and validation stay aligned.

## Canonical example

The authoritative example is [`configs/harness.example.jsonc`](../configs/harness.example.jsonc).

Its shipped defaults are:

- provider: `default`
- provider type: `openai_compatible`
- endpoint: `http://127.0.0.1:8317/v1`
- default agent: `build`
- planning agent: `plan`
- default model family: `gpt-5.4-mini`

## Top-level keys

| Key | Purpose for the shipped path |
| --- | --- |
| `$schema` | Optional schema URI for editor tooling. |
| `agent` | Legacy-compatible single map for shipped agent definitions such as `plan` and `build`. |
| `agents` | Canonical agent-profile map; the loader normalizes this with `agent`. |
| `default_agent` | Default interactive/prompt agent, which the shipped example sets to `build`. |
| `hooks` | Optional hook configuration for runtime lifecycle integration. |
| `integrations` | Remote search and MCP integration settings used by read-only investigation tools. |
| `instructions` | Optional extra instruction files appended to each configured agent prompt. |
| `logging` | Log and artifact output controls. |
| `lsp` | Language-server integration settings for `code.lsp`. |
| `permissions` | Global permission defaults and shell allowlist policy. |
| `providers` | Provider definitions and model catalogs. |
| `runtime` | Session directories, background-task tuning, determinism, and prompt runtime behavior. |
| `skills` | Optional shipped skill configuration and discovery overrides. |
| `ui` | UI defaults such as the chosen default profile. |

## Provider contract

The shipped example keeps docs, config, and live signoff aligned around a local CLIProxy-compatible bridge. The example intentionally uses `${OPENAI_API_KEY:-sk-zerolimit}` so first-run docs do not require a real remote credential just to exercise the local path.

## Agent contract

### `build`
- default implementation lane
- `variant: "high"`
- edit + shell + task permissions enabled
- includes file edit, shell, batch, and `agent.spawn` tools
- prompt requires focused implementation plus verification evidence

### `plan`
- explicit planning lane
- `plan_mode: true`
- `exit_target_profile: "build"`
- edit + shell + task permissions denied
- uses read-only investigation tools plus `plan.exit`
- prompt requires a concrete plan before approval-gated handoff

### Secondary shipped lanes
- `tool_audit`: evidence-first signoff profile for shipped surface validation
- `deep_compat`: compat regression profile for alias/tool-surface parity

## Model selection from config

The harness chooses a model from config through the selected agent/profile:

- each profile's `model_ref` selects its provider/model pair
- `ui.default_profile` picks which configured profile `harness tui` and `harness prompt` start with when you do not pass `--profile`
- switching `ui.default_profile` or a profile `model_ref` therefore changes the default runtime model choice without requiring CLI flags

## Reasoning preset selection from config

Profiles can also pin a reasoning preset with `reasoning_effort`:

- set `agents.<name>.reasoning_effort` to `none`, `minimal`, `low`, `medium`, `high`, or `xhigh`
- the harness applies that preset after model selection, so the configured reasoning behavior stays predictable when a profile switches models
- configured variant metadata can still supply model-specific defaults, but an explicit profile `reasoning_effort` wins when both are set

## Other shipped settings that matter to the Plan/Build path

### `permissions`
The example keeps defaults ask-gated, then grants stronger powers per-agent. This makes `plan` read-only by policy while leaving `build` usable for implementation.

### `runtime.background_tasks`
Concurrency and stale-timeout settings are configured in the shipped example so the default interactive path stays responsive under background verification.

### `integrations`
Remote search and MCP server definitions are part of the shipped config surface because they affect which read-only investigation tools are available in `plan` and `build`.

### `instructions`
The harness also accepts an `instructions` array inspired by Opencode. Each entry is read as a workspace-visible file path and appended to every configured agent prompt at runtime. This keeps the behavior explicit and legible: the prompt still starts from each profile's configured `system_prompt`, then adds the extra instruction file contents in order.

## Opencode-like compatibility aliases

The loader now accepts a focused set of Opencode-style config aliases and normalizes them into the canonical harness shape:

- top-level `mcp` -> `integrations.mcp.servers`
- top-level `permission` -> `permissions.defaults` for the overlapping ask/allow/deny fields
- agent `model` -> `model_ref` (`provider/model` becomes `provider:model`)
- agent `prompt` -> `system_prompt`
- agent `steps` / `maxSteps` -> `max_iters`
- agent `permission` -> per-agent `permissions`

When `permissions`, `runtime`, or `integrations` are omitted entirely, the harness now fills them with safe defaults before validation so compact Opencode-like configs stay loadable. Command-specific shell permission rules are still out of scope for now; only the wildcard `bash: { "*": ... }` form is normalized.

## `HarnessConfig` schema reference

| Key | Notes |
| --- | --- |
| `$schema` | Optional JSON Schema pointer. |
| `agent` | Accepted alias for agent definitions. |
| `agents` | Canonical agent-definition map. |
| `default_agent` | Default selected agent/profile. |
| `hooks` | Hook definitions and execution policy. |
| `integrations` | MCP and remote-search integration config. |
| `instructions` | Extra prompt-instruction file paths. |
| `logging` | Session/event logging config. |
| `lsp` | LSP server wiring and feature toggles. |
| `permissions` | Permission defaults, overrides, and shell allowlist. |
| `providers` | Provider transports plus per-model metadata. |
| `runtime` | Background-task, prompt, and deterministic runtime settings. |
| `skills` | Skill loading and shipped-skill config. |
| `ui` | UI/profile defaults and presentation settings. |

## Minimal non-default example

```jsonc
{
  "hooks": {
    "on_run_started": []
  },
  "skills": {
    "enabled": true
  },
  "lsp": {
    "enabled": true
  }
}
```

## Related docs

- [`docs/plan-build-workflow.md`](plan-build-workflow.md)
- [`docs/testing.md`](testing.md)
