---
name: omx-setup
description: Setup and configure Agent Harness using current CLI behavior
---

# Harness Setup

Use this skill when users want to install or refresh Agent Harness for the **current project plus user-level Harness directories**.

## Command

```bash
omx setup [--force] [--merge-agents] [--dry-run] [--verbose] [--scope <user|project>] [--plugin|--legacy|--install-mode <legacy|plugin>]
```

If you only want lightweight `AGENTS.md` scaffolding for an existing repo or subtree, use `omx agents-init [path]` instead of full setup.

Supported setup flags (current implementation):
- `--force`: overwrite/reinstall managed artifacts where applicable
- `--merge-agents`: when `AGENTS.md` already exists, preserve user-authored content and insert/refresh Harness-managed generated sections between explicit `<!-- Harness:AGENTS:START -->` / `<!-- Harness:AGENTS:END -->` markers
- `--dry-run`: print actions without mutating files
- `--verbose`: print per-file/per-step details
- `--scope`: choose install scope (`user`, `project`)
- `--plugin`: use Codex plugin delivery for bundled skills while archiving/removing legacy Harness-managed prompts/native agents and keeping setup-owned runtime hooks
- `--legacy`: use legacy setup delivery, overriding any persisted plugin install mode
- `--install-mode`: explicitly choose setup delivery mode (`legacy` or `plugin`); canonical form for scripted setup

## What this setup actually does

`omx setup` performs these steps:

1. Resolve setup scope:
   - `--scope` explicit value
   - else persisted `./.omx/setup-scope.json` (with automatic migration of legacy values)
   - if a TTY user has persisted setup preferences, `omx setup` first summarizes the recorded choices and asks whether to **keep**, **review/change**, or **reset** them
   - else interactive prompt on TTY (default `user`)
   - else default `user` (safe for CI/tests)
2. If scope is `user`, resolve user skill delivery mode:
   - explicit `--plugin`, `--legacy`, or `--install-mode legacy|plugin`, if present
   - persisted install mode in `./.omx/setup-scope.json`, if present and the TTY review decision is `keep`
   - else discovered installed plugin cache under `${CODEX_HOME:-~/.codex}/plugins/cache/**/.codex-plugin/plugin.json` with `name: Agent Harness` makes `plugin` the default
   - else interactive prompt on TTY (`legacy` by default, or `plugin` when a plugin cache is discovered)
   - else default `legacy` unless a plugin cache is discovered
3. Create directories and persist effective scope/install mode
4. In legacy mode, install prompts/native agents/skills and merge full config.toml. In plugin mode, archive/remove legacy Harness-managed prompts/native agents/skills but keep native Codex hooks installed.
5. Verify Team CLI API interop markers exist in built `dist/cli/team.js`
6. Generate AGENTS.md defaults only when selected/allowed (or legacy behavior outside plugin mode)
7. Configure notify hook references outside plugin mode and write `./.omx/hud-config.json`

## Important behavior notes

- `omx setup` prompts for scope when no scope is provided and stdin/stdout are TTY. If `./.omx/setup-scope.json` already exists, setup now summarizes the saved choices first and asks whether to keep them, review/change them, or reset and behave like a fresh setup run.
- Non-interactive setup never blocks for this review prompt: it keeps deterministic CLI/persisted/default behavior for CI and scripted installs.
- In `user` scope, `omx setup` also prompts for skill delivery mode when no prior install mode is kept; installed plugin cache discovery makes plugin mode the default prompt/non-interactive choice.
- Local project orchestration file is `./AGENTS.md` (project root).
- If `AGENTS.md` exists and neither `--force` nor `--merge-agents` is used, interactive TTY runs ask whether to overwrite. Non-interactive runs preserve the file.
- Use `--merge-agents` to keep existing project guidance while allowing setup to refresh Harness-managed AGENTS sections and the generated model capability table idempotently.
- Scope targets:
  - `user`: user directories (`~/.codex`, `~/.codex/skills`, `~/.omx/agents`)
  - `project`: local directories (`./.codex`, `./.codex/skills`, `./.omx/agents`)
- User-scope skill delivery targets:
  - `legacy`: keep installing/updating Harness skills in the resolved user skill root
  - `plugin`: rely on Codex plugin discovery for bundled skills and archive/remove legacy Harness-managed prompts/skills/native agents; setup still installs native Codex hooks and setup-owned runtime feature flags (`hooks = true` on current Codex, legacy `codex_hooks = true` when that is the only reported hook feature, plus `goals = true`) because plugins do not carry hooks or enable external goal context by themselves.
- Migration hint: in `user` scope, if historical `~/.agents/skills` still exists alongside `${CODEX_HOME:-~/.codex}/skills`, current setup prints a cleanup hint. **Why the paths differ**: `${CODEX_HOME:-~/.codex}/skills/` is the path current Codex CLI natively loads as its skill root; `~/.agents/skills/` was the skill root in an older Codex CLI release before `~/.codex` became the standard home directory. Harness writes only to the canonical `${CODEX_HOME:-~/.codex}/skills/` path. When both directories exist simultaneously, Codex discovers skills from both trees and may show duplicate entries in Enable/Disable Skills. Archive or remove `~/.agents/skills/` to resolve this.
- If persisted scope is `project`, `omx` launch automatically uses `CODEX_HOME=./.codex` unless user explicitly overrides `CODEX_HOME`.
- Plugin mode prompts separately for optional AGENTS.md defaults and optional `developer_instructions` defaults. If `developer_instructions` already exists, setup asks before overwriting it; non-interactive runs preserve it.
- With `--force` or `--merge-agents`, AGENTS updates may still be skipped if an active Harness session is detected (safety guard).
- Legacy persisted scope values (`project-local`) are automatically migrated to `project` with a one-time warning.

## Setup-owned configuration surfaces

Use this map when reconciling setup behavior or debugging a confusing install:

| Surface | Owner | Notes |
| --- | --- | --- |
| `./.omx/setup-scope.json` | `omx setup` | Persists setup scope and user-scope skill delivery mode. TTY reruns summarize it and offer keep/review/reset. |
| `~/.codex/config.toml` / `./.codex/config.toml` | `omx setup` generated blocks + user edits | Setup refreshes Harness-managed blocks while preserving supported manual content; setup-owned runtime feature flags include `multi_agent`, `child_agents_md`, the Codex hook feature flag (`hooks` or legacy `codex_hooks`), and `goals`. |
| `~/.codex/hooks.json` / `./.codex/hooks.json` | `omx setup` shared ownership | Setup owns Harness native hook wrappers and preserves user-owned hooks. |
| prompts, skills, native agents | `omx setup` or Codex plugin delivery | Legacy mode installs local files; plugin mode relies on plugin discovery for bundled skills and archives/removes legacy Harness-managed prompt/native-agent copies. |
| `AGENTS.md` | `omx setup` with overwrite safety | Generated defaults or managed refreshes are guarded by force/session checks. |
| `./.omx/hud-config.json` | `omx setup` / `$hud` | Setup creates the focused default; `$hud` can adjust it later. |
| notification hooks | `omx setup` / `$configure-notifications` | Setup wires defaults outside plugin skill delivery; notification skill owns deeper provider configuration. |

## If `$omx-setup` is missing or stale

The source repo ships `skills/omx-setup/SKILL.md` and the catalog marks it active. If Codex does not show `$omx-setup`, treat it as an installation/discovery issue rather than a missing source skill:

1. Run `omx setup --verbose` in the intended scope.
2. Run `omx doctor` and check the reported setup scope, Codex home, skill root, and hook/config status.
3. If using project scope, confirm `./.codex/skills/omx-setup/SKILL.md` exists.
4. If using user scope, confirm `${CODEX_HOME:-~/.codex}/skills/omx-setup/SKILL.md` exists in legacy mode, or that the Agent Harness plugin is installed/discovered in plugin mode.
5. If duplicate/stale skills appear, check for legacy `~/.agents/skills` overlap and follow the cleanup hint printed by setup/doctor.

## Recommended workflow

1. Run setup:

```bash
omx setup --force --verbose
```

2. Verify installation:

```bash
omx doctor
```

3. Start Codex with Harness in the target project directory.

## Expected verification indicators

From `omx doctor`, expect:
- Prompts installed (scope-dependent: user or project)
- Skills installed (scope-dependent: user or project)
- AGENTS.md found in project root
- `Harness workflow projection` exists
- CLI-first config present in the scope target `config.toml`; first-party Harness MCP servers and shared MCP registry sync are omitted by default unless setup was run with `--mcp compat`

## Troubleshooting

- If using local source changes, run build first:

```bash
npm run build
```

- If your global `omx` points to another install, run local entrypoint:

```bash
node bin/omx.js setup --force --verbose
node bin/omx.js doctor
```

- If AGENTS.md was not overwritten during `--force`, stop active Harness session and rerun setup.
- If AGENTS.md was not merged during `--merge-agents`, stop active Harness session and rerun setup.

## Harness substrate override

When this skill is loaded by `agent-harness`, the workflow protocol above is the behavioral source, but the runtime substrate differs from Harness:

- Use coordinator-owned workflow events, workflow projections, task records, and evidence artifacts as the authority.
- Do **not** write or mutate per-mode `Harness workflow projection/*.json` files; lifecycle, phase, continuation, and closeout state are event-sourced by the harness.
- Translate Harness CLI/state operations to harness-native surfaces when needed: workflow evidence/status/goal/wiki CLI commands, native `task`/team tools, and workflow projections.
- Treat native terminal UI-specific Harness team/question instructions as conceptual guidance unless the harness exposes an equivalent native tool; prefer the harness native tool surface.
- Keep final claims evidence-backed: changed files, commands run, artifacts/evidence refs, remaining risks, and the stop condition reached.

## Harness state contract

Harness workflow state is authoritative through coordinator-owned events, workflow projections, native tool artifacts, and recorded workflow evidence. Skills must not require external state files, terminal-pane routing, or upstream CLI lifecycle commands as proof of progress.

## Execution protocol

Use the native Harness command dispatch, question, team, task, evidence, and verification surfaces named by the active workflow. Treat compatibility references as historical context only, and translate them into coordinator-owned actions before acting.

## Evidence and closeout contract

Record material progress as workflow evidence with artifact paths or command output summaries. Close only after the relevant checks pass, pending tasks are resolved or explicitly aborted, and the operator-facing status can be replayed from Harness events.

## Stop/escalation conditions

Stop when the workflow objective is verified complete, cancelled by the operator, or blocked by missing authority. Escalate only for destructive, credentialed, external-production, or materially scope-changing choices.

## Verification checklist

- Native Harness workflow projection reflects the expected mode/status.
- Required evidence artifacts or command summaries are recorded.
- Targeted tests, lint, docs checks, or visual/review gates named by the workflow have fresh results.
- No external state-file, terminal multiplexer, or upstream CLI command is the proof boundary.

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.
