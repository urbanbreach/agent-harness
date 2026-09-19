# Plan 005: Redact tool artifacts before writing them to disk

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-core/src/tool.rs crates/harness-core/src/redact.rs crates/harness-core/src/coord/task_lifecycle.rs crates/harness-tools/src/fs_read.rs crates/harness-tools/src/fs_read/window.rs crates/harness-tools/src/fs_read/tests.rs crates/harness-tools/src/shell_run.rs crates/harness-tools/src/hashline_apply.rs crates/harness-tools/src/ast_grep.rs crates/harness-tools/tests/hashline_apply_test.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE (2026-09-20)
- **Audit finding**: 4 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: M
- **Risk**: HIGH
- **Depends on**: None
- **Category**: security
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/228

## Why this matters

Event-envelope redaction does not protect separately written artifacts. Read spills, command output, edit diffs, edit before-images and formatter-refreshed diffs currently contain raw tool content. Apply the existing redaction policy before persistence and compute digests from the bytes actually stored, without corrupting workspace files or growing the bounded read path into a whole-file buffer.

## Current state

- [crates/harness-core/src/tool.rs:445](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/tool.rs#L445) — Common ArtifactStore::write_text writes and hashes the supplied raw string.
- [crates/harness-tools/src/fs_read.rs:335](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/fs_read.rs#L335) — Read spills bypass the store and write formatted source lines directly.
- [crates/harness-tools/src/shell_run.rs:599](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/shell_run.rs#L599) — Command overflow uses the common text writer.
- [crates/harness-tools/src/hashline_apply.rs:537](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L537) — Edit diff and before-image persistence.
- [crates/harness-tools/src/ast_grep.rs:806](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/ast_grep.rs#L806) — Another diff producer must redact before line-prefix generation.
- [crates/harness-core/src/coord/task_lifecycle.rs:946](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/task_lifecycle.rs#L946) — Formatter refresh reads before-images and writes a replacement diff directly.
- [crates/harness-core/src/coord/snapshot.rs:101](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/snapshot.rs#L101) — Existing restore snapshots deliberately omit content changed by redaction.
- [crates/harness-core/src/redact.rs:108](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/redact.rs#L108) — Existing text redaction policy; structured objects use redact_value.

[crates/harness-core/src/tool.rs:460](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/tool.rs#L460):

```rust
        fs::write(&target, contents.as_bytes()).map_err(|source| {
            ArtifactStoreError::WriteFile {
                path: target.display().to_string(),
                source,
            }
        })?;

        let digest = blake3::hash(contents.as_bytes()).to_hex().to_string();
```

[crates/harness-tools/src/fs_read.rs:348](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/fs_read.rs#L348):

```rust
        let visible_line = truncate_fs_read_line(&line);
        let rendered = if render.hashline_anchors {
            let anchor = build_fs_read_line_anchor(line_number, &line);
            format_fs_read_hashline_line(&anchor, &visible_line)
        } else {
            format_fs_read_output_line(&visible_line, line_number, render)
        };
        artifact_writer.write_rendered_line(&rendered)
    })?;

    Ok(ArtifactRef {
        path: target.artifact_path,
```

[crates/harness-tools/src/hashline_apply.rs:548](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-tools/src/hashline_apply.rs#L548):

```rust
fn write_before_artifact(
    ctx: &ToolContext,
    edit_id: &str,
    before: &str,
) -> Result<harness_core::tool::ArtifactRef, ToolError> {
    ctx.artifact_store()
        .tool_err("failed to access artifact store")?
        .write_text(&format!("edit-{edit_id}.before"), before)
        .tool_err("failed to write before artifact")
}
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-core/src/coord/snapshot.rs:109](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/coord/snapshot.rs#L109):

```rust
        let content = String::from_utf8(bytes)
            .ok()
            .filter(|text| !text.contains('\0'))
            .filter(|text| redactor.redact_text(text) == *text)
            .filter(|text| {
                // Structured config credentials must not hide inside a JSON string field.
                json5::from_str::<serde_json::Value>(text)
                    .ok()
                    .is_none_or(|value| crate::redact::redact_value(redactor, &value) == value)
            });
```

Relevant design contract: [docs/permissions/privacy-and-local-data.md:13](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/docs/permissions/privacy-and-local-data.md#L13).

The privacy guide says redaction is implemented in harness-core/src/redact.rs. Reuse those patterns and markers; do not introduce a second secret policy. Workspace content, read stamps, edit anchors and content digests used for editing must continue to describe the actual source. Only the durable diagnostic representation is redacted. Snapshot recovery intentionally protects sensitive content rather than restoring redacted substitutes. Raw provider/MCP media policy and shell capture budgets are separate findings.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(fs_read) \| test(bash_spills)'` | Selected tests pass after the repair; selection must not be empty. |
| Shared redaction/store checks | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(redact) \| test(artifact_store) \| test(diff_helper_tests)'` | All selected tests pass. |
| Public edit artifacts | `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` | All selected tests pass. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-core -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-core/src/tool.rs`
- `crates/harness-core/src/redact.rs`
- `crates/harness-core/src/coord/task_lifecycle.rs`
- `crates/harness-tools/src/fs_read.rs`
- `crates/harness-tools/src/fs_read/window.rs`
- `crates/harness-tools/src/fs_read/tests.rs`
- `crates/harness-tools/src/shell_run.rs`
- `crates/harness-tools/src/hashline_apply.rs`
- `crates/harness-tools/src/ast_grep.rs`
- `crates/harness-tools/tests/hashline_apply_test.rs`

Administrative updates are limited to execution status/evidence in `plans/005-redact-persisted-tool-artifacts.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-005-redact-persisted-tool-artifacts`.
- Keep this repair in one logical change; if instructed to commit, use `fix(artifacts): redact tool content before persistence`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Make the common text persistence boundary safe

In ArtifactStore::write_text, redact before writing and hash the stored redacted bytes. Reuse DefaultRedactor for text; when the body is a complete structured JSON/JSON5 value, apply redact_value before serializing so opaque values under sensitive keys retain key context. Preserve non-sensitive text byte-for-byte where no redaction applies. Extend the existing artifact-store test to read persisted bytes and verify its digest. Do not add a raw-write escape hatch. Inventory every write_text caller with rg; existing callers that already redact must remain harmless under repeated redaction.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(redact) | test(artifact_store)'` → Tests verify redacted on-disk bytes and matching digests, while ordinary artifact content is unchanged.

### Step 2: Redact streamed file output before decoration

Update write_fs_read_artifact_streaming so it redacts raw lines before truncation, line-number prefixes or hashline presentation. Retain bounded streaming. For multi-line private-key blocks, use a small shared stateful redaction path in redact.rs that recognizes the existing BEGIN/END private-key boundary syntax, emits a marker and suppresses block contents without buffering the block. An unterminated block stays suppressed through EOF. When the requested offset begins after the first line, scan earlier lines for redaction state without emitting them, so an offset inside a block cannot expose its suffix. Hashline anchors still derive from the original line, never the redacted display. Reuse ordinary DefaultRedactor text handling outside blocks. Do not promise detection of arbitrary opaque secrets in unstructured prose.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(fs_read)'` → Existing offset, truncation and source-anchor tests pass; extended spill cases omit synthetic token and multi-line block content, including an offset into a block.

### Step 3: Keep edit and formatter artifacts consistent

Redact before/after text before producing durable diffs in hashline_apply and ast_grep; line prefixes must not split the input to multi-line redaction. Store redacted before-images and document their diagnostic-only use. The traced production consumer of before_rel_path is refresh_formatted_diffs: redact the formatted source with the same policy before comparing it with the stored before-image, then write the replacement through ArtifactStore rather than std::fs::write and use the returned digest. Preserve actual workspace writes and actual new-file digests. Add one local test in task_lifecycle.rs for formatter diff refresh using temporary before/after files; a real formatter process is not required.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(diff_helper_tests)'` → The formatter-refresh test checks the saved diff and digest, and no raw synthetic secret is persisted. Non-sensitive formatting changes remain visible.

### Step 4: Extend public spill and edit behavior checks

Extend fs_read_adds_truncation_marker_and_spills_full_output_artifact, bash_spills_large_output_to_artifact_for_event_stability, and the existing successful hashline edit test. Use synthetic marker-bearing content that is guaranteed to trigger existing policy, inspect the actual artifact files, and retain ordinary-content controls. Verify a structured artifact's opaque sensitive-key value at the common writer. Do not assert merely that an artifact filename contains redacted.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(fs_read) | test(bash_spills)'` → Existing and extended read/shell cases pass. Also run the public edit-artifact command in the command table; it must pass.

### Step 5: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Extend the existing spill tests rather than creating a large new privacy fixture suite. Cover the distinct sinks: common writer with structured key context and digest, bounded file streaming including multi-line/offset behavior, public edit artifacts, and formatter rewrite. All content is synthetic. Check persisted bytes, not only event summaries. The real edited file must retain its intended unredacted content.

## Done criteria

All must hold:

- [x] Read, shell, edit, before-image and formatter artifacts omit synthetic secret material covered by the existing redaction policy.
- [x] Artifact digests match the persisted bytes after redaction and formatter refresh.
- [x] File reads remain streamed; source anchors, edit content and actual file digests remain correct.
- [x] No raw temporary spill file or opt-out persistence API is introduced.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(fs_read) | test(bash_spills)'` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-core -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- Any before-image consumer is found to restore or apply its bytes to workspace content. Do not substitute redacted text into a recovery path; report and narrow the design.
- The read change buffers the whole file/block, writes raw staging data, or requires a new artifact format/framework.
- Redaction causes a non-sensitive source change, invalidates actual edit anchors, or turns an artifact failure into a false successful file rollback.

## Maintenance notes

Keep every artifact writer on this persistence policy, including formatter rewrites. Add a caller-specific regression only for a genuinely distinct formatting/streaming boundary. Snapshot recovery remains digest-protected and outside this plan; arbitrary encoded MCP media belongs to finding 18.

## Execution evidence — 2026-09-20

Implemented for issue #228 on `codex/plan-005-redact-persisted-tool-artifacts`.
The prescribed drift check from `2e342840` produced no source differences.

- `ArtifactStore::write_text` redacts text and complete JSON/JSON5 values before
  writing, and hashes the stored bytes. Ordinary content is preserved; repeated
  redaction retains valid structured content, including cookie-bearing strings.
- Read spills redact each raw line before truncation or decoration. The scanner
  visits skipped lines to retain private-key state, suppresses unterminated blocks
  through EOF, and preserves surrounding ordinary text and source-derived anchors.
  Memory remains bounded by a line; no raw staging file is created. Read spills
  retain their existing optional-digest behavior (`None`).
- Hashline and ast-grep diffs redact source text before diff generation. Diagnostic
  before-images use the same policy. Formatter refresh redacts its inputs and
  writes through the common store; workspace bytes and file digests remain raw.
- The caller inventory covered all common text writers. Before-image consumers are
  formatter refresh and TUI display; none restore their bytes to workspace files.
- Extended existing store, read, shell and public edit tests, plus one formatter
  refresh test. The store and formatter regressions failed before the fix (2/2),
  demonstrating raw-secret persistence. All synthetic-secret checks now pass.

| Verification command | Result |
|---|---|
| `cargo check --workspace --locked --offline` | PASS, exit 0 |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --lib -E 'test(fs_read) \| test(bash_spills)'` | PASS, 16 tests, exit 0 |
| `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(redact) \| test(artifact_store) \| test(diff_helper_tests)'` | PASS, 25 tests, exit 0 |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test hashline_apply_test` | PASS, 4 tests, exit 0 |
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test native_ast_grep_replace_test` | PASS, 4 tests, exit 0 |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo clippy -p harness-core -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | PASS, exit 0 |
| `git diff --check` | PASS, exit 0 |

The first integration build caught an attempted read-spill hash using a test-only
`blake3` dependency. That optional addition was removed; the successful rerun and
workspace checks above verify the production build without dependency changes.
There are no remaining required-gate blockers. Broader historical gates were not
rerun and are not claimed green.

The changed-path and untracked-file inventories were checked against the initial
state. Only the nine scoped source/test paths, this plan, and its index entry are
staged. Existing `harness.jsonc`, unrelated planning edits/files, and
`20260906-192230/` remain outside the commit.
