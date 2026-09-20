# Plan 019: Remove encoded MCP media from durable tool results and support exports

> **Executor instructions:** Follow the ordered steps and their verification commands. Honor the STOP conditions, preserve unrelated work, and update this plan and its row in `plans/README.md` when finished. This is an implementation handoff; publication does not mean the fix has been implemented.
>
> **Drift check — run first:** `git diff --stat 7f5a7ec6cba58cf82e95800b137d97bb056f7fc4..HEAD -- crates/harness-tools/src/mcp.rs crates/harness-tools/src/mcp_render.rs crates/harness-core/src/redact.rs crates/harness/src/sessions/export/redaction.rs crates/harness-tools/tests/mcp_generic_test.rs crates/harness-tools/tests/common/mcp_server.rs crates/harness/tests/replay_sessions_cli/08b_sessions_export_redaction_artifacts_test.rs`. Also run `git status --short` and compare the excerpts below with the working tree. Reconcile expected prerequisite edits before implementation; stop on unexplained behavior changes. If this local baseline commit is unavailable in the executor checkout, use the inlined excerpts to establish an equivalent baseline before proceeding.

## Status

- **Execution:** IMPLEMENTED — focused verification PASS; integrated gates/review pending (2026-09-20)
- **Issue:** [#242](https://github.com/urbanbreach/agent-harness/issues/242)
- **Priority:** P1
- **Effort:** M
- **Risk:** MED
- **Depends on:** plan 005 (`plans/005-redact-persisted-tool-artifacts.md`, [issue #228](https://github.com/urbanbreach/agent-harness/issues/228))
- **Category:** security
- **Audit finding:** 18 of the 2026-09-18 deep audit
- **Planned at:** commit `7f5a7ec6cba58cf82e95800b137d97bb056f7fc4`, 2026-09-20
- **Publication request:** Explicitly authorized for the remaining findings, 2026-09-20.
- **Planning evidence:** Source and existing test inspection at this local revision. Proposed checks and regressions below have not been run or implemented by the advisor. This document is self-contained; it does not require a GitHub blob link to the local commit.

## Why this matters

MCP tool, resource and prompt results retain their raw JSON, including encoded images/audio/resource blobs. Generic text redaction cannot inspect those bytes, and fallback rendering can expose them. Omit recognized media payloads before durable serialization and sanitize historical copies during support export.

## Current state

`crates/harness-tools/src/mcp.rs:496` — The complete remote result is placed in durable structured output.

```rust
        Ok(self.tool_result(
            display_text,
            json!({
                "tool": tool_name,
                "arguments": arguments,
                "result": result,
            }),
            &metadata,
        ))
```

`crates/harness-tools/src/mcp_render.rs:33` — Unknown content falls back to compact JSON, which includes payload bytes.

```rust
        Some("image") => Some(format!(
            "[image {}]",
            entry
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("unknown mime")
        )),
        Some("resource") => entry
            .get("resource")
            .and_then(render_resource_entry)
            .or_else(|| Some(compact_json(entry))),
        _ => Some(compact_json(entry)),
```

## Conventions and exemplar

Plan 005 provides redacted artifacts; preserve it. Add one narrow protocol-aware JSON normalization helper in harness-core::redact for tools and CLI to share. Only recognized MCP content envelopes may lose binary data; arbitrary application keys named data/blob must survive. Keep text and safe MIME/type metadata. Provider content is transient via the existing skipped-serialization field; no new media feature or binary secret scanner.

`crates/harness/src/sessions/export/redaction.rs:78` — Support export already sanitizes a copied value before redaction and final scanning.

```rust
fn sanitized_support_export_value(export: &SessionExportBundle) -> serde_json::Result<Value> {
    let mut value = serde_json::to_value(export)?;
    let removed = remove_provider_reasoning_delta_events(&mut value);
    remove_provider_reasoning_delta_replay_counts(&mut value, removed);
    Ok(value)
```

Follow the workspace's Rust 2021 conventions, explicit errors and existing injected test seams. Coordinator authority, append-only durable history, offline replay/readiness and pure TUI projection remain contracts. Extend meaningful public-boundary coverage where possible; do not add trivial or duplicate tests. Use nextest rather than cargo test.

## Commands you will need

Run from the repository root with the existing stable Rust toolchain and locked dependencies. No Rust dependency addition or lockfile update is part of this plan. A filtered test run selecting zero cases is not verification.

| Purpose | Command | Expected result |
|---|---|---|
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` | Expected results are specified per step; final run passes with nonzero selection. |
| Compile | `cargo check -p harness-tools --locked --offline` | Exit 0. |
| Rust formatting | `cargo fmt --all -- --check` | Exit 0; check mode only. |
| Scoped lint | `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; report independent baseline failures separately. |
| Export compatibility | `cargo nextest run --profile ci --locked --offline -p harness --test replay_sessions_cli_test -E 'test(sessions_export)'` | All selected cases pass. |
| Cross-crate compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Whitespace | `git diff --check` | Exit 0. |
| Scope | `git status --short` | Only the explicitly allowed implementation files and plan records are changed by this work. |

## Scope

**In scope — only these implementation files may change:**

- `crates/harness-tools/src/mcp.rs`
- `crates/harness-tools/src/mcp_render.rs`
- `crates/harness-core/src/redact.rs`
- `crates/harness/src/sessions/export/redaction.rs`
- `crates/harness-tools/tests/mcp_generic_test.rs`
- `crates/harness-tools/tests/common/mcp_server.rs`
- `crates/harness/tests/replay_sessions_cli/08b_sessions_export_redaction_artifacts_test.rs`

Administrative updates to `plans/019-strip-mcp-media-from-durable-state.md` and `plans/README.md` are also allowed.

**Out of scope:** all other files; unrelated refactors; new dependencies; new event schemas; rewriting existing session histories; credential changes; live-service or native signoff unless explicitly authorized separately. Preserve the bounded behavior and exclusions in the steps; do not expand scope to make an unrelated baseline failure disappear.

## Git workflow

Use an isolated executor checkout or a dedicated `codex/plan-019-strip-mcp-media-from-durable-state` branch without switching or resetting the user's active dirty checkout. Keep one focused logical change; existing commit style includes `fix(config): retain authored permission rule precedence`. Do not commit to the user's branch, push, or open a PR unless instructed by the operator.

## Steps

### Step 1: Extend the existing MCP fixture with media envelopes

Add table-driven tool result, resource contents and prompt-message cases containing synthetic image/audio/resource bytes plus neighboring ordinary text and application data. Assert display output and serialized structured results omit only the recognized encoded payloads. Use a legacy ToolCallFinished export fixture with the same envelopes and assert source journal bytes are unchanged.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` → Raw encoded-payload assertions fail on the baseline; ordinary text/application data remain part of the expected result.

### Step 2: Normalize each MCP persistence entry point

Implement the narrow helper in core/redact.rs and apply it before display rendering and ToolResult structured_json construction for tools/call, resources/read and prompts/get. For recognized image/audio/resource media emit a stable omitted-media marker with safe metadata. Cover nested resource content without recursively deleting arbitrary keys. Preserve existing textual failure handling; do not use compact_json on raw unsupported media.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` → All MCP result paths omit encoded bytes from display and serialized output while preserving neighboring ordinary data.

### Step 3: Sanitize legacy support-export copies

Apply the same protocol-aware transformation to historical MCP result envelopes in sanitized_support_export_value before the existing redactor and fail-closed scanner. Do not mutate stored journals or raw backup exports. Run the export target and the MCP fixture; protect the existing provider-reasoning removal and redaction manifest behavior.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness --test replay_sessions_cli_test -E 'test(sessions_export)'` → Both test selections pass; exports contain no synthetic payload while the original journal remains byte-identical.

## Test plan

Use the behavioral cases and existing fixture named in the steps; their assertions define the regression being protected. Prefer extending those tests over creating an additional suite. The exemplar above shows the local pattern. Use controlled inputs, temporary owned directories and explicit synchronization; never use live credentials or network responses as fixtures.

Run `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` after implementation, followed by the additional compatibility checks in the command table. Preserve the unchanged control cases specified in the steps.

## Done criteria

All must hold:

- [x] Serialized tool results and displays from all three MCP entry points omit recognized encoded media.
- [x] Historical support-export copies omit the same media without changing source journals.
- [x] Ordinary text and unrelated data/blob fields survive; existing redaction and reasoning removal tests pass.
- [ ] Every final verification command above meets its expected result; any deliberately failing baseline regression is documented separately from the passing final run.
- [x] `git diff --check` exits 0 and the implementation diff is limited to the Scope list.
- [ ] Record actual commands/results and any material limits in this plan; update its execution status and index row. Do not describe an unrun check as passing.

## STOP conditions

Stop and report the concrete blocker instead of expanding scope if:

- The current code contradicts the inlined baseline and the difference is not an understood prerequisite change.
- A verification fails twice after a reasonable scoped correction, or the repair requires an out-of-scope file.
- Recognizing media requires guessing from arbitrary application key names instead of protocol structure.
- The fix requires decoding private media, persisting a replacement raw file, or changing event schemas.

## Maintenance notes

New MCP content types must define separate display, transient-provider and durable representations. Keep support-export normalization aligned with newly produced events.

## Execution evidence — 2026-09-20

Implemented for issue #242 on `codex/issue-242-media`, based on `3d8e3d4f`.
The prescribed drift check from `7f5a7ec6` showed no changes in the scoped files.

- Shared normalization visits only MCP content, resource contents, and prompt
  content envelopes. Image/audio `data` and embedded/read-resource `blob` values
  become `[MCP media omitted]`; neighboring text, MIME/type metadata, and ordinary
  application `data`/`blob` (including `structuredContent`) remain intact.
- All three execution paths normalize before rendering and result construction.
  Remote failures retain textual error handling; single-block and legacy array
  prompt content render safely. No binary provider feature or dependency is added.
- Historical support-export copies normalize recognized MCP wrappers and scrub
  their encoded payloads from duplicated task/tool summary text. The source
  journal and raw backup paths are unchanged.
- The new public MCP regression failed before implementation: 6 tests selected,
  5 passed, 1 failed because encoded media remained in the first-class tool result
  (nextest exit 100). The same command passes after implementation: 6/6.

All cargo verification uses `CARGO_TARGET_DIR=/home/urbanbreach/Projects/agent-harness/target/issue-closure`,
`CARGO_BUILD_JOBS=2`, `CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.

| Verification command | Result |
|---|---|
| `cargo nextest run --profile ci --locked --offline -p harness-tools --test mcp_generic_test` | PASS, 6 tests, exit 0 |
| `cargo nextest run --profile ci --locked --offline -p harness --test replay_sessions_cli_test -E 'test(sessions_export)'` | PASS, 11 tests (54 unrelated tests skipped), exit 0 |
| `cargo check -p harness-tools --locked --offline` | Queued worktree run canceled at operator request; delegated to integrated workspace verification |
| `cargo fmt --all -- --check` | PASS, exit 0 |
| `cargo clippy -p harness-tools --all-targets --all-features --locked --offline -- -D warnings` | Queued worktree run canceled at operator request; delegated to integrated workspace verification |
| `cargo check --workspace --locked --offline` | Queued worktree run canceled at operator request; delegated to integrated workspace verification |
| `git diff --check` | PASS, exit 0 |
| `git status --short` | Only the seven scoped implementation files and this plan |

The first export run passed the 10 existing cases and rejected the new fixture
for a noncanonical tool-call ID before exercising the export. The fixture now uses
the required `toolcall_` numeric IDs; all 11 export cases pass on rerun.

The operator owns the `plans/README.md` row update and independent agent review
after integration. This commit does not claim that review or issue closure.
