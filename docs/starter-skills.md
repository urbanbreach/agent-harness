# Starter skills

The repository ships a small starter skill pack under `.agents/skills/`.

## Included skills
- `rust-best-practices` — Rust-focused contribution and verification guidance for this workspace.
- `issue-delivery` — issue closeout checklist for docs, verification, and commit/issue hygiene.

## Discovery order
By default the harness searches these project roots, in order:
1. `.opencode/skills`
2. `.claude/skills`
3. `.agents/skills`

That means the bundled starter pack acts as a safe fallback. To override a shipped skill, add a directory with the same skill name earlier in the search order (for example `.opencode/skills/rust-best-practices/SKILL.md`).

## Extending the pack
- Add new project-local skills under any configured project root.
- Keep frontmatter minimal: `name` and `description`.
- Prefer small, task-specific guidance over long policy dumps.
- If a new skill is referenced from docs/tests/example configs, ship it in-repo so fresh checkouts stay reproducible.

## Using the shipped example config
The repo also ships `configs/harness.example.jsonc`, but the CLI only auto-discovers
`./harness.jsonc` or the XDG config path. For a fresh checkout, run with:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc tui
```

or copy the example config to `./harness.jsonc`.
