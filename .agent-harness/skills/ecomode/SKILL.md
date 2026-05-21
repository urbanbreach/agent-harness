---
name: ecomode
description: Ecomode deprecated shim
---

# Ecomode deprecated

Hard-deprecated. Do not invoke or route this skill. Use `$ultrawork` directly for maintained high-throughput execution workflows.

## What Ecomode Does

Overrides default model selection to prefer cheaper tiers:

| Default Tier | Ecomode Override |
|--------------|------------------|
| THOROUGH | STANDARD, THOROUGH only if essential |
| STANDARD | LOW first, STANDARD if needed |
| LOW | LOW - no change |

## What Ecomode Does NOT Do

- **Persistence**: Use `ralph` for "don't stop until done"
- **Parallel Execution**: Use `ultrawork` for parallel agents
- **Delegation Enforcement**: Always active via core orchestration

## Combining Ecomode with Other Modes

Ecomode is a modifier that combines with execution modes:

| Combination | Effect |
|-------------|--------|
| `eco ralph` | Ralph loop with cheaper agents |
| `eco ultrawork` | Parallel execution with cheaper agents |
| `eco autopilot` | Full autonomous with cost optimization |

## Ecomode Routing Rules

**ALWAYS prefer lower tiers. Only escalate when task genuinely requires it.**

| Decision | Rule |
|----------|------|
| DEFAULT | Start with LOW tier for most tasks |
| UPGRADE | Escalate to STANDARD when LOW tier fails or task requires multi-file reasoning |
| AVOID | THOROUGH tier - only for planning/critique if essential |

## Agent Selection in Ecomode

**FIRST ACTION:** Before delegating any work, read the agent reference file:
```
Read file: docs/shared/agent-tiers.md
```
This provides the complete agent tier matrix, MCP tool assignments, and selection guidance.

**Ecomode preference order:**

```
// PREFERRED - Use for most tasks
use /prompts:executor for this scoped task
use /prompts:explore for this scoped task
use /prompts:architect for this scoped task

// FALLBACK - Only if LOW fails
use /prompts:executor for this scoped task
use /prompts:architect for this scoped task

// AVOID - Only for planning/critique if essential
use /prompts:planner for this scoped task
```

## Delegation Enforcement

Ecomode maintains all delegation rules from core protocol with cost-optimized routing:

| Action | Delegate To | Model |
|--------|-------------|-------|
| Code changes | executor | LOW / STANDARD |
| Analysis | architect | LOW |
| Search | explore | LOW |
| Documentation | writer | LOW |

### Background Execution
Long-running commands (install, build, test) run in background. Maximum 20 concurrent.

## Token Savings Tips

1. **Batch similar tasks** to one agent instead of spawning many
2. **Use explore (LOW tier)** for file discovery, not architect
3. **Prefer LOW-tier executor routing** for simple changes - only upgrade if it fails
4. **Use writer (LOW tier)** for all documentation tasks
5. **Avoid THOROUGH-tier agents** unless the task genuinely requires deep reasoning

## Disabling Ecomode

Ecomode can be completely disabled via config. When disabled, all ecomode keywords are ignored.

Set in `configured Harness home/.omx-config.json`:
```json
{
  "ecomode": {
    "enabled": false
  }
}
```

## State Management

Use the CLI-first state surface (`harness workflow evidence ... --json`) for ecomode lifecycle state. If explicit MCP compatibility tools are already available, equivalent `omx_state` calls are optional compatibility, not the default.

- **On activation**:
  `harness workflow evidence write --input '{"mode":"ecomode","active":true}' --json`
- **On deactivation/completion**:
  `harness workflow evidence write --input '{"mode":"ecomode","active":false}' --json`
- **On cancellation/cleanup**:
  run `$cancel` (which should call `harness workflow evidence clear --input '{"mode":"ecomode"}' --json`)

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
