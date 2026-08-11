---
name: review-work
description: High-rigor post-implementation review workflow using named Harness subagents.
argument_hint: review changed work against goal and evidence
allowed_tools: task, background_output, background_cancel, bash, read, grep
mcp: none
resources:
---

# Review Work

## Purpose

Run a structured post-implementation review that checks goal fit, code quality, security, hands-on QA, and missing context before closeout.

## Use When

Use after significant implementation, refactors, security-sensitive changes, or when the operator asks to verify or review work.

## Do Not Use When

Do not use before there is changed work to review, for trivial one-line edits, or to replace the required test/manual QA evidence.

## Execution Policy

Use real Harness routes only. Launch independent reviewers with `task(subagent_type="explore", ...)` for local constraints and invariants, `task(subagent_type="general", ...)` for implementation quality and hands-on QA, and `task(subagent_type="librarian", ...)` when external documentation or upstream context matters. Set `run_in_background=true` and `load_skills=[]`, collect results with `background_output`, and treat conditional approval as rejection until fixed.

## Steps

1. Summarize the requested goal, changed files, scenarios, and verification already run.
2. Ask `explore` to verify local constraints, invariants, and hidden failure modes.
3. Ask `general` to review implementation quality, maintainability, and hands-on QA.
4. Ask `librarian` to verify external API or upstream claims when the change depends on them.
5. Fix blocking findings, rerun targeted verification, and repeat review only for affected concerns.

## Tool Usage

Use `task(subagent_type=..., run_in_background=true, load_skills=[])` for parallel review tasks and `background_output` after completion. Use `bash` only for git/test status and never to edit files.

## Escalation and Stop Conditions

Stop if a reviewer reports an unresolved correctness, safety, permission, data-loss, or evidence gap. Do not declare completion on "looks good but" feedback.

## Final Checklist

- Goal and scenarios matched.
- All blocking findings fixed.
- Targeted tests and manual surface evidence rerun.
- Review outputs are summarized without bloating parent context.

## Advanced Notes

Stable id: `skill:project:review-work`. Reviewer selection is explicit and limited to shipped named subagents.
