# Starter skills

The repository ships a small starter skill pack under `.agent-harness/skills/`.

## Included skills
- `rust-best-practices` — Rust-focused contribution and verification guidance for this workspace.
- `issue-delivery` — issue closeout checklist for docs, verification, and commit/issue hygiene.
- `git-master` — safe git commit, rebase, and history-search workflows.
- `review-work` — post-implementation review orchestration using shipped Harness categories.
- `frontend-ui-ux` — visual engineering guidance for UI/UX polish with deterministic evidence.

## Discovery order
By default the harness searches these project roots, in order:

1. `.agent-harness/skills`
2. `.harness/skills`

It then searches configured global roots such as
`~/.config/agent-harness/skills`. With `skills.walk_to_git_root: true`, project
roots are checked from the current workspace up to the nearest `.git` ancestor;
the nearest matching skill name wins and lower-precedence duplicates are reported
as `shadowed` in the compact catalog.

That means the bundled starter pack is the canonical project-local location. To override a shipped skill, replace the matching directory under `.agent-harness/skills` (for example `.agent-harness/skills/rust-best-practices/SKILL.md`).

## V1 frontmatter

Every skill lives in `<skill-name>/SKILL.md` and starts with frontmatter:

```markdown
---
name: rust-best-practices
description: Baseline Rust guidance for this workspace.
argument_hint: optional short usage hint
allowed_tools: read, grep
target_agent: build
target_category: deep
mcp: deferred-local-metadata
resources: bundled-reference-not-loaded
---
```

Required fields are `name` and `description`. `name` must match the directory
name and use lowercase words separated by single hyphens. Optional V1 fields are
`argument_hint`, `allowed_tools`, `target_agent`, `target_category`, `mcp`,
`resources`, and a string-to-string `metadata` map. CamelCase aliases accepted by
the config reference are also accepted. Unsupported public fields make the skill
catalog entry `malformed` rather than silently changing behavior.

## Extending the pack
- Add new project-local skills under any configured project root.
- Keep frontmatter minimal unless the extra metadata is useful before activation.
- Prefer small, task-specific guidance over long policy dumps.
- Use this body template when adding a durable skill:
  - purpose
  - use when
  - do not use when
  - execution policy
  - steps
  - tool usage
  - escalation or stop conditions
  - final checklist
  - advanced notes or bundled-reference pointers
- If a new skill is referenced from docs/tests/example configs, ship it in-repo so fresh checkouts stay reproducible.

## Progressive disclosure and governance

Catalog, doctor, and support export surfaces expose compact metadata only:
stable id, name, description, source scope, root, location, status, permission
mode, optional V1 metadata, and `body_loaded: false`. Full `SKILL.md` bodies are
loaded only when the `skill` tool activates a loadable skill or `task(load_skills
= [...])` resolves loadable skills before child spawn.

Use `skills.disabled` to turn off skills by name, pattern, or stable id such as
`skill:project:rust-best-practices`. Disabled, denied, malformed, missing, and
symlink-unsafe skills are visible enough to diagnose but cannot load. Metadata
such as `allowed_tools` is descriptive/restrictive only; it never grants tools,
changes a profile toolset, or bypasses coordinator permission checks.

## Built-in skill use-when / do-not-use-when

| Stable id | Use when | Do not use when |
|---|---|---|
| `skill:project:git-master` | The operator asks for commits, rebases, squashes, or history archaeology. | The task is ordinary coding with no git operation requested, or the action would rewrite history without approval. |
| `skill:project:review-work` | Significant changed work needs high-rigor review across goal fit, quality, security, QA, and context. | There is no changed work yet or the edit is trivial enough for direct verification. |
| `skill:project:frontend-ui-ux` | A UI, TUI, layout, typography, color, motion, or visual evidence problem is in scope. | The task is backend-only or provider/session/runtime logic with no visible surface. |

Disable a built-in with `skills.disabled`, for example `"skill:project:git-master"`.

## Using the local runtime config
The repo ships a project-local `./harness.jsonc`, which the CLI auto-discovers
alongside `./harness.json` plus the XDG runtime config paths. TUI-only settings
live separately in `tui.jsonc` / `tui.json`. When both global and local runtime
files exist, the XDG file provides shared defaults and the local file overrides
it. For a fresh checkout, run with:

```bash
cargo run -p harness -- --config harness.jsonc tui
```

The shipped example remains available at `configs/harness.example.jsonc` for
schema/reference validation.
