---
name: frontend-ui-ux
description: Visual engineering guidance for UI, typography, spacing, motion, and evidence-backed operator polish.
argument_hint: UI surface or visual problem
allowed_tools: read, grep, bash
target_agent: build
target_category: visual-engineering
mcp: none
resources:
---

# Frontend UI UX

## Purpose

Improve visible UI surfaces with strong hierarchy, spacing, typography, color, interaction states, and deterministic visual evidence.

## Use When

Use for TUI rendering, layout, accessibility, visual polish, animation/motion decisions, or UI affordance improvements.

## Do Not Use When

Do not use for backend-only logic, provider protocols, event-store changes, or broad architecture decisions without a visible surface.

## Execution Policy

Start from the existing visual language. Make the smallest UI change that solves the operator problem. Verify with snapshots, TestBackend rendering, PTY evidence, or a browser/terminal surface as applicable.

## Steps

1. Identify the user-visible surface and current interaction state.
2. Read adjacent UI components, theme tokens, keybinding labels, and snapshots.
3. Improve hierarchy, contrast, spacing, or copy without adding unrelated components.
4. Run deterministic render coverage and capture the visible surface.
5. Report the evidence path or snapshot/test name.

## Tool Usage

Use read/search tools for source context and `bash` for focused UI tests. Prefer existing snapshot and PTY lanes over ad hoc screenshots.

## Escalation and Stop Conditions

Stop if the requested visual direction conflicts with accessibility, terminal constraints, or a documented UX invariant; ask one blocking question.

## Final Checklist

- Surface and state identified.
- Existing style matched.
- Keyboard/assistive affordances preserved.
- Deterministic visual evidence captured.

## Advanced Notes

Stable id: `skill:project:frontend-ui-ux`. This is a disableable built-in capability tied to Harness visual evidence posture.
