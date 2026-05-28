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
| `unspecified-low` | category | Low-effort fallback for uncategorized tasks. |
| `unspecified-high` | category | High-effort fallback for complex uncategorized tasks. |
| `writing` | category | Documentation, prose, and technical writing. |
| `title` | hidden | Session title generation. |
| `summary` | hidden | Session summary generation. |
| `compaction` | hidden | Provider-context compaction summary. |

Each catalog entry carries stable id, display name, role, mode, hidden flag, category binding, display order, prompt asset status/source, model ref plus resolved provider/model/variant, fallback chain, toolset, permission posture, skill metadata, and readiness warnings.

## Category fallback

Category routing uses ordinary configured profiles. When `task(category = "...")` names an unknown or disabled category, the fallback chain is visible and falls back to `general`, except parent profiles explicitly disabled by policy such as `plan`. Task output reports requested category, resolved profile, fallback chain, permission metadata, model metadata, toolset, loaded skills, and next actions.

## Enforcement boundary

The catalog is metadata only. Coordinator runtime remains the authority for event appends, scheduling, permission checks, task lifecycle, Plan-to-Explore restrictions, read-only subagent restrictions, and tool execution. Changing a catalog doc or TUI label does not grant tools or bypass policy.
