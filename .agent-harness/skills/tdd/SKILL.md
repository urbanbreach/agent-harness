---
name: tdd
description: TDD deprecated shim
---

# TDD deprecated

Hard-deprecated. Do not invoke or route this skill. Keep test-first discipline inside the active implementation workflow and verify with the project test suite.

## The Iron Law

**NO PRODUCTION CODE WITHOUT A FAILING TEST FIRST**

Write code before test? DELETE IT. Start over. No exceptions.

## Red-Green-Refactor Cycle

### 1. RED: Write Failing Test
- Write test for the NEXT piece of functionality
- Run test - MUST FAIL
- If it passes, your test is wrong

### 2. GREEN: Minimal Implementation
- Write ONLY enough code to pass the test
- No extras. No "while I'm here."
- Run test - MUST PASS

### 3. REFACTOR: Clean Up
- Improve code quality
- Run tests after EVERY change
- Must stay green

### 4. REPEAT
- Next failing test
- Continue cycle

## Enforcement Rules

| If You See | Action |
|------------|--------|
| Code written before test | STOP. Delete code. Write test first. |
| Test passes on first run | Test is wrong. Fix it to fail first. |
| Multiple features in one cycle | STOP. One test, one feature. |
| Skipping refactor | Go back. Clean up before next feature. |

## Commands

Before each implementation:
```bash
# Run the project's test command - should have ONE new failure
```

After implementation:
```bash
# Run the project's test command - new test should pass, all others still pass
```

## Output Format

When guiding TDD:

```
## TDD Cycle: [Feature Name]

### RED Phase
Test: [test code]
Expected failure: [what error you expect]
Actual: [run result showing failure]

### GREEN Phase
Implementation: [minimal code]
Result: [run result showing pass]

### REFACTOR Phase
Changes: [what was cleaned up]
Result: [tests still pass]
```

## External Model Consultation (Preferred)

The tdd-guide agent SHOULD consult Codex for test strategy validation.

### Protocol
1. **Form your OWN test strategy FIRST** - Design tests independently
2. **Consult for validation** - Cross-check test coverage strategy
3. **Critically evaluate** - Never blindly adopt external suggestions
4. **Graceful fallback** - Never block if tools unavailable

### When to Consult
- Complex domain logic requiring comprehensive test coverage
- Edge case identification for critical paths
- Test architecture for large features
- Unfamiliar testing patterns

### When to Skip
- Simple unit tests
- Well-understood testing patterns
- Time-critical TDD cycles
- Small, isolated functionality

### Tool Usage
Prefer native `test-engineer` consultation or CLI-backed ask surfaces when available. Optional MCP compatibility ask tools may be used only when already enabled. If consultation tools are unavailable, fall back to the `test-engineer` agent.

**Remember:** The discipline IS the value. Shortcuts destroy the benefit.

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
