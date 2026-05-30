# Agents and subagents

Harness resolves agent/profile/category metadata through the `harness-core::agent_catalog` seam. Doctor JSON and support export consume that catalog directly; task route output and TUI/status/help labels are aligned with the same resolved concepts where this slice touches them, while runtime enforcement remains coordinator-owned.

## Shipped profiles

| Id | Role | Scope |
|---|---|---|
| `build` | primary | Default implementation lane. |
| `plan` | primary | Planning lane; edits are runtime-limited to the active plan file and Plan may delegate only to `explore`. |
| `discipline` | primary | Strict delivery lane with todo/delegation/verification emphasis. |
| `explore` | subagent | Read-only repository lookup and local code search. |
| `general` | subagent | Focused implementation/research child work. |
| `visual-engineering` | category | UI/UX, layout, styling, animation, visual design. |
| `artistry` | category | Complex creative product or implementation work. |
| `ultrabrain` | category | Hard logic, architecture, algorithms, deep debugging. |
| `deep` | category | End-to-end implementation or research. |
| `quick` | category | Small low-risk implementation or cleanup. |
| `unspecified-low` | category | Low-to-moderate fallback for contained uncategorized tasks. |
| `unspecified-high` | category | High-effort fallback for complex uncategorized tasks. |
| `writing` | category | Documentation, prose, and technical writing. |
| `title` | hidden | Session title generation. |
| `summary` | hidden | Session summary generation. |
| `compaction` | hidden | Provider-context compaction summary. |

Each catalog entry carries stable id, display name, role, mode, hidden flag, category binding, display order, prompt asset status/source, model ref plus resolved provider/model/variant, fallback chain metadata, toolset, permission posture, skill metadata, and readiness warnings. The shipped category routes use named `category-*` model profiles in `configs/harness.example.jsonc`, so the local starter preserves OMO-style category scale through GPT-family primary targets plus validated fallback metadata while larger provider catalogs can retarget the same profile names. Runtime execution currently selects the primary target from the profile; automatic provider/model retry is a separate runtime feature.

## Category fallback

Category routing uses ordinary configured profiles. When `task(category = "...")` names an unknown or disabled category, the fallback chain is visible and falls back to `general`, except parent profiles explicitly disabled by policy such as `plan`. Task output reports requested category, resolved profile, fallback chain, permission metadata, model metadata, toolset, loaded skills, and next actions.

## Structured delegation body

The `task` tool prompt is still a string, but V1 guidance recommends a structured delegation body so child context stays useful and reviewable. Include these fields in the prompt text when delegating non-trivial work:

| Field | Purpose |
|---|---|
| `context` | What task, files, modules, and constraints the child should know. |
| `goal` | The decision or artifact the child must produce. |
| `downstream use` | How the parent will use the result. |
| `request` | The exact work to perform and output format. |
| `required tools` | Tool classes the child is expected or forbidden to use. |
| `must-do` | Non-negotiable checks or evidence. |
| `must-not-do` | Scope boundaries, forbidden edits, or unavailable agents. |

Parent-visible child summaries are capped before they are returned through
`task(run_in_background = false)` or `background_output`. The runtime surfaces
redacted `result_summary` / `failure_summary` text capped at 1,200 characters and
a structured `child_summary` object with `kind`, `summary`, `max_chars`,
`original_chars`, and `truncated`. The cap keeps parent context lean while the
child session id and `next_actions` still allow explicit continuation or result
retrieval. Child agents should still prefer structured `answer`, `files`,
`changes`, `verification`, `risks`, and `next_steps` over raw transcript text.

## Enforcement boundary

The catalog is metadata only. Coordinator runtime remains the authority for event appends, scheduling, permission checks, task lifecycle, Plan-to-Explore restrictions, read-only subagent restrictions, and tool execution. Changing a catalog doc or TUI label does not grant tools or bypass policy.
