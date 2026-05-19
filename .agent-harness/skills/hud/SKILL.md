---
name: "hud"
description: "Show or configure the Harness HUD (two-layer statusline)"
role: "display"
scope: ".omx/**"
---

# HUD Skill

The Harness HUD uses a two-layer architecture:

1. **Layer 1 - Codex built-in statusLine**: Real-time TUI footer showing model, git branch, and context usage. Configured via `[tui] status_line` in `~/.codex/config.toml`. Zero code required.

2. **Layer 2 - `omx hud` CLI command**: Shows Harness-specific orchestration state (ralph, ultrawork, autopilot, team, pipeline, ecomode, turns). Reads `Harness workflow projection/` files.

## Quick Commands

| Command | Description |
|---------|-------------|
| `omx hud` | Show current HUD (modes, turns, activity) |
| `omx hud --watch` | Live-updating display (polls every 1s) |
| `omx hud --json` | Raw state output for scripting |
| `omx hud --preset=minimal` | Minimal display |
| `omx hud --preset=focused` | Default display |
| `omx hud --preset=full` | All elements |

## Presets

### minimal
```
[Harness] ralph:3/10 | turns:42
```

### focused (default)
```
[Harness] ralph:3/10 | ultrawork | team:3 workers | turns:42 | last:5s ago
```

### full
```
[Harness] ralph:3/10 | ultrawork | autopilot:execution | team:3 workers | pipeline:exec | turns:42 | last:5s ago | total-turns:156
```

## Setup

`omx setup` automatically configures both layers:
- Adds `[tui] status_line` to `~/.codex/config.toml` (Layer 1)
- Writes `.omx/hud-config.json` with default preset (Layer 2)
- Default preset is `focused`; if HUD/statusline changes do not appear, restart Codex CLI once.

## Layer 1: Codex Built-in StatusLine

Configured in `~/.codex/config.toml`:
```toml
[tui]
status_line = ["model-with-reasoning", "git-branch", "context-remaining"]
```

Available built-in items (Codex CLI v0.101.0+):
`model-name`, `model-with-reasoning`, `current-dir`, `project-root`, `git-branch`, `context-remaining`, `context-used`, `five-hour-limit`, `weekly-limit`, `codex-version`, `context-window-size`, `used-tokens`, `total-input-tokens`, `total-output-tokens`, `session-id`

## Layer 2: Harness Orchestration HUD

The `omx hud` command reads these state files:
- `Harness workflow projection/ralph-state.json` - Ralph loop iteration
- `Harness workflow projection/ultrawork-state.json` - Ultrawork mode
- `Harness workflow projection/autopilot-state.json` - Autopilot phase
- `Harness workflow projection/team-state.json` - Team workers
- `Harness workflow projection/pipeline-state.json` - Pipeline stage
- `Harness workflow projection/ecomode-state.json` - Ecomode active
- `Harness workflow projection/hud-state.json` - Last activity (from notify hook)
- `.omx/metrics.json` - Turn counts

## Configuration

HUD config stored at `.omx/hud-config.json`:
```json
{
  "preset": "focused"
}
```

## Color Coding

- **Green**: Normal/healthy
- **Yellow**: Warning (ralph >70% of max)
- **Red**: Critical (ralph >90% of max)

## Troubleshooting

If the TUI statusline is not showing:
1. Ensure Codex CLI v0.101.0+ is installed
2. Run `omx setup` to configure `[tui]` section
3. Restart Codex CLI

If `omx hud` shows "No active modes":
- This is expected when no workflows are running
- Start a workflow (ralph, autopilot, etc.) and check again

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
