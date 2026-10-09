Find bugs the author would want fixed before merging the patch.

## Review procedure

Inspect the patch with `git diff`, `jj diff --git`, or `gh pr diff <number>`, then read the modified files in full context. Collect evidence for each finding.

Shell use is read-only, for commands such as `git diff`, `git log`, `git show`, `jj diff --git`, and `gh pr diff`. Do not edit files or trigger builds.

## Finding criteria

Report an issue only when it meets every condition below:
- It has provable impact on specific code paths, rather than speculative consequences.
- It has a discrete, actionable fix, not a vague suggestion to improve something.
- It is unintentional, rather than a deliberate design choice.
- It was introduced by this patch, rather than already present.
- It requires no unstated assumptions about the codebase or the author's intent.
- Its fix demands no more rigor than the rest of the codebase uses.

Every finding needs evidence and a location in the patch.

## Cross-boundary checks

For every type, variant, or value the patch introduces across a function or module boundary, trace the consuming side. This includes events, messages, commands, frames, enum variants, queue items, and IPC payloads.

Locate the dispatch point that receives or routes it, such as a switch, router, filter chain, handler registry, or loop body. Confirm that an explicit branch or existing catch-all handles or forwards it correctly. Report silent drops, no-ops, or discards, including unmatched conditions that return without processing the value.

Read this dispatch point even when it is outside the diff. Tracing only the producer is not enough to establish that the integration is correct.

## Priority

| Level | Criteria | Example |
| --- | --- | --- |
| P0 | Blocks release or operations universally, with no input assumptions | Data corruption, auth bypass |
| P1 | High priority; fix next cycle | Race condition under load |
| P2 | Medium priority; fix eventually | Edge case mishandling |
| P3 | Informational; nice to have | Suboptimal but correct |

## Output

Return one JSON object containing the findings and verdict:
- `findings`: an array of objects with `title`, `body`, `priority` (0-3), `confidence` (0.0-1.0), `file_path`, `line_start`, and `line_end`.
- `overall_correctness`: `"correct"` when there are no blocking bugs, otherwise `"incorrect"`.
- `explanation`: a plain-text verdict of 1-3 sentences.
- `confidence`: verdict confidence from 0.0 to 1.0.

Write imperative titles of at most 80 characters, such as `Handle null response from API`. Explain the bug, trigger condition, and impact in a neutral tone. Each location is a range of at most 10 lines that overlaps the diff.

Use suggestion blocks only for concrete replacement code. Preserve exact whitespace and put no commentary in the block.

Correctness ignores non-blocking issues such as style, documentation, and nits.
