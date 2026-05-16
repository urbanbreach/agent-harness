# Starter skills

The repository ships a small starter skill pack under `.agent-harness/skills/`.

## Included skills
- `rust-best-practices` — Rust-focused contribution and verification guidance for this workspace.
- `issue-delivery` — issue closeout checklist for docs, verification, and commit/issue hygiene.
- `git-master` — Git workflow guidance for clean history, safe diffs, and evidence-rich commits.
- `frontend-ui-ux` — frontend implementation guidance for usable, responsive interfaces.
- `review-work` — review protocol for correctness, safety, regression, and test coverage issues.
- `playwright` — browser automation guidance for Playwright-backed MCP sessions and screenshot/interaction evidence.
- `agent-browser` — optional `agent-browser` CLI workflow guidance with dependency diagnostics.
- `dev-browser` — persistent development-browser workflow guidance for iterative UI/debug evidence.
- `ai-slop-remover` — behavior-preserving cleanup guidance for removing vague, duplicated, or over-engineered code.
- `team-mode` — declared-team and active team coordination guidance for mailbox/task/shutdown flows.

## Discovery order
By default the harness searches these project roots, in order:
1. `.agent-harness/skills`
2. `.opencode/skills`
3. `.claude/skills`
4. `.agents/skills`
5. `.harness/skills`

That means the bundled starter pack is the canonical project-local location. To override a shipped skill, replace the matching directory under `.agent-harness/skills` (for example `.agent-harness/skills/rust-best-practices/SKILL.md`).

## Extending the pack
- Add new project-local skills under any configured project root.
- Keep frontmatter explicit. `name` and `description` are required; optional
  `tools`, `permissions`, `commands`, `environment`, `mcp`, and `verification`
  fields describe scoped capabilities and diagnostics without starting external
  processes during discovery.
- Disable all skill loading with `skills.disabled: true`, or disable individual
  skills with `skills.disabled_skills` / `skills.disabledSkills`.
- Prefer small, task-specific guidance over long policy dumps.
- If a new skill is referenced from docs/tests/example configs, ship it in-repo so fresh checkouts stay reproducible.

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
