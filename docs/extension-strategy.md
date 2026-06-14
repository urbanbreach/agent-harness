# Extension strategy

Harness keeps the core small and gives every extension surface explicit
authority, configuration, and evidence. Current V1 extension paths are
config-backed MCP servers, markdown skills, coordinator-owned native lifecycle
hooks, and a descriptor-only typed extension manifest seam. The final-slice
closeout ships the manifest as schema/validation/replay metadata only; markdown
command files and extension/plugin command hooks remain intentionally
unsupported or post-V1 runtime behavior.

## Current safe paths

## Config-backed MCP

MCP servers are declared in runtime config under `mcp`. Enabled servers register discovered tools through the native registry. Disabled entries remain documented but do not launch.

## Markdown skills

Markdown skills live under configured skill roots such as `.agent-harness/skills`.
Discovery reads frontmatter and compact metadata; full bodies and bundled
`resources` files load only when the `skill` tool or `task(load_skills=[...])`
activates them. Skills never grant runtime tools or bypass coordinator
permissions.

Bundled resources use progressive disclosure. The `resources` frontmatter field
is a comma- or newline-separated list of relative file paths under the skill
directory. Directories, globs, absolute paths, `..`, and symlink escapes are
rejected before reading. V1 caps each activation to 5 files, 64 KiB per file, 200
KiB total loaded bytes, and path depth 4 under the skill root. Loaded resource
text is redacted and appended to the normal skill activation body, so catalog,
doctor, and support surfaces still expose compact metadata only.

Harness-owned skill roots stay first for V1. External editor/assistant/agent
roots such as `.external-editor/skills`, `.assistant/skills`, and
`.agents/skills` are adapter-deferred and ignored by default; operators may list
them explicitly in `skills.project_roots` or `skills.global_roots`, but that is a
configuration choice rather than a shipped compatibility adapter. Explicitly
listed compatibility roots are imported after Harness-owned and other
non-compatibility roots, so they cannot silently shadow shipped or Harness-owned
skills.

## Native lifecycle hooks and V1 command stance

V1 command/hook stance: V1 slash commands in the TUI are first-party UI actions, not executable
markdown-defined command files. Markdown command directories, command file
schemas, `$ARGUMENTS` substitution, rules/context glob injection with
session-scoped priority or consume semantics, and command interpolation are
intentionally_unsupported for strict V1 unless a later typed manifest/command
seam re-scopes them. Because markdown command interpolation is unsupported, it
cannot execute during replay.

The shipped hook surface is the coordinator-owned native lifecycle hook list in
runtime config. Hooks observe lifecycle points through allowlisted commands after
the coordinator reaches that point; they do not append events directly, schedule
tasks directly, register tools, resolve permissions, or run during replay.
Critical hook failure fails closed at the coordinator boundary for the owning
operation. Noncritical hook failure records metadata without turning a failed
hook into a successful operation. Deterministic/replay modes suppress live hook
execution while preserving hook metadata already in events.

### Lifecycle phase map

| Hook lifecycle event | V1 status | Runtime boundary | Safety / replay note |
|---|---|---|---|
| `run_started` | native | run lifecycle | Coordinator starts the run and records hook metadata; replay reads prior metadata only. |
| `run_finished` | native | run lifecycle | Coordinator finishes the run after owned work completes; hook failure cannot rewrite prior events. |
| `run_failed` | native | run lifecycle | Coordinator records failure state; hooks observe the terminal failure boundary. |
| `agent_turn_started` | native | message/turn boundary | Coordinator starts a provider turn; hooks cannot inject provider-visible context by side effect. |
| `agent_turn_finished` | native | message/turn boundary | Coordinator finishes the provider turn and records metadata; replay does not execute hooks. |
| `tool_call_started` | native | tool preflight/result | Runs after coordinator permission/scheduling has started the tool lifecycle; edit/bash authority still comes from permission policy. |
| `tool_call_finished` | native | tool preflight/result | Runs at tool completion; critical failure records failed tool metadata and cancels owned task completion. |
| `provider_request_started` | native | provider request params | Runs around provider request construction/execution; provider transport remains owned by the coordinator/provider abstraction. |
| `provider_request_finished` | native | provider request result | Records provider boundary metadata without letting hooks mutate replayed provider output. |
| `compaction_requested` | native | compaction request | Critical failure cancels compaction; successful output may provide `compaction_summary:` under coordinator validation. |
| `compaction_written` | native | compaction result | Observes checkpoint artifact write; event log remains append-only. |
| `compaction_applied` | native | compaction result | Observes active context checkpoint application; replay derives this from recorded events/artifacts. |
| `compaction_failed` | native | compaction result | Observes failed compaction; no retry loop is created by the hook surface. |
| `subagent_spawned` | native | subagent lifecycle | Coordinator-owned spawn event and permission rules remain authoritative. |
| `subagent_finished` | native | subagent lifecycle | Coordinator records task/subagent terminal metadata; hooks cannot bypass worker redelegation policy. |
| `permission_requested` | native | permission preflight | Observes a pending permission; hook output cannot grant permission. |
| `permission_resolved` | native | permission result | Observes operator/coordinator decision after resolution; hook output cannot change the recorded decision. |
| `markdown_command_file` | intentionally_unsupported | command seam | No V1 command file schema, `$ARGUMENTS` substitution, or interpolation execution. |
| `rules_context_injection` | intentionally_unsupported | context transform | No V1 source-file/glob/priority/consume rules injection surface. |
| `typed_extension_command_hook` | post_v1 | extension manifest seam | Future descriptor/plugin work must route through coordinator permissions, artifacts, and replay-safe metadata first. |
| `fallback_external_plugin_hook` | post_v1 | extension/plugin runtime | Arbitrary executable plugins and upstream command-hook compatibility remain post-V1. |

## Typed extension manifest seam (descriptor-only V1)

`ExtensionManifestV1` is a typed descriptor and schema, not a plugin host. The
schema lives at `configs/extension-manifest.v1.schema.json` and uses
`schemaVersion: "extension.manifest.v1"`. It can describe stable extension ids,
capability ids, disablement defaults, optional descriptor arrays for tools,
hooks, commands, prompts, MCP bundles, diagnostics, provider decorators, and
static replay labels/templates.

The V1 parser rejects unknown fields, duplicate capability ids, missing
capability references, unknown hook lifecycle events, dynamic replay text, and
tool descriptors without a public permission name (`bash`, `edit`, `question`,
`task`, `webfetch`, `websearch`, `codesearch`, or `lsp`). Parse/validation
returns descriptor metadata only. It does not discover manifests at runtime,
register tools, execute commands, launch MCP servers, invoke provider
decorators, load external code, or mutate sessions.

Replay support is static metadata rendering: old manifest metadata can be
projected from the stored descriptor fields (extension id, capability ids,
disabled capabilities, descriptor counts, and replay labels) without loading any
extension package or executing extension code. Any future extension-provided
behavior must enter through the existing native registry, coordinator-owned
permission checks, artifact/redaction paths, and replay side-effect boundaries.

Extension tool descriptors declare public permission names, but extension-provided
  tools are not registered or executed in V1 and no runtime permission path
  exists yet.
Replay support for extension manifests is limited to static descriptor/config
  metadata; it does not render extension tool events or load extension code.
Extension-provided tools are not registered or executed in V1; no runtime permission path exists yet.
Replay support is descriptor/config metadata only and does not render extension tool events.

## Core runtime behavior vs disableable built-in capabilities

| Surface | Classification | Stable id | Default state |
|---|---|---|---|
| Coordinator event append, scheduling, permissions, lifecycle | core runtime behavior | n/a | enabled |
| Native tool registry | core runtime behavior | n/a | enabled |
| Agent profile prompts | core runtime behavior | n/a | enabled by config |
| `frontend-ui-ux` skill | disableable built-in capability | `skill:project:frontend-ui-ux` | loadable |
| `git-master` skill | disableable built-in capability | `skill:project:git-master` | loadable |
| `review-work` skill | disableable built-in capability | `skill:project:review-work` | loadable |

## Built-in capability order and state policy

Order is intentional where it affects runtime behavior: coordinator event append and permission checks own authority before native tool registration, native tool registration owns tool ids before agent prompt assembly advertises tool use, and compaction consumes replay-derived event/tool context after those events exist. Disableable built-in skill rows are sorted by stable id so doctor, docs, and tests stay deterministic; skill activation still respects the operator-requested `load_skills` order.

V1 disableable built-in skills write no JSONL or artifact state by themselves.
They can change prompt context only after explicit `skill` or
`task(load_skills=[...])` activation, and that activity is represented by the
existing event schema and tool output summaries. Bundled resources follow the
same activation-only contract and are capped/redacted before they enter the
skill body. Any future release-blocking built-in that writes JSONL or artifact
state must document its `schema_version`, migration policy, and replay behavior
before the roadmap box can stay checked. Existing release evidence artifacts
document their schemas in the owning surface: event logs in `docs/architecture.md` and `docs/sessions-and-replay.md`, native tool artifacts in
`docs/native-tool-catalog.md`, simulation artifacts in `docs/testing.md`, and
lane-specific perf/PTY artifacts in `docs/budgets.md` and `docs/testing.md`.

## Deferred seams

The typed extension manifest is descriptor-only in V1. Runtime discovery,
extension package loading, executable command hooks, MCP launch from manifests,
provider decorator invocation, and extension-provided tool registration remain
post-V1 until a separate host design proves command mediation, sandboxing,
permissions, artifacts, redaction, and replay safety.

Markdown slash-command schemas, `$ARGUMENTS` substitution, command interpolation policy, rules/context file injection, and migration of future extension-provided command hooks onto the typed manifest seam remain intentionally unsupported or post-V1 as labeled in the lifecycle map above. Existing coordinator lifecycle hooks are native runtime hooks, not a plugin host and not a markdown command system.

Arbitrary executable plugins, upstream plugin compatibility, browser/media automation, OAuth MCP, server/share/enterprise surfaces, and broad cloud/telemetry/billing features remain post-V1.

## Evidence stance

Every extension surface needs a source of truth, doctor visibility, docs, deterministic tests, and honest failure modes before it can be called release-quality.
