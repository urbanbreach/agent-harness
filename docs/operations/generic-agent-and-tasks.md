# Generic agent and tasks

Harness uses one generic coding prompt for interactive turns. There is no selectable primary role, category router, or planning agent. Named subagents remain as bounded extension-style profiles: `explore` for local codebase research, `general` for focused implementation or research, and `librarian` for external documentation and repository research.

Session title generation and provider-context compaction are coordinator-owned internal operations. Their dedicated prompts are not agents, are not configurable through `agent`, do not receive tools, and do not appear in the interactive runtime catalog.

## Generic execution configuration

The top-level `model` selects the model for the generic parent. The `agent.default` object can tune its prompt, variant, sampling, toolset, permission overlay, iteration budget, and tool-failure behavior. Named subagent entries can tune their own bounded prompts and tools. Categories and alternate primary profiles are rejected.

Harness materializes the interactive configuration as `default` so persisted events and coordinator APIs retain a stable execution-profile field. Child tasks record the selected subagent id. Historical event profile strings remain replay data and are never rewritten.

## Permission and toolset boundaries

The coordinator owns tool availability and permission decisions. Each child uses its own configured `agent.<name>.tools` and `agent.<name>.permission`. A parent's toolset and `task` permission control whether it can start or continue the selected child; the parent's other role restrictions are not inherited. A parent with `edit: "deny"` can delegate implementation to `general` when shared policy permits editing.

For child actions, shared top-level policy is a ceiling: combine the child's role decision with shared policy using **deny first, then ask, then allow**. Existing approvals can satisfy an ask, but cannot override a deny. Primary-agent permission precedence is unchanged. Tools absent from the child's list remain unavailable, including calls inside `batch`.

| Role | Default tools |
|---|---|
| `explore` | `read`, `glob`, `grep`, `list`, `ast_grep_search`, `webfetch`, `websearch`, `session_list`, `session_read`, `session_search`, `session_info`, `batch`, `bash`, `lsp`, `skill` |
| `librarian` | Explore's tools plus `codesearch` |
| `general` | Librarian's native tools except `skill`, plus `edit`, `write`, `apply_patch` |
| `default` | Existing primary tools, including task delegation and skill loading |

Research roles deny native editing, questions, delegation, and todo mutation. Both can run bash commands, use LSP queries, and load skills. Their prompts direct them to research rather than implementation; bash and MCP can still mutate files, so an edit deny is not filesystem confinement. The explicit `lsp.rename` editing tool remains unavailable. Explore has native AST search; Librarian additionally has external `codesearch`.

MCP discovery automatically adds concrete tools from configured servers to `default`, `explore`, and `librarian`, including when the native tool list is customized. General still requires exact MCP IDs in its tool list. Both role and shared policy constrain MCP execution: stdio MCP uses the `bash` capability; HTTP MCP uses network policy. Discovery does not add the generic MCP gateway tools.

The `skill` tool uses read permission and the existing per-skill load policy, independently of task delegation permission. Skills supply instructions; an `allowed_tools` declaration cannot grant tools or permissions. General continues to receive skills through the parent's `load_skills`. Task results report the prepared child's actual toolset, with available tools still subject to argument-specific permission checks. Resume prepares children from current configuration using the same rules as fresh spawn; historical policy snapshots are not restored or rewritten.

Permissions are policy checks, not operating-system confinement. See the [permissions threat model](../permissions/permissions.md).

## Structured delegation body

The `task` tool starts or continues a named subagent. New tasks require `subagent_type`, `prompt`, `run_in_background`, and `load_skills`; continuations use `task_id` or `session_id`. Include these fields in the prompt text when delegating non-trivial work:

| Field | Purpose |
|---|---|
| `context` | What task, files, modules, and constraints the child should know. |
| `goal` | The decision or artifact the child must produce. |
| `downstream use` | How the parent will use the result. |
| `request` | The exact work to perform and output format. |
| `required tools` | Tool classes the child is expected or forbidden to use. |
| `must-do` | Non-negotiable checks or evidence. |
| `must-not-do` | Scope boundaries, forbidden edits, or unavailable capabilities. |

Parent-visible child summaries are capped before they are returned through `task(run_in_background = false)` or `background_output`. The runtime surfaces redacted summary text plus structured truncation metadata, while the child session id and next actions allow explicit continuation or result retrieval.

## Enforcement boundary

The coordinator remains the only authority for event appends, scheduling, permission checks, child ownership, task lifecycle, cancellation, and tool execution. Changing prompt text, docs, or TUI labels cannot grant a capability or bypass policy.
