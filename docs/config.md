# Config reference

The harness public config contract uses harness-centered naming throughout.
Canonical runtime config discovery uses `harness.json` / `harness.jsonc`; TUI-only
settings use `tui.json` / `tui.jsonc`.

The generated JSON schemas are the source of truth:

- runtime: `configs/config.json`
- tui: `configs/tui.json`

## Public contract summary

| Area | Canonical shape | Notes |
| --- | --- | --- |
| Runtime config file | `harness.json` / `harness.jsonc` | Shared defaults live under the matching XDG harness directory. |
| TUI config file | `tui.json` / `tui.jsonc` | Runtime and TUI settings are intentionally split. |
| Core runtime keys | `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions` | Unsupported product-level areas are rejected explicitly. |
| TUI surface | `keybinds` | Unsupported TUI-only fields fail validation. |
| Permission naming | `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp` | Legacy `shell` / `network` remain compatibility-only. |
| Prompt asset discovery | `.agent-harness/agents/*.md` | `AGENTS.md` is still auto-discovered separately. |

## Runtime top-level keys

| Key | Purpose |
| --- | --- |
| `$schema` | Optional schema URI for editor integration. |
| `agent` | Optional agent overrides or custom agent definitions. |
| `default_agent` | Default interactive agent selected at startup. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `mcp` | MCP server definitions keyed by server name. |
| `model` | Default full-capability model reference. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. |
| `provider` | Provider definitions keyed by provider id. |
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

## Prompt and instruction discovery

The runtime config stays focused on provider/model/agent selection. Prompt prose
and repository instructions still come from files:

1. inline `agent.<name>.system_prompt`
2. discovered `.agent-harness/agents/<name>.md`
3. markdown frontmatter `system_prompt` in `.agent-harness/agents/<name>.md`

Project instructions are still auto-discovered from `AGENTS.md`. If
`instructions` is set in the runtime config, those entries are prepended ahead
of the discovered `AGENTS.md` content.

## Compatibility behavior

The loader still accepts the previous broad harness-native shape for migration:

- `providers`, `agents`, `permissions`
- `runtime`, `integrations`, `ui`
- `hooks`, `skills`, `lsp`, `logging`, `hashline_edit`
- compatibility aliases such as `categories`, `profiles`, `backgroundTask`, `paths`, and `deterministic`
- compatibility permission names such as `shell` and `network`
- compatibility config path `$XDG_CONFIG_HOME/harness/config.jsonc`

Those keys and paths are compatibility inputs, not the canonical public contract.
New configs, examples, docs, and schema-driven validation should use the
harness-centered runtime/TUI split shown above.

## Validation behavior

- Unsupported top-level areas such as `server`, `command`, `plugin`, `share`, and `autoupdate` are rejected explicitly.
- Unsupported TUI fields are rejected explicitly.
- `{env:VAR}` resolves to an empty string when `VAR` is unset.
- `{file:path}` is supported for string references and resolves relative to the config file when the config comes from disk.
- Legacy `${VAR}` and `${VAR:-fallback}` references remain accepted for compatibility.

## Provider context compaction expectations

Provider-context compaction relies on the active profile/model metadata, especially:

- `context_window_tokens`
- `max_input_tokens`
- `max_output_tokens`

The coordinator uses those values to decide when proactive compaction should checkpoint older provider-visible history and how much recent context to preserve verbatim. The preserved tail defaults to roughly a quarter of usable context, clamped to a practical coding-agent range, while always keeping at least the latest complete turn when possible.

On successful compaction, checkpoints are written under `artifacts/compactions/<agent_id>/` and recorded in the session event log. Checkpoints and compaction events include additive before/after active-context estimates (`tokens_before_estimate`, `tokens_after_estimate`, summary-token estimate, compacted/preserved turn counts, and estimated reduction) so UIs can report whether compaction helped without treating historical provider spend as active context. Checkpoints also include structured source facts, tail-boundary metadata, summary-source metadata, and a timeline entry for replay/UIs. Resume reconstructs provider context from the latest applied checkpoint plus post-checkpoint deltas in `events.jsonl`; the event log itself stays append-only.

Manual `/compact` is a checkpoint command, not a guaranteed immediate token-shrink command: it writes a checkpoint now, summarizes older completed turns, preserves the latest completed turn verbatim, and uses the normal compaction artifact/event format. The success notice reports the active-context estimate delta when available, or says the estimate was unchanged. The summary is a deterministic structured checkpoint with sections for goal, constraints, progress, blockers, decisions, next steps, critical context, source facts, and relevant files/artifacts; it is still lossy. Sessions with only one completed turn no-op because there is no older turn to summarize.

Lifecycle hooks may use `event = "compaction_requested"` to observe or cancel compaction. A critical hook failure cancels compaction and records `CompactionFailed`. A successful hook can replace the summary by emitting output prefixed with `compaction_summary:`; otherwise the deterministic summary builder is used.

Overflow retry is related but distinct: if the provider rejects a request for context-window reasons, the coordinator may compact and retry once with the checkpointed context when that retry can prove it shrank the provider-visible payload.

TUI memory or transcript caps are separate presentation settings. They affect what the operator sees on screen, not the persisted provider context used for resume or overflow-retry compaction. The TUI distinguishes active context estimate from cumulative provider tokens spent: active context may decrease after `CompactionApplied`, while total spend remains cumulative and never decreases.
