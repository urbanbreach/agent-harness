# Config reference

The harness public config contract uses harness-centered naming throughout.
Canonical runtime config discovery uses `harness.json` / `harness.jsonc`; TUI-only
settings use `tui.json` / `tui.jsonc`.

The generated JSON schemas are the source of truth:

- runtime: `configs/config.json`
- tui: `configs/tui.json`

## Minimal starter

Start with `configs/harness.example.jsonc`. It keeps the happy path small: one
OpenAI-compatible provider, one default model, explicit built-in agents you can
toggle with `enable`, scalar permission mode, and optional MCP. The runtime still
fills in the default details for each listed agent unless you override them.

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
  "model": "default/gpt-5.4-mini",
  "agent": {
    "build": { "enable": true },
    "plan": { "enable": true },
    "general": { "enable": true },
    "explore": { "enable": true },
    "title": { "enable": true, "hidden": true },
    "summary": { "enable": true, "hidden": true },
    "compaction": { "enable": true, "hidden": true }
  },
  "default_agent": "build",
  "permission": "ask"
}
```

Only write the settings you want to own. The example lists built-in agents for
discoverability, but each `{ "enable": true }` entry still inherits the shipped
description, prompt, permissions, and tools. Keep model catalog metadata, agent
tool lists, background-task knobs, and compaction defaults out of day-to-day
configs unless a project needs a deliberate override.

Reasoning-effort presets use the same explicit `variants` shape as OpenCode.
Each variant is a named model option preset; for OpenAI-compatible reasoning
models, set `metadata.reasoningEffort` so the TUI can display and select variants
like `low`, `medium`, or `high`. Use additional variant fields only for
non-standard names or per-variant limits, modalities, or options.

The larger provider catalog lives in `configs/provider-catalog.reference.jsonc`.
That file is a reference and validation fixture for provider and model metadata,
including variants and larger model lists. It is not auto-loaded by config
discovery. Validate it explicitly when you want to check the catalog:

```bash
cargo run -p harness -- --config configs/provider-catalog.reference.jsonc config validate
```

You can also update the checked-in generated provider catalog from the public
models.dev capability dataset, similar to Pi's generated model registry:

```bash
cargo run -p harness -- models generate
```

`models generate` is an explicit offline-maintenance command, not runtime
discovery. By default it fetches `https://models.dev/api.json`, filters to
non-deprecated tool-call-capable models, and writes
`configs/provider-catalog.generated.json`. The harness binary embeds that file
with `include_str!`, so `models generated` can print the static registry without
network access, matching Pi's generate-then-bundle workflow. Use
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

## Public contract summary

| Area | Canonical shape | Notes |
| --- | --- | --- |
| Runtime config file | `harness.json` / `harness.jsonc` | Shared defaults live under the matching XDG harness directory. |
| TUI config file | `tui.json` / `tui.jsonc` | Runtime and TUI settings are intentionally split. |
| Core runtime keys | OpenCode-compatible `provider`, `model`, `small_model`, `agent`, `default_agent`, `permission`, `mcp`, `skills`, `instructions`, plus harness runtime extensions | Side-effectful OpenCode product areas are accepted only when inactive and rejected when active. |
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
| `autoshare` | OpenCode-compatible sharing flag; inactive `false` is accepted, active sharing is rejected. |
| `autoupdate` | OpenCode-compatible update flag; inactive `false` is accepted, active updates are rejected. |
| `command` | OpenCode command configuration; accepted only when empty because the harness does not execute configured commands. |
| `compaction` | OpenCode-compatible compaction settings accepted as inert compatibility input; harness compaction knobs live under `runtime.compaction`. |
| `default_agent` | Default interactive agent selected at startup; the shipped example keeps `build` as the default while `plan` remains selectable. |
| `disabled_providers` | OpenCode-compatible provider filter accepted as inert compatibility input. |
| `enabled_providers` | OpenCode-compatible provider filter accepted as inert compatibility input. |
| `enterprise` | OpenCode enterprise configuration; accepted only when empty because the harness does not implement enterprise product integration. |
| `experimental` | OpenCode-compatible experimental settings accepted as inert compatibility input. |
| `formatter` | OpenCode-compatible formatter settings accepted as inert compatibility input. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `layout` | Deprecated OpenCode layout setting accepted as inert compatibility input. |
| `logLevel` | OpenCode-compatible log-level setting accepted as inert compatibility input. |
| `lsp` | OpenCode-compatible LSP setting; `false` disables harness LSP overrides, object values map to harness LSP servers when possible. |
| `mcp` | MCP server definitions keyed by server name. |
| `mode` | Deprecated OpenCode alias for `agent`; entries are translated as agent definitions. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback targets. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. |
| `plugin` | OpenCode plugin list; accepted only when empty because plugins are not loaded by the harness. |
| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime knobs that are not provider/model/agent definitions, currently including provider-context compaction settings. |
| `server` | OpenCode server configuration; accepted only when empty because server commands are outside this runtime config. |
| `share` | OpenCode sharing mode; only `disabled` is accepted. |
| `shell` | OpenCode-compatible default-shell setting accepted as inert compatibility input. |
| `small_model` | Optional smaller model reference for custom secondary profiles. |
| `snapshot` | OpenCode-compatible snapshot setting accepted as inert compatibility input. |
| `skills` | Shared skill discovery roots and permission overrides for skill loading. |
| `tool_output` | OpenCode-compatible tool-output truncation setting accepted as inert compatibility input. |
| `tools` | OpenCode-compatible top-level tool map accepted as inert compatibility input. |
| `username` | OpenCode-compatible username setting accepted as inert compatibility input. |
| `watcher` | OpenCode-compatible watcher settings accepted as inert compatibility input. |

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

1. inline `agent.<name>.system_prompt` / `agent.<name>.prompt`
2. discovered `.agent-harness/agents/<name>.md`
3. markdown frontmatter `system_prompt` / `prompt` in `.agent-harness/agents/<name>.md`

Project instructions are still auto-discovered from `AGENTS.md`. If
`instructions` is set in the runtime config, those entries are prepended ahead
of the discovered `AGENTS.md` content.

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

The shipped `build` agent also exposes the coordinator-owned `team_*` tools for
lead-agent coordination. A team has four stable roles: supervisor/operator,
lead, write-capable member, and read-only research member. `team_create.lead`
selects an optional write-capable lead profile; when present, the coordinator
spawns and projects it separately from members. Team member entries default to
`role: "member"`; set `role: "research"` only for read-only profiles such as
`explore`. Research members may appear in team status and complete shutdown
handshakes, but coordinator validation denies their team message/task writes.
Team member profiles that need to write shared team messages or tasks must
include the relevant `team_*` tool ids in their toolset; worker calls are bound
to the lead/member identity projected from the team event log.

### Plan operator workflow

Use Plan when the operator wants a reviewed implementation plan before changing
project files. Harness ships Plan as a stable public runtime surface, not an
experimental OpenCode flag, and the safety boundary is enforced by coordinator
permissions as well as prompt instructions.

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

This differs intentionally from OpenCode's broader experimental Plan behavior:
Harness keeps `plan_exit` available in the shipped `plan` profile and keeps
Plan-spawned child work restricted to `explore` unless a future policy adds tested
parent-permission inheritance for write-capable subagents.

The shipped agent names are available without extra config: primary
`build` and `plan`, subagents `general` and `explore`, plus hidden `title`,
`summary`, and `compaction` profiles. `explore` is a read-only local codebase
search profile for `task(subagent_type: "explore")`. `general` is a broader
focused implementation/research profile for `task(subagent_type: "general")`; it
intentionally omits `task` by default so subagents do not recursively redelegate
unless a project opts into that tool.
When a subagent profile does not configure its own `model`, task delegation
inherits the invoking parent turn's active model and model settings. If the
subagent profile has an explicit `model`, that configured model wins. The `task`
tool requires `run_in_background` and `load_skills` on every call; pass
`load_skills: []` when no skill context is needed. Listed skills are resolved
before the child is spawned, missing or denied skills fail the call, and loaded
skill content is injected into the child prompt before the original task body.
`task(run_in_background: true)` returns a child `request_id`; use the
`background_output` tool with that `request_id` to inspect completion status or
the terminal result. Retrieval is event-replay based and does not advance the
child task. To stop an authorized non-terminal child request, call
`background_output` with the same `request_id`, `cancel: true`, and an optional
`reason`; the coordinator records cancellation through the normal task lifecycle.
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
      "cargo test*": "ask",
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
`cargo test*`, or the `*` catch-all. Edit selectors are either an exact
workspace-relative path, a trailing `/**` path prefix such as `docs/**`, or the
`*` catch-all. Task selectors match the requested subagent/profile/category name;
they accept exact names, `*` catch-all, and simple `*` glob patterns such as
`review-*`. Regex is not supported.

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

- Unsupported top-level areas are limited to active OpenCode product features and unknown keys.
- OpenCode top-level areas that would trigger product side effects (`server`, `command`, `plugin`, `share`, `autoshare`, `autoupdate`, `enterprise`) are rejected when active; inactive forms such as empty maps/lists, `share: "disabled"`, or `autoupdate: false` are accepted.
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
