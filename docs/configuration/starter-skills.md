# Starter skills

The repository ships a small starter skill pack under `.agent-harness/skills/`.

## Included skills

| Skill | Purpose |
| --- | --- |
| `rust-best-practices` | Rust contribution and verification guidance |
| `issue-delivery` | Issue completion, documentation, and commit checks |
| `git-master` | Git commits, rebases, and history searches |
| `review-work` | Review of completed changes |
| `frontend-ui-ux` | UI design and visual verification |
| `harness-qa` | Offline checks through `scripts/harness-qa-dogfood.sh` and optional live smoke through `scripts/harness-qa-live-smoke.sh` |

## Discovery order

By default the harness searches these Harness-owned project roots, in order at
each workspace ancestor:

1. `.agent-harness/skills`
2. `.harness/skills`

It then searches configured non-compatibility global roots such as
`~/.config/agent-harness/skills`. With `skills.walk_to_git_root: true`, project
roots are checked from the current workspace up to the nearest `.git` ancestor;
the nearest matching skill name wins and lower-precedence duplicates are reported
as `shadowed` in the compact catalog. If a config lists extra project roots, the
Harness-owned roots at that ancestor still run before other non-compatibility
roots in the same class; roots in the same class keep their configured order.

To override a shipped skill, replace its directory under `.agent-harness/skills`,
such as `.agent-harness/skills/rust-best-practices/SKILL.md`.

Compatibility roots from other editors or assistants are deliberately not part
of default V1 discovery. `.external-editor/skills`, `.assistant/skills`,
`.agents/skills`, and user-level equivalents are tracked as adapter work and are
ignored unless an operator explicitly adds them to `skills.project_roots` or
`skills.global_roots`. When imported this way, compatibility roots are searched
after Harness-owned and other non-compatibility project/global roots, even if the
compatibility path appears earlier in the config array. A duplicate
`.agents/skills/git-master/SKILL.md` is therefore `shadowed` by
`.agent-harness/skills/git-master/SKILL.md`; if only compatibility roots contain
a skill, project compatibility roots win before global compatibility roots and
configured order breaks ties within that compatibility class.

## V1 frontmatter

Every skill lives in `<skill-name>/SKILL.md` and starts with frontmatter:

```markdown
---
name: rust-best-practices
description: Baseline Rust guidance for this workspace.
argument_hint: optional short usage hint
allowed_tools: read, grep
mcp: deferred-local-metadata
resources: references/usage.md, references/checklist.md
---
```

Required fields are `name` and `description`. `name` must match the directory
name and use lowercase words separated by single hyphens. Optional V1 fields are
`argument_hint`, `allowed_tools`, `mcp`,
`resources`, and a string-to-string `metadata` map. `resources` is a comma- or
newline-separated list of relative files under the skill directory; it is loaded
only on activation, never during catalog discovery. CamelCase aliases accepted by
the config reference are also accepted. Unsupported public fields make the skill
catalog entry `malformed` rather than silently changing behavior.

## Extending the pack

Add project-local skills under a configured project root. Keep frontmatter to
the metadata a reader needs before activation.

Describe the purpose, use when, do not use when, and execution policy in plain
language. Add steps, tool requirements, stop conditions, and a final checklist
only when they help the person or agent using the skill. Link longer references.

If documentation, tests, or example configs reference a skill, include it in the
repository so fresh checkouts can load it.

## Progressive disclosure and governance

Catalog, doctor, and support export output contains compact metadata only:
stable id, name, description, source scope, root, location, status, permission
mode, optional V1 metadata, and `body_loaded: false`. Full `SKILL.md` bodies are
loaded only when the `skill` tool activates a loadable skill or `task(load_skills
= [...])` resolves loadable skills before child spawn. Declared resource files
follow the same activation-only path and are appended under `## Bundled
resources`.

Resource loading is bounded: max 5 files per activation, max 64 KiB per file,
max 200 KiB total loaded bytes, and max path depth 4 under the skill root.
Absolute paths, `..`, globs, directories, and symlink escapes are rejected before
reading. Loaded resource text is redacted before it enters the skill output.

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
| `skill:project:harness-qa` | Product-touching runtime/CLI/tool/scenario changes need offline mock dogfood evidence; optional live smoke when live env is present. | Live without env; tool-matrix ownership via live; freestyle eval as CI proof; PTY/native or simulation-matrix claims from dogfood alone. |

Disable a built-in with `skills.disabled`, for example `"skill:project:git-master"`.

## Using the local runtime config
A project-local `./harness.jsonc` is auto-discovered
alongside `./harness.json` plus the XDG runtime config paths. TUI-only settings
live separately in `tui.jsonc` / `tui.json`. When both global and local runtime
files exist, the XDG file provides shared defaults and the local file overrides
it. To try the shipped example, run:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc tui
```

The shipped example remains available at `configs/harness.example.jsonc` for
schema/reference validation.
