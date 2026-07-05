# AGENTS: .agent-harness

## OVERVIEW
Runtime-discovered harness assets: agent profile markdown, model-family prompt fragments, shipped skill packages, optional local plans/wiki notes, and generated session state.

Read root `AGENTS.md` first. This file is about runtime assets, not project coding-agent instructions.

## STRUCTURE
```text
.agent-harness/
├── agents/         # runtime profile frontmatter and prompt templates
├── prompt-families/ # model-family prompt fragments loaded into composed prompts
├── skills/          # skill packages with SKILL.md frontmatter
├── plans/           # runtime planning artifacts when present; generated/local
├── wiki/            # local runtime notes/evidence when present
└── sessions/        # generated runtime session data; not source
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Primary profiles | `agents/build.md`, `agents/plan.md` | Build is default, Plan is read-only planning. |
| Subagents | `agents/explore.md`, `agents/general.md` | Used by `task(subagent_type=...)`; explore is read-only codebase search. |
| Category profiles | `agents/artistry.md`, `agents/deep.md`, `agents/quick.md`, `agents/ultrabrain.md`, `agents/unspecified-high.md`, `agents/unspecified-low.md`, `agents/visual-engineering.md`, `agents/writing.md` | Used by `task(category=...)`; ordinary toggleable profiles with category model profiles. |
| Family prompt fragments | `prompt-families/anthropic.md`, `prompt-families/gemini.md`, `prompt-families/kimi.md`, `prompt-families/trinity.md` | Loaded by model-family prompt composition and drift-tested against snapshots. |
| Config-defined profile metadata | `../configs/harness.example.jsonc`, `../harness.jsonc` | Model, variant, hidden flag, tools, permissions, skill policy. |
| Shipped skills | `skills/*/SKILL.md` | Runtime-loadable skill packages with V1 frontmatter. |
| Skill docs | `../docs/starter-skills.md` | Discovery order, allowed metadata, malformed/disabled behavior. |
| Generated state | `sessions/`, `sessions/tui/prompt-history.json` | Runtime output; do not edit as source. |

## CONVENTIONS
- `AGENTS.md` files are project instructions; `agents/*.md` files are runtime prompt assets. Keep layers separate.
- Category profiles are ordinary toggleable profiles used by `task(category=...)`; keep the 8 shipped category files aligned with config defaults and README claims.
- Recursive task delegation stays denied unless config/tests change.
- Plan may delegate only to `explore` under shipped policy; do not broaden by prompt text alone.
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
- Do not broaden hidden/internal profiles into visible defaults without docs, examples, tests, and README.
- Do not edit session artifacts, prompt history, or generated plan/evidence artifacts as source.
