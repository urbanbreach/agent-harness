# Starter skills

The repository ships a small starter skill pack under `.agent-harness/skills/`.

## Included skills
- `rust-best-practices` — Rust-focused contribution and verification guidance for this workspace.
- `issue-delivery` — issue closeout checklist for docs, verification, and commit/issue hygiene.

## Discovery order
By default the harness searches these project roots, in order:
1. `.agent-harness/skills`

That means the bundled starter pack is the canonical project-local location. To override a shipped skill, replace the matching directory under `.agent-harness/skills` (for example `.agent-harness/skills/rust-best-practices/SKILL.md`).

## Extending the pack
- Add new project-local skills under any configured project root.
- Keep frontmatter minimal: `name` and `description`.
- Prefer small, task-specific guidance over long policy dumps.
- If a new skill is referenced from docs/tests/example configs, ship it in-repo so fresh checkouts stay reproducible.

## Using the shipped example config
The repo also ships `configs/harness.example.jsonc`, but the CLI only auto-discovers
`./harness.jsonc` / `./harness.json` plus the XDG runtime config paths. TUI-only
settings live separately in `tui.jsonc` / `tui.json`. When both global and local
runtime files exist, the XDG file provides shared defaults and the local file
overrides it. For a fresh checkout, run with:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc tui
```

or copy the example config to `./harness.jsonc`.
