# Extension strategy

Harness keeps the core small and gives every extension surface explicit authority, configuration, and evidence. Current V1 extension paths are config-backed MCP servers and markdown skills. The typed extension manifest and command/hook seams are final-slice or post-V1 implementation work, not shipped runtime features in this slice.

## Current safe paths

## Config-backed MCP

MCP servers are declared in runtime config under `mcp`. Enabled servers register discovered tools through the native registry. Disabled entries remain documented but do not launch.

## Markdown skills

Markdown skills live under configured skill roots such as `.agent-harness/skills`. Discovery reads frontmatter and compact metadata; full bodies load only when the `skill` tool or `task(load_skills=[...])` activates them. Skills never grant runtime tools or bypass coordinator permissions.

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

The V1 disableable built-in skills write no JSONL or artifact state by themselves. They can change prompt context only after explicit `skill` or `task(load_skills=[...])` activation, and that activity is represented by the existing event schema and tool output summaries. Any future release-blocking built-in that writes JSONL or artifact state must document its `schema_version`, migration policy, and replay behavior before the roadmap box can stay checked. Existing release evidence artifacts document their schemas in the owning surface: event logs in `docs/architecture.md` and `docs/sessions-and-replay.md`, native tool artifacts in `docs/native-tool-catalog.md`, simulation artifacts in `docs/testing.md`, and lane-specific perf/PTY artifacts in `docs/budgets.md` and `docs/testing.md`.

## Deferred seams

The typed extension manifest is final-slice work. It is expected to describe optional tools, prompts, commands, MCP bundles, diagnostics, provider decorators, capability ids, disablement state, and replay-safe extension event rendering. It is not implemented in this slice.

The command/hook seam is final-slice or post-V1 work. Markdown slash-command schemas, `$ARGUMENTS` substitution, command interpolation policy, hook lifecycle execution, hook phases, and migration of built-in lifecycle behavior onto hooks are intentionally not implemented here.

Arbitrary executable plugins, upstream plugin compatibility, browser/media automation, OAuth MCP, server/share/enterprise surfaces, and broad cloud/telemetry/billing features remain post-V1.

## Evidence stance

Every extension surface needs a source of truth, doctor visibility, docs, deterministic tests, and honest failure modes before it can be called release-quality.
