# AGENTS: .agent-harness

## OVERVIEW
Runtime-discovered Harness assets: the generic agent prompt, model-family prompt fragments, shipped skill packages, optional local plans/wiki notes, and generated session state.

Read root `AGENTS.md` first. This file is about runtime assets, not project coding-agent instructions.

## STRUCTURE
```text
.agent-harness/
├── agents/         # generic runtime prompt asset
├── prompt-families/ # model-family prompt fragments loaded into composed prompts
├── skills/          # skill packages with SKILL.md frontmatter
├── plans/           # runtime planning artifacts when present; generated/local
├── wiki/            # local runtime notes/evidence when present
└── sessions/        # generated runtime session data; not source
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Generic prompt | `agents/default.md` | Used by interactive runs. |
| Named subagents | `agents/explore.md`, `agents/general.md`, `agents/librarian.md` | Selected with `task(subagent_type=...)`; follow the Pi extension model rather than primary-role switching. |
| Family prompt fragments | `prompt-families/anthropic.md`, `prompt-families/gemini.md`, `prompt-families/kimi.md`, `prompt-families/trinity.md` | Loaded by model-family prompt composition and drift-tested against snapshots. |
| Config-defined profile metadata | `../configs/harness.example.jsonc`, `../harness.jsonc` | Model, variant, hidden flag, tools, permissions, skill policy. |
| Shipped skills | `skills/*/SKILL.md` | Runtime-loadable skill packages with V1 frontmatter. |
| Skill docs | `../docs/configuration/starter-skills.md` | Discovery order, allowed metadata, malformed/disabled behavior. |
| Generated state | `sessions/`, `sessions/tui/prompt-history.json` | Runtime output; do not edit as source. |

## CONVENTIONS
- `AGENTS.md` files are project instructions; `agents/*.md` files are runtime prompt assets. Keep layers separate.
- Named subagents remain bounded child profiles; capability differences remain coordinator- and permission-owned.
- Prompt-family assets must stay branding-safe and tool-honest; do not claim unavailable browser/editor/task controls.
- Skill `name` must match its directory and use lowercase single-hyphen words.
- Skill catalog/doctor/support-export surfaces expose compact metadata only; full bodies/resources load only on activation.
- Skill frontmatter, resources, and MCP declarations are runtime inputs; avoid secrets and host-specific paths.
- Treat `sessions/`, generated `plans/`, and local evidence/wiki outputs as runtime state unless a test explicitly fixtures them.

## TESTS
```bash
cargo nextest run -p harness --test bootstrap_profiles_test
cargo nextest run -p harness family_prompt
cargo nextest run -p harness-tools --test skill_load_discovery_test
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
```

## ANTI-PATTERNS
- Do not use prompt assets to bypass coordinator permissions or tool capability filtering.
- Do not put project coding-agent instructions into runtime prompt files unless the runtime should load them.
- Do not add prompt-family claims that conflict with the active provider/tool surface.
- Do not reintroduce selectable role or category prompts; internal title and compaction operations own dedicated prompts in coordinator code.
- Do not edit session artifacts, prompt history, or generated plan/evidence artifacts as source.
