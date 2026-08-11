# Generic agent and tasks

Harness uses one Pi-style generic coding prompt for interactive turns. There is no selectable primary role, category router, or planning agent. Named subagents remain as bounded extension-style profiles: `explore` for local codebase research, `general` for focused implementation or research, and `librarian` for external documentation and repository research.

Session title generation and provider-context compaction are coordinator-owned internal operations. Their dedicated prompts are not agents, are not configurable through `agent`, do not receive tools, and do not appear in the interactive runtime catalog.

## Generic execution configuration

The top-level `model` selects the model for the generic parent. The `agent.default` object can tune its prompt, variant, sampling, toolset, permission overlay, iteration budget, and tool-failure behavior. Named subagent entries can tune their own bounded prompts and tools. Categories and alternate primary profiles are rejected.

Harness materializes the interactive configuration as `default` so persisted events and coordinator APIs retain a stable execution-profile field. Child tasks record the selected subagent id. Historical event profile strings remain replay data and are never rewritten.

## Permission and toolset boundaries

The coordinator remains the authority for both tool availability and permission decisions. A tool absent from the generic toolset is not advertised to the provider. A denied capability is blocked before execution. Child tasks do not gain permissions from prompt text or from being delegated.

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
