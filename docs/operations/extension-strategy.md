# Extension strategy

Harness supports config-backed MCP servers, markdown skills, and native lifecycle
hooks. A typed extension manifest describes capabilities but does not execute
plugins. Markdown command files and extension command hooks remain unsupported.

| Extension | What it can do | Where to configure it |
| --- | --- | --- |
| MCP server | Register concrete tools from an enabled server | `mcp` in runtime config |
| Markdown skill | Add instructions and declared resources when activated | Configured skill roots |
| Native lifecycle hook | Run an allowed command at a coordinator lifecycle point | Runtime hook config |
| Typed extension manifest | Describe capabilities and static replay metadata | `extension.manifest.v1` descriptors |

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
text is redacted and appended to the normal skill activation body. The catalog,
doctor, and support output expose compact metadata only.

Harness-owned skill roots stay first for V1. External editor/assistant/agent
roots such as `.external-editor/skills`, `.assistant/skills`, and
`.agents/skills` are adapter-deferred and ignored by default; operators may list
them explicitly in `skills.project_roots` or `skills.global_roots`, but that is a
configuration choice rather than a shipped compatibility adapter. Explicitly
listed compatibility roots are imported after Harness-owned and other
non-compatibility roots, so they cannot silently shadow shipped or Harness-owned
skills.

## Native lifecycle hooks and commands

TUI slash commands are built-in UI actions. V1 does not execute markdown command
files, substitute `$ARGUMENTS`, interpolate commands, or inject rules by source
file, glob, priority, or consume policy. These command/hook formats remain
unsupported.

Runtime config lists the native lifecycle hooks. The coordinator owns their execution. Hooks observe lifecycle points through allowlisted commands after
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
| `compaction_written` | native | compaction result | Legacy lifecycle name; current compaction commits `SessionCompaction` without a checkpoint artifact. |
| `compaction_applied` | native | compaction result | Observes context application; replay reads the committed compaction event. |
| `compaction_failed` | native | compaction result | Observes failed compaction without starting a retry loop. |
| `subagent_spawned` | native | subagent lifecycle | Coordinator-owned spawn event and permission rules remain authoritative. |
| `subagent_finished` | native | subagent lifecycle | Coordinator records task/subagent terminal metadata; hooks cannot bypass worker redelegation policy. |
| `permission_requested` | native | permission preflight | Observes a pending permission; hook output cannot grant permission. |
| `permission_resolved` | native | permission result | Observes operator/coordinator decision after resolution; hook output cannot change the recorded decision. |
| `markdown_command_file` | intentionally_unsupported | command loading | No V1 command file schema, `$ARGUMENTS` substitution, or interpolation execution. |
| `rules_context_injection` | intentionally_unsupported | context transform | No V1 rules injection by source file, glob, priority, or consume policy. |
| `typed_extension_command_hook` | post_v1 | extension manifest | Future descriptor/plugin work must route through coordinator permissions, artifacts, and replay-safe metadata first. |
| `fallback_external_plugin_hook` | post_v1 | extension/plugin runtime | Arbitrary executable plugins and upstream command-hook compatibility remain post-V1. |

## Typed extension manifest descriptors

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

## Core runtime behavior vs disableable built-in capabilities

| Surface | Classification | Stable id | Default state |
|---|---|---|---|
| Coordinator event append, scheduling, permissions, lifecycle | core runtime behavior | n/a | enabled |
| Native tool registry | core runtime behavior | n/a | enabled |
| Agent profile prompts | core runtime behavior | n/a | enabled by config |
| `frontend-ui-ux` skill | disableable built-in capability | `skill:project:frontend-ui-ux` | loadable |
| `git-master` skill | disableable built-in capability | `skill:project:git-master` | loadable |
| `harness-qa` skill | disableable built-in capability | `skill:project:harness-qa` | loadable |
| `review-work` skill | disableable built-in capability | `skill:project:review-work` | loadable |

## Built-in capability order and state policy

The coordinator owns event appends and permission checks. The native registry
assigns tool IDs before prompt assembly advertises them. Compaction reads event
and tool context only after those events exist.

Disableable built-in skill rows are sorted by stable id so doctor, docs, and tests
stay deterministic. Skill activation respects the operator-requested
`load_skills` order.

V1 disableable built-in skills write no JSONL or artifact state by themselves.
They can change prompt context only after explicit `skill` or
`task(load_skills=[...])` activation, and that activity is represented by the
existing event schema and tool output summaries. Bundled resources follow the
same activation-only contract and are capped/redacted before they enter the
skill body. A built-in that writes JSONL or artifacts must document its `schema_version`,
migration policy, and replay behavior. Existing release evidence artifacts
document their schemas in the relevant guide: event logs in `docs/architecture/architecture.md` and `docs/architecture/sessions-and-replay.md`, native tool artifacts in
`docs/tools/native-tool-catalog.md`, simulation artifacts in `docs/testing/testing.md`, and
lane-specific perf/PTY artifacts in `docs/testing/budgets.md` and `docs/testing/testing.md`.

## Unsupported extension execution

The typed extension manifest is descriptor-only in V1. Runtime discovery,
extension package loading, executable command hooks, MCP launch from manifests,
provider decorator invocation, and extension-provided tool registration remain
post-V1 until a separate host design proves command mediation, sandboxing,
permissions, artifacts, redaction, and replay safety.

The lifecycle map lists unsupported markdown commands, interpolation, and rules
injection. Existing lifecycle hooks run through the coordinator.

Executable plugins, upstream plugin compatibility, browser and media automation,
OAuth MCP, server hosting, session sharing, enterprise administration, cloud
services, telemetry, and billing remain post-V1.

## Verification requirements

Document each supported extension, its doctor output, its permission checks, and
its failure behavior. Verify those contracts with deterministic tests before
claiming release support.
