---
name: git-master
description: Safe git workflow guidance for commits, rebases, and history searches in the Harness workspace.
argument_hint: commit | rebase | history-search
allowed_tools: bash
target_agent: build
target_category: deep
mcp: none
resources: deferred-reference-not-loaded
---

# Git Master

## Purpose

Use git with atomic, reviewable intent while preserving the shared worktree and never rewriting history without explicit operator approval.

## Use When

Use when the operator asks to commit, inspect history, find when code changed, rebase, squash, or prepare a clean branch for review.

## Do Not Use When

Do not use for routine source edits without a git request, destructive cleanup, force pushing, or bypassing hooks.

## Execution Policy

Start with read-only git status/diff/log context. Commit only when explicitly requested. Prefer multiple atomic commits for unrelated concerns. Rebase or force-push only when the operator requested it and the branch safety check allows it.

## Steps

1. Detect mode: commit, rebase, or history-search.
2. Inspect `git status`, staged/unstaged diff, recent log style, branch, and upstream.
3. For commits, group files by atomic behavior and keep tests with their implementation.
4. For rebases, verify the branch is not `main`/`master` and preserve a clean abort path.
5. For history search, use pickaxe/blame/file-log commands and report source-backed findings.

## Tool Usage

Use `bash` for git commands only. Do not use shell text processing for file reads or edits. Destructive git commands need explicit operator confirmation.

## Escalation and Stop Conditions

Stop before amending, resetting, force pushing, or committing secrets unless the operator explicitly approves the exact action.

## Final Checklist

- Worktree status reviewed.
- Commit/message style follows the repository history.
- Hooks were not skipped.
- Final status and next push/PR step are reported.

## Advanced Notes

Stable id: `skill:project:git-master`. This is a disableable built-in capability, not core runtime behavior.
