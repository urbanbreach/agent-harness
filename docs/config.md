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
    "discipline": { "enable": true },
    "general": { "enable": true },
    "explore": { "enable": true },
    "visual-engineering": { "enable": true },
    "artistry": { "enable": true },
    "ultrabrain": { "enable": true },
    "deep": { "enable": true },
    "quick": { "enable": true },
    "unspecified-low": { "enable": true },
    "unspecified-high": { "enable": true },
    "writing": { "enable": true },
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
| `compatibility` | Safe migration controls and import diagnostics for Claude/OpenCode/OMO-compatible agents, skills, commands, MCP files, hooks, and extension manifests. |
| `default_agent` | Default interactive agent selected at startup; the shipped example keeps `build` as the default while `plan` remains selectable. |
| `disabled_agents` | Top-level compatibility shortcut merged into `compatibility.disabled_agents`. |
| `disabled_commands` | Top-level compatibility shortcut merged into `compatibility.disabled_commands`. |
| `disabled_extensions` | Top-level compatibility shortcut merged into `compatibility.disabled_extensions`. |
| `disabled_hooks` | Top-level compatibility shortcut merged into `compatibility.disabled_hooks`. |
| `disabled_mcps` | Top-level compatibility shortcut merged into `compatibility.disabled_mcp_servers`. |
| `disabled_skills` | Top-level compatibility shortcut merged into `compatibility.disabled_skills`. |
| `disabled_providers` | OpenCode-compatible provider filter accepted as inert compatibility input. |
| `enabled_providers` | OpenCode-compatible provider filter accepted as inert compatibility input. |
| `enterprise` | OpenCode enterprise configuration; accepted only when empty because the harness does not implement enterprise product integration. |
| `experimental` | OpenCode-compatible experimental settings accepted as inert compatibility input. |
| `formatter` | OpenCode-compatible formatter settings accepted as inert compatibility input. |
| `instructions` | Optional inline instructions or instruction file paths prepended before agent prompts. |
| `layout` | Deprecated OpenCode layout setting accepted as inert compatibility input. |
| `logLevel` | OpenCode-compatible log-level setting accepted as inert compatibility input. |
| `lsp` | OpenCode-compatible LSP setting; `false` disables harness LSP overrides, object values map to harness LSP servers when possible. |
| `mcp` | MCP server definitions keyed by server name; disabled bundled examples include Exa, Tavily, Context7, and Grep.app-style profiles. |
| `mode` | Deprecated OpenCode alias for `agent`; entries are translated as agent definitions. |
| `model` | Default full-capability model reference. |
| `model_profile` | Named model selectors that resolve to configured provider/model targets plus optional fallback targets. |
| `permission` | Default permission policy for the supported tool subset plus optional shell allowlist. |
| `plugin` | OpenCode plugin list; accepted only when empty because plugins are not loaded by the harness. |
| `provider` | Provider definitions keyed by provider id. |
| `runtime` | Runtime knobs that are not provider/model/agent definitions, currently including provider-context compaction and staged workflow settings. |
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
Declared team specs can be stored at `.agent-harness/teams/<name>.json` or the
user Harness config `teams/` directory. `team_list` reports these declared specs
alongside replay-derived active runs and validates the spec version, file/name
match, non-empty members, member bounds, lead eligibility, and research/member
role compatibility. Declared specs do not spawn teams by themselves; `team_create`
remains the coordinator-owned state transition.

Team specs and task/delete metadata may include workflow policy refs such as
`workflow_id`, `evidence_ref`, `synthesis_ref`, `file_claim.path`,
`worktree.path`, and `tmux.pane`. These fields are advisory, replay-derived
coordination state: file claims are not filesystem locks, and replay never
creates worktrees or tmux panes. `team_delete` is completion-gated: all members
must have approved shutdown and the projection must show no pending/claimed/
in-progress team tasks plus verification evidence, unless `abort_reason`
metadata is supplied explicitly.

The shipped `discipline` agent is the opt-in autonomous delivery workflow. It is
a separate primary profile, not a global toggle: use it when a turn should enforce
todo hygiene, focused delegation, and end-to-end surface verification. The
behavior remains prompt/profile-scoped and does not add coordinator-owned
background scheduler loops, plugin loading, or hidden continuation semantics.

`harness doctor` validates the operator-facing orchestration surface without
making provider, MCP, browser, or terminal network/session calls. It checks
provider/model metadata, provider credential availability without printing key
values, configured agent and model-profile references, shipped workflow profile
availability, effective fallback chains, model tool/modality capability,
category route coverage, profile tool ids, permissions,
session-directory readiness, configured MCP server state, and browser/terminal
dependency diagnostics.
Use `--json` for machine-readable output.

At runtime, model-profile fallback targets are consumed only when configured and
only for classified retryable provider failures such as rate limits, overloads,
transport errors, and post-compaction context-window failures. Provider
start/finish metadata records the fallback attempt and classified reason without
changing replay semantics.

### Workflow contract registry

Harness keeps first-party workflow ids in code registries instead of treating a
project file as authoritative runtime state. The initial workflow-slice SSOT is
split by crate responsibility:

- `harness-core::workflow_registry` owns stable workflow mode, lane, outcome,
  evidence category, transition policy, doctor-check, and docs-anchor ids.
- `harness-core::command_registry` owns canonical command names and aliases such
  as `workflow-run`, `workflow-status`, `workflow-signoff`, `workflow-cancel`,
  `workflow-dossier`, `workflow-snapshot`, `plan-consensus`, and `goal-ledger`.
  These entries resolve to workflow intents and must not execute shell tools.
- `evidence.plan_consensus` records the reviewed planner/architect/critic plan
  artifact metadata (ADR, options, risks, test plan, staffing, evidence refs,
  verdict, and bounded critic iterations) so replay can expose current plan
  status without reading artifacts.
- `evidence.goal_ledger` records goal/story create and checkpoint metadata,
  including evidence refs and final quality-gate refs, so replay can derive
  aggregate goal status and block final completion until checkpoint evidence
  plus verification/review quality evidence are present.
- `harness doctor --json` includes the `workflow_contract_registry` check so docs
  anchors and stable id groups drift visibly.
- `harness doctor --json` also includes `workflow_context_snapshot`, which
  reports the redacted/capped context snapshot artifact contract that workflow
  status and dossier projections consume from events.
- `harness doctor --json` includes `workflow_runtime_config`, which validates
  staged `runtime.workflow` defaults and operator limits without launching
  providers, tools, tmux, or workers.
- `harness doctor --json` includes `workflow_simulator`, which verifies the
  deterministic testkit simulator contract requires context snapshot evidence
  plus `evidence.simulated_tool_result` before signoff unless an operator waiver
  is recorded.
- `harness doctor --json` includes `workflow_stale_work_loop`, which inspects
  the latest `events.jsonl` under the configured session directory and warns
  when workflow-owned continuations are still active or reminder-queued.
- `harness workflow run/status/signoff/cancel/dossier/snapshot/plan-consensus/goal/init`
  are the CLI foundation commands. `status`, goal status/read/list, and
  dossier/snapshot reads are projection-only over `events.jsonl`; dossier export
  includes the replay-derived quality gate and prompt-to-artifact completion audit.
  `harness workflow goal create/status/checkpoint/list/read` is the
  goal-ledger surface, while `init --check` reports planned files without
  writing and `init --apply` is the explicit write path for safe generated files
  under `.agent-harness/`.
- `harness workflow snapshot write --json` is the minimal coordinator-backed
  CLI write path for `/interview` and `/workflow run` intake snapshots; it
  stores artifacts under the session run and emits workflow evidence when a
  workflow id is provided.
- `harness workflow plan-consensus --json` writes a session-scoped plan artifact
  and records `evidence.plan_consensus`; `/ralplan` and `/consensus-plan`
  command aliases resolve to the same workflow intent.
- `docs/omx-workflow-slice-spec.md` remains the source narrative for the broader
  slice, while replayable workflow state must still come from coordinator-owned
  events and redacted artifact references.

### Plan operator workflow

Use Plan when the operator wants a reviewed implementation plan before changing
project files. Harness ships Plan as a stable public runtime surface, not an
experimental upstream-compatible flag, and the safety boundary is enforced by coordinator
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

This differs intentionally from broader upstream experimental Plan behavior:
Harness keeps `plan_exit` available in the shipped `plan` profile and keeps
Plan-spawned child work restricted to `explore` unless a future policy adds tested
parent-permission inheritance for write-capable subagents.

The shipped agent names are available without extra config: primary
`build`, `plan`, and `discipline`, subagents `general`, `explore`,
`visual-engineering`, `artistry`, `ultrabrain`, `deep`, `quick`,
`unspecified-low`, `unspecified-high`, and `writing`, plus hidden `title`,
`summary`, and `compaction` profiles. `explore` is a read-only local codebase
search profile for `task(subagent_type: "explore")`. `general` is a broader
focused implementation/research profile for `task(subagent_type: "general")`.
The category profiles are OMO-style routing lanes for `task(category: "...")`:
the task tool selects the matching profile first and falls back to `general` only
when no matching category profile is configured. `visual-engineering` covers UI,
UX, layout, styling, animation, and design; `artistry` covers complex creative
problem-solving; `ultrabrain` covers hard logic, architecture, algorithms, and
deep debugging; `deep` covers autonomous research and end-to-end implementation;
`quick` covers small low-risk changes; `unspecified-low` and `unspecified-high`
cover uncategorized low- and high-effort work; and `writing` covers docs and
prose. Shipped subagents intentionally omit or deny `task` by default so they do
not recursively redelegate unless a project opts into that tool.
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
The general `task_create`, `task_list`, `task_get`, and `task_update` tools are
not scheduler handles; they manage replayable persistent dependency tasks through
coordinator-owned `PersistentTaskCreated` / `PersistentTaskUpdated` events. Use
them for Atlas/Team/continuation planning state with `subject`, `description`,
`status`, `active_form`, `blocked_by`, projected `blocks`, `owner`, `metadata`,
and `run_id` / `thread_id`. `task_list` also reports `ready_task_ids` for pending
tasks whose blockers are complete. Execution of those tasks still goes through
normal coordinator-owned `task` delegation.

## Skill loading

`skills` controls discovery and loading for the `skill` tool and
`task(load_skills=[...])`. By default Harness searches project roots in this
order: `.agent-harness/skills`, `.opencode/skills`, `.claude/skills`,
`.agents/skills`, and `.harness/skills`; then user roots
`~/.config/agent-harness/skills`, `~/.config/opencode/skills`, `~/.claude/skills`,
and `~/.agents/skills`. Harness-owned paths stay first unless config replaces
the root lists explicitly.

`SKILL.md` requires `name` and `description` frontmatter. It also accepts
optional `mcp`, `permissions`, `tools`, `commands`, `environment`, and
`verification` fields. Discovery parses those fields into visible metadata only;
it does not start MCP servers or execute commands. Loaded skill content and
frontmatter policy are injected into the intended skill-tool response or child
task prompt only, and child task structured output records `loaded_skills`
metadata for replay/audit.

`skills.permissions` can allow, ask, or deny names/patterns. Denied and invalid
skills stay hidden from direct loads, while `skill({ "list": true })` reports
visible, denied, invalid, and shadowed entries plus search roots for diagnostics.
`skills.disabled: true` disables all skill discovery/loading, and
`skills.disabled_skills` / `skills.disabledSkills` disables specific skills
without changing root order.


## Compatibility imports

Harness migration compatibility is adapter-only. It translates selected
Claude/OpenCode/OMO-compatible files into first-class Harness configuration and
records the outcome under `compatibility.imports`; it does not execute plugin
code, load arbitrary extension runtimes, or enable rejected product areas. Active
`server`, `command`, `plugin`, `share`, `autoshare`, `autoupdate`, and
`enterprise` behavior still fails validation unless represented by one of the
safe adapters below.

When a runtime config is loaded from disk, Harness searches the current project
and config directory ancestors for these migration inputs:

- `.opencode/agent/*.md`, `.opencode/agents/*.md`, `.claude/agents/*.md`, and
  `.agents/agents/*.md` as Harness agent profiles.
- `.opencode/skills/*/SKILL.md`, `.claude/skills/*/SKILL.md`, and
  `.agents/skills/*/SKILL.md` as discoverable Harness skills.
- `.agent-harness/commands/*.md`, `.opencode/command/*.md`,
  `.opencode/commands/*.md`, `.claude/commands/*.md`,
  `.agents/commands/*.md`, and `.harness/commands/*.md` as prompt-only slash
  command templates. Imported templates appear in the TUI slash menu and submit
  prompt text back through the normal coordinator path; shell execution still
  goes through the normal tool permission path.
- A sibling `.mcp.json` file as MCP server configuration.
- `.claude/settings.json`, `.claude/settings.local.json`, `.opencode/hooks.json`,
  and `.agents/hooks.json` safe hook subsets as typed hook import records.
  They are inert by default; set `compatibility.enable_imported_hooks: true`
  only when the project explicitly opts into executing imported hook argv entries
  through the normal Harness lifecycle hook and shell-allowlist gates.
- `.codex-plugin/plugin.json`, `.agent-harness/extensions/*/plugin.json`,
  `.agents/plugins/*/plugin.json`, and `.codex/plugins/*/plugin.json` as
  manifest-only extension registrations. Manifest registration records id, name,
  version, path, and enabled state; executable plugin loading is intentionally
  deferred.

Use `compatibility.required: true` only for controlled migrations where an import
failure should abort startup. Otherwise import failures are non-fatal and visible
in `harness doctor --json` under the stable `compatibility_imports` check. Each
compatibility item can be disabled individually:

```jsonc
{
  "compatibility": {
    "required": false,
    "enable_imported_hooks": false,
    "disabled_agents": ["legacy-writer"],
    "disabled_skills": ["old-skill"],
    "disabled_commands": ["deploy-prod"],
    "disabled_mcp_servers": ["legacy_mcp"],
    "disabled_hooks": ["compat:PostToolUse:claude-settings-json:0"],
    "disabled_extensions": ["demo-extension"]
  }
}
```

The same disabled lists are accepted as top-level shortcuts (`disabled_agents`,
`disabled_skills`, `disabled_commands`, `disabled_mcps`, `disabled_hooks`, and
`disabled_extensions`) and are merged into `compatibility`. Static deny and
explicit disable settings remain stronger than compatibility imports. Imported
compatibility hooks remain non-executable unless `enable_imported_hooks` is
explicitly enabled.

## MCP compatibility

Top-level `mcp` entries configure first-class MCP servers. HTTP servers require
an `endpoint`/`url`; stdio servers require a `command` vector. The shipped
example config includes disabled Exa, Tavily, Context7, and Grep.app profile
templates so projects can opt in without inventing ids.

For migration, when a runtime config is loaded from disk, Harness also reads a
sibling `.mcp.json` file if present. Claude/OpenCode-style `mcpServers` (or
`servers`) entries are translated into `mcp` servers unless the main config
already defines the same id. `command: "tool"` plus `args: [...]` becomes a
stdio command vector; `url` / `endpoint` becomes an HTTP MCP server. Environment
values in `.mcp.json` are treated as literal strings and are not shell-expanded
by the compatibility loader; use Harness `{env:VAR}` references in first-class
config when expansion is intended.

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
- compatibility permission names such as `shell`, `network`, `write`, `delegate_task`, and `delegateTask`
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
- Safe Claude/OpenCode permission aliases translate only to existing Harness seams: `write` maps to `edit`, `delegate_task` / `delegateTask` map to `task`, `shell` maps to `bash`, and `network` maps to `webfetch`/`websearch`; inert `read`, `doom_loop`, and `external_directory` inputs are accepted without broadening capabilities.
- `{file:path}` is supported for string references and resolves relative to the config file when the config comes from disk.
- Legacy `${VAR}` and `${VAR:-fallback}` references remain accepted for compatibility.

## Hooks

Harness hook configuration is coordinator-owned and replay-safe. `hooks.lifecycle`
keeps the command-hook compatibility surface, while the coordinator maps every
lifecycle event onto a typed middleware phase before recording metadata:

- `message_received`
- `agent_turn_started`
- `provider_params`
- `provider_context_transform`
- `tool_preflight`
- `tool_result`
- `agent_turn_finished`
- `session_idle`
- `compaction_requested`

## Slash command and continuation surface

Harness keeps the command registry separate from TUI-only actions. Built-in OMO-compatible
commands include `/init-deep`, `/ralph-loop`, `/ulw-loop`, `/cancel-ralph`, `/refactor`,
`/start-work`, `/stop-continuation`, `/remove-ai-slops`, `/handoff`, and `/hyperplan`.
Built-in registry actions may load prompts/skills, create plan or handoff artifacts,
or start/stop a coordinator-owned continuation. Workflow commands also expose
typed TUI intents for `/workflow-*`, `/plan-consensus`, `/goal-ledger`,
`/research-mission`, and `/wiki` surfaces, with compatibility aliases such as
`/ralplan`, `/ultragoal`, `/autoresearch`, and `/workflow-wiki`. Imported command
templates are prompt-only slash commands; they submit the template text (with
`{{args}}` expanded from the preserved draft when present) back through the
normal coordinator path. They do not execute shell code directly; shell
execution still requires the normal native tool permission flow.

Custom command lookup roots include Harness, OpenCode, Claude, and Agents-compatible locations:
`.agent-harness/commands`, `.opencode/command`, `.opencode/commands`, `.claude/commands`,
`.agents/commands`, `.harness/commands`, and matching user config directories.

Continuation loops are explicit and bounded. `/ralph-loop` starts Ralph continuation,
`/ulw-loop` starts ultrawork continuation, and `/stop-continuation` or `/cancel-ralph`
stops the active loop. Continuation start/reminder/stop/limit events are append-only and
resume/replay safe; replay renders them without scheduling provider work.

Lifecycle hook entries use stable `id` values. Set `hooks.disabled = true` to skip
all configured hooks, or `hooks.disabledHooks` / `hooks.disabled_hooks` to skip
individual ids while still recording skipped hook metadata. Deterministic and
replay/resume paths also suppress hook command execution; replay reconstructs the
recorded hook metadata from events only.

Hook command stdout or stderr may emit a redacted JSON effect payload. Supported
`kind` / `effect` values are `allow`, `deny`, `transform_context`,
`request_reminder`, `write_artifact`, `add_diagnostic`, `truncate_output`,
`recover`, and `notify`. Effects are persisted on the hook execution metadata;
`deny` cancels the current coordinator operation through normal cancellation and
failed-tool/failed-compaction events. Effect summaries and hook output summaries
are redacted and capped before persistence. Artifact effects should reference
already-redacted files under the run artifact directory, for example:

Workflow hook policies are typed metadata on those effects, not separate event
writers. Hook JSON may include `policy`, `policy_action`/`decision`, optional
`target`, and `state_affecting`. Recognized policy ids cover
`keyword_alias_detection`, `vague_request_planning_gate`,
`active_context_injection`, `evidence_classification`, `recovery_hint`,
`continuation_policy`, `compaction_preservation`, and
`final_missing_evidence_warning`. When a policy affects workflow state or
provider context, the capped/redacted decision metadata is persisted on the
hook execution record so replay can show the decision without rerunning hooks.

```json
{
  "effects": [
    { "kind": "truncate_output", "summary": "cap grep output to provider budget" },
    {
      "kind": "transform_context",
      "policy": "active_context_injection",
      "decision": "inject_snapshot",
      "workflow_id": "wf_123",
      "summary": "inject active workflow snapshot"
    },
    { "kind": "recover", "summary": "retry missing tool result once" },
    {
      "kind": "write_artifact",
      "summary": "redacted hook details",
      "artifact_ref": { "path": "hooks/example.redacted.json", "digest": "..." }
    }
  ]
}
```

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

Public workflow knobs live under `runtime.workflow`. The first slice keeps these
as staged configuration for command foundations, doctor checks, and future
simulator/signoff policy; replayable workflow state still comes from events and
redacted artifacts:

| Key | Default | Purpose |
| --- | --- | --- |
| `enabled` | `true` | Enables Harness-native workflow command surfaces and doctor readiness checks. Existing replay/status reads remain projection-only even when disabled. |
| `aliases` | `true` | Enables canonical workflow aliases such as `/workflow`, `/signoff`, and `/dossier` in command registries. |
| `projectArtifacts` / `project_artifacts` | `false` | Reserves a future opt-in for writing durable project workflow artifacts outside session runs. |
| `run.defaultLane` / `run.default_lane` | `simulated` | Default workflow lane recorded by `harness workflow run` when `--lane` is omitted. |
| `run.requireDossier` / `run.require_dossier` | `true` | Future signoff policy flag requiring a Run Dossier before terminal success. |
| `run.requireEvidence` / `run.require_evidence` | `true` | Future signoff policy flag requiring mapped evidence or waiver before terminal success. |
| `interview.defaultProfile` / `interview.default_profile` | `standard` | Default profile name for future workflow intake interviews. |
| `interview.threshold` | `0.2` | Ambiguity threshold used by intake gating. |
| `interview.maxRounds` / `interview.max_rounds` | `12` | Maximum interview rounds before handoff or blocker. |
| `planConsensus.maxIterations` / `plan_consensus.max_iterations` | `5` | Maximum critic/planner consensus iterations. |
| `planConsensus.deliberateTriggers` / `plan_consensus.deliberate_triggers` | `auth`, `security`, `migration`, `public api`, `pii` | Terms that should bias future workflow intake toward consensus planning. |
| `workLoop.maxIterations` / `work_loop.max_iterations` | `10` | Default bound for workflow-owned continuation loops. |
| `workLoop.requireManualQa` / `work_loop.require_manual_qa` | `true` | Future signoff policy flag for manual QA evidence. |
| `team.maxMembers` / `team.max_members` | `8` | Maximum declared workflow team size. |
| `team.maxParallelMembers` / `team.max_parallel_members` | `4` | Maximum parallel workflow team members; doctor fails if this exceeds `maxMembers`. |
| `team.tmuxVisualization` / `team.tmux_visualization` | `false` | Future opt-in for tmux visualization diagnostics. |
| `team.worktrees` | `false` | Future opt-in for permissioned workflow worktrees. |
| `goal.requireFinalQualityGate` / `goal.require_final_quality_gate` | `true` | Goal-ledger policy requiring verification/review quality-gate evidence before final completion. |
| `researchLoop.maxIterations` / `research_loop.max_iterations` | `10` | Default bound for future validator-gated research loops. |
| `wiki.enabled` | `false` | Enables future markdown wiki workflow surfaces. |
| `wiki.root` | `.agent-harness/wiki` | Project wiki root when wiki workflows are enabled. |
| `wiki.autoCapture` / `wiki.auto_capture` | `false` | Future opt-in for automatic wiki capture from workflow evidence. |

On successful compaction, checkpoints are written under `artifacts/compactions/<agent_id>/` and recorded in the session event log. Checkpoints and compaction events include additive before/after active-context estimates (`tokens_before_estimate`, `tokens_after_estimate`, summary-token estimate, compacted/preserved turn counts, and estimated reduction) so UIs can report whether compaction helped without treating historical provider spend as active context. Checkpoints also include structured source facts, tail-boundary metadata, summary-source metadata, the summary contract version, replay-derived read/modified file counts, and a timeline entry for replay/UIs. Resume reconstructs provider context from the latest applied checkpoint plus post-checkpoint deltas in `events.jsonl`; the event log itself stays append-only.

Manual `/compact` is a checkpoint command, not a guaranteed immediate token-shrink command: it writes a checkpoint now, summarizes older completed turns, preserves the latest completed turn verbatim, and uses the normal compaction artifact/event format. The success notice reports the active-context estimate delta when available, or says the estimate was unchanged. The default summary contract uses the Harness sections for goal, constraints, progress, key decisions, next steps, and critical context, with operational memory and source facts added as replay-derived context; it is still lossy. Sessions with only one completed turn no-op because there is no older turn to summarize.

Lifecycle hooks may use `event = "compaction_requested"` to observe or cancel compaction. A critical hook failure cancels compaction and records `CompactionFailed`. A successful hook can replace the summary by emitting output prefixed with `compaction_summary:`; hook overrides take precedence over model-backed summaries. Otherwise, model-backed summaries are used only when explicitly enabled, and invalid/empty/failing model output falls back to the deterministic structured summary with `summary_source.deterministic_fallback=true`.

Overflow retry is related but distinct: if the provider rejects a request for context-window reasons, the coordinator may compact and retry once with the checkpointed context when that retry can prove it shrank the provider-visible payload. Estimated pre-prompt compaction uses the same checkpoint path before provider request construction. If a pre-prompt checkpoint cannot reduce the estimated active context, the coordinator records the failure and does not loop on the same turn.

Failed or aborted provider turns can be preserved in active context and checkpoint artifacts. Replay/debug projections keep the incomplete marker, failure stage, and redacted reason so a future provider call does not treat partial assistant output as a completed answer.

Operational memory is derived from persisted events and checkpoint artifacts, not from live filesystem scans. It records capped read-file facts, modified-file facts, compact operation facts, and metadata counts that help operators understand what context survived compaction.

TUI memory or transcript caps are separate presentation settings. They affect what the operator sees on screen, not the persisted provider context used for resume or overflow-retry compaction. The TUI distinguishes active context estimate from cumulative provider tokens spent: active context may decrease after `CompactionApplied`, while total spend remains cumulative and never decreases.
