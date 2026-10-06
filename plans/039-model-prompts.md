# Editable model prompts

Status: Complete, 2026-10-06.

## Requested result

Keep the existing shared system prompt and adapt its behavior for each supported
model. Model entry points must be Markdown files that operators can edit without
rebuilding Harness. Review the reference agent and mode prompt variants before
choosing the adjustments. Verify GPT and GLM 5.3 with the available credentials,
and verify selection and rendering for the remaining presets deterministically.

## Implementation

1. Inventory the reference prompt bodies, shared builders, model routing and eval
   guidance. Record transferable behavior and features Harness does not implement
   in the prompt bundle's source notes.
2. Add model Markdown entry points over the existing shared template. Keep tool
   mechanics, permissions, child roles and project instructions shared. Give
   model-specific behavior and eval wording their own editable files.
3. Resolve named model versions before family defaults. Preserve catalog metadata
   for aliases and rebuild selection on model switches, fallbacks and child turns.
4. Load project overrides from `.agent-harness/prompts`, then user overrides from
   `$XDG_CONFIG_HOME/harness/prompts` or `~/.config/harness/prompts`, then bundled
   files. Re-read on each provider iteration. Preserve explicit literal
   `agent.<name>.system_prompt` overrides. Reject unreadable or invalid selected
   templates instead of silently using a different prompt.
5. Give the system prompt and eval tool description consistent model-specific
   routing guidance. Independent read batches use eval; result-dependent work and
   isolated operations remain direct. Tool visibility and permissions remain
   coordinator policy.
6. Update operator documentation and extend existing provider-boundary coverage
   for editable files, precedence, fallback, model switching and child execution.

## Evidence required

- A complete inventory and comparison of the relevant reference prompt files.
- Every bundled model entry point renders with both full and restricted tools.
- Project and user edits reach provider requests without recompilation.
- GPT and GLM resolve to their own prompts and eval instructions.
- Model switches, provider fallbacks and native children use the actual model.
- Literal overrides, project instructions and command rules keep their contracts.
- Invalid overrides fail visibly, and absent files use the documented fallback.
- Scoped nextest checks, formatting, workspace compilation and relevant quality
  gates pass. Live evidence records actual tool choices separately from scripted
  request-capture checks. Unavailable credentials are reported as a live-test limit.

## Verification

- Added 50 editable model entry points and five eval dialects over the existing
  shared template. The prompt bundle README records the reference file inventory,
  model routing, behavioral differences and adaptation boundaries.
- Every bundled entry point renders with full and restricted tools. Existing
  provider-boundary tests now exercise model aliases, fallback, resume, explicit
  switching, literal overrides and actual child model/role selection.
- Project and user files reach provider requests; an override written during a
  tool call appears on the next request. Ancestor project discovery and bundled
  fallback work. Invalid syntax, empty/oversized files, traversal and escaping
  symlinks fail before a provider request.
- Live GPT-6.1 Sol and GLM 5.3 runs used normal bundled prompts and exposed
  `read,list,grep,bash,eval`. Both used two eval calls for discovery followed by
  independent reads and returned the correct fixture total, 87. For one file,
  GLM used one direct read; GPT used a direct list followed by a direct read.
  An initial GLM run used direct reads after its first batch. Explicitly applying
  the rule to every batch fixed the observed repeat case.
- `cargo fmt --all -- --check`, workspace check, workspace Clippy with all targets
  and features, and all 1,979 deterministic nextest tests passed. The four scoped
  prompt tests also passed after their final test-module move. Static test-suite
  gates passed.
- The repository branding gate still reports two unchanged files:
  `docs/evidence/tui-rewrite/runtime-state/post-measure-host.json` and
  `plans/001-role-scoped-subagents.md`. They have no diff against HEAD and were
  left outside this change.

Local validation output is in `/tmp/harness-prompt-audit-039/validation-final/`
and `/tmp/harness-prompt-audit-039/quality/`. Live scenario output is in
`/tmp/harness-model-boundaries-_v0mhj7y/`; the initial and corrected GLM trials
are in `/tmp/harness-model-prompts-live-72ppw9my/` and
`/tmp/harness-model-prompts-live-zpkzqy98/`. These are temporary local artifacts,
not committed support exports. Other models were not tested against live
providers; their coverage proves selection and rendering, not compliance.
