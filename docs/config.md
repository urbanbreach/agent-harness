# Config reference

The harness public config contract uses harness-centered naming throughout.
Canonical runtime config discovery uses `harness.json` / `harness.jsonc`; TUI-only
settings use `tui.json` / `tui.jsonc`.

The generated JSON schemas are the source of truth:

- runtime: `configs/config.json`
- tui: `configs/tui.json`

## Minimal starter

Start with `configs/harness.example.jsonc`. It keeps the happy path small: one
OpenAI-compatible provider, one default model, scalar permission mode, and
optional MCP. The runtime fills in the standard `build` agent and
provider-context compaction defaults unless you override them explicitly.

```jsonc
{
  "$schema": "./config.json",
  "provider": {
    "default": {
      "type": "openai_compatible",
      "name": "Local OpenAI-Compatible Provider",
      "options": {
        "baseURL": "http://127.0.0.1:8317/v1",
        "apiKey": "placeholder-api-key"
      },
      "models": {
        "gpt-5.4-mini": {
          "name": "GPT 5.4 Mini",
          "limit": { "context": 272000, "input": 272000, "output": 128000 }
        }
      }
    }
  },
  "model": "default/gpt-5.4-mini",
  "default_agent": "build",
  "permission": "ask"
}
```

Only write the settings you want to own. Model catalog metadata, agent tool
lists, background-task knobs, and compaction defaults are runtime concerns; keep
them out of day-to-day configs unless a project needs a deliberate override.

The larger provider catalog lives in `configs/provider-catalog.reference.jsonc`.
That file is a reference and validation fixture for provider and model metadata,
including variants and larger model lists. It is not auto-loaded by config
discovery. Validate it explicitly when you want to check the catalog:

```bash
cargo run -p harness -- --config configs/provider-catalog.reference.jsonc config validate
```

## Public contract summary

| Area | Canonical shape | Notes |
| --- | --- | --- |
| Runtime config file | `harness.json` / `harness.jsonc` | Shared defaults live under the matching XDG harness directory. |
| TUI config file | `tui.json` / `tui.jsonc` | Runtime and TUI settings are intentionally split. |
| Core runtime keys | `provider`, `model`, `small_model`, `model_profile`, `agent`, `default_agent`, `permission`, `runtime`, `mcp`, `skills`, `instructions` | Unsupported product-level areas are rejected explicitly. |
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
| `default_agent` | Default interactive agent selected at startup. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `mcp` | MCP server definitions keyed by server name. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback targets. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. |
| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime knobs that are not provider/model/agent definitions, currently including provider-context compaction settings. |
| `small_model` | Optional smaller model reference for custom secondary profiles. |
| `skills` | Shared skill discovery roots and permission overrides for skill loading. |

## TUI top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `keybinds` | Supported TUI keybinding overrides. |

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

1. inline `agent.<name>.system_prompt`
2. discovered `.agent-harness/agents/<name>.md`
3. markdown frontmatter `system_prompt` in `.agent-harness/agents/<name>.md`

Project instructions are still auto-discovered from `AGENTS.md`. If
`instructions` is set in the runtime config, those entries are prepended ahead
of the discovered `AGENTS.md` content.

## Permission policy

The canonical scalar form is:

```jsonc
{ "permission": "ask" }
```

`permission` accepts exactly `"ask"`, `"allow"`, or `"deny"`. A scalar applies to
all canonical public permission kinds: `bash`, `edit`, `question`, `task`,
`webfetch`, `websearch`, `codesearch`, and `lsp`.

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

`bash` and `edit` also support bounded selector maps. They are not a general
policy language:

```jsonc
{
  "permission": {
    "bash": {
      "git status": "allow",
      "cargo test*": "ask",
      "*": "deny"
    },
    "edit": {
      "docs/**": "allow",
      "crates/harness-core/src/config.rs": "ask",
      "*": "deny"
    }
  }
}
```

Bash selectors are either an exact command string, a trailing `*` prefix such as
`cargo test*`, or the `*` catch-all. Edit selectors are either an exact
workspace-relative path, a trailing `/**` path prefix such as `docs/**`, or the
`*` catch-all. Regex and general glob syntax are not supported.

`shell_allowlist` remains supported inside `permission` for the existing shell
allowlist checks. Permission decisions improve operator UX by deciding whether a
tool call runs, asks, or is denied. They are not a sandbox or security boundary.

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

- Unsupported top-level areas such as `server`, `command`, `plugin`, `share`, `autoupdate`, `enterprise`, `experimental`, and top-level `tools` are rejected explicitly.
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
operator-facing presets such as an extended-context CLIProxyAPI GPT profile while
still using the same underlying provider model id.

The coordinator uses those values to decide when proactive compaction should checkpoint older provider-visible history and how much recent context to preserve verbatim. The preserved tail defaults to roughly a quarter of usable context, clamped to a practical coding-agent range, while always keeping at least the latest complete turn when possible.

Public compaction knobs live under `runtime.compaction`:

| Key | Default | Purpose |
| --- | --- | --- |
| `modelBacked` / `model_backed` | `false` | When enabled, the coordinator asks a configured provider model for the checkpoint summary. Model output must keep the Harness structured headings and fit the summary budget, otherwise deterministic fallback is used. |
| `model` / `modelRef` / `model_ref` | unset | Optional model reference for summary calls. When unset, the active turn model is used. |
| `splitOversizedTurns` / `split_oversized_turns` | `false` | Allows overflow compaction to split an oversized latest turn inside the checkpoint artifact, compacting the earlier portion while preserving a suffix as recent provider context. |
| `autoRetryOverflow` / `auto_retry_overflow` | `true` | Keeps the existing one-shot overflow compaction retry enabled. Set `false` to fail immediately on provider context-window errors. |
| `structuredSummaryContract` / `structured_summary_contract` | `true` | Requires default-on checkpoint summaries to carry the Harness sections `Goal`, `Constraints`, `Progress`, `Key Decisions`, `Next Steps`, and `Critical Context`. Set `false` only for legacy heading compatibility. |
| `estimatedTokenTriggers` / `estimated_token_triggers` | `true` | Allows proactive and pre-prompt compaction to use deterministic context estimates when provider usage or model metadata is absent. |
| `fallbackInputTokens` / `fallback_input_tokens` | `32768` | Input budget used for estimated trigger checks when the active model does not publish a context window or max input token limit. |

On successful compaction, checkpoints are written under `artifacts/compactions/<agent_id>/` and recorded in the session event log. Checkpoints and compaction events include additive before/after active-context estimates (`tokens_before_estimate`, `tokens_after_estimate`, summary-token estimate, compacted/preserved turn counts, and estimated reduction) so UIs can report whether compaction helped without treating historical provider spend as active context. Checkpoints also include structured source facts, tail-boundary metadata, summary-source metadata, the summary contract version, replay-derived read/modified file counts, and a timeline entry for replay/UIs. Resume reconstructs provider context from the latest applied checkpoint plus post-checkpoint deltas in `events.jsonl`; the event log itself stays append-only.

Manual `/compact` is a checkpoint command, not a guaranteed immediate token-shrink command: it writes a checkpoint now, summarizes older completed turns, preserves the latest completed turn verbatim, and uses the normal compaction artifact/event format. The success notice reports the active-context estimate delta when available, or says the estimate was unchanged. The default summary contract uses the Harness sections for goal, constraints, progress, key decisions, next steps, and critical context, with operational memory and source facts added as replay-derived context; it is still lossy. Sessions with only one completed turn no-op because there is no older turn to summarize.

Lifecycle hooks may use `event = "compaction_requested"` to observe or cancel compaction. A critical hook failure cancels compaction and records `CompactionFailed`. A successful hook can replace the summary by emitting output prefixed with `compaction_summary:`; hook overrides take precedence over model-backed summaries. Otherwise, model-backed summaries are used only when explicitly enabled, and invalid/empty/failing model output falls back to the deterministic structured summary with `summary_source.deterministic_fallback=true`.

Overflow retry is related but distinct: if the provider rejects a request for context-window reasons, the coordinator may compact and retry once with the checkpointed context when that retry can prove it shrank the provider-visible payload. Estimated pre-prompt compaction uses the same checkpoint path before provider request construction. If a pre-prompt checkpoint cannot reduce the estimated active context, the coordinator records the failure and does not loop on the same turn.

Failed or aborted provider turns can be preserved in active context and checkpoint artifacts. Replay/debug projections keep the incomplete marker, failure stage, and redacted reason so a future provider call does not treat partial assistant output as a completed answer.

Operational memory is derived from persisted events and checkpoint artifacts, not from live filesystem scans. It records capped read-file facts, modified-file facts, compact operation facts, and metadata counts that help operators understand what context survived compaction.

TUI memory or transcript caps are separate presentation settings. They affect what the operator sees on screen, not the persisted provider context used for resume or overflow-retry compaction. The TUI distinguishes active context estimate from cumulative provider tokens spent: active context may decrease after `CompactionApplied`, while total spend remains cumulative and never decreases.
