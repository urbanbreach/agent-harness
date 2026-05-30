# ADR: `ast_grep_replace` edit-safety gate

Status: accepted for strict V1 closeout G005 (2026-05-30)

## Decision

Ship `ast_grep_replace` as a first-party native tool only through the existing edit authority boundary.

The tool defaults to `mode: "dry_run"`. The ast-grep CLI is invoked only to produce JSON rewrite matches and replacement byte ranges; Harness never passes an update/apply flag that lets the adapter mutate the live workspace. `mode: "apply"` validates the planned ranges against the current file contents and writes through Harness workspace path checks, atomic writes, and diff artifacts.

## Safety gate

- Runtime capability is `ToolCapability::EditFs`.
- Public permission mapping is `edit` / `PermissionKind::EditFs`.
- Coordinator path-scoped policy denial, including list-valued `paths`, happens
  before `ToolCallStarted` and before adapter invocation.
- The tool is not reachable through `ReadFs`, `codesearch`, aliases, or a read-only fallback.
- Arguments use strict schemas with unknown fields denied.
- Paths must stay inside the workspace, existing roots must match adapter output, and traversal is rejected.
- Dry-run returns structured edits and a diff artifact without changing files.
- Apply refuses truncated result sets to avoid partial rewrites.
- Apply rejects missing byte offsets, stale file contents, invalid byte ranges, and overlapping rewrites.
- Missing ast-grep binaries and invalid patterns return actionable errors.

## Evidence

- `cargo test -p harness-tools --test native_ast_grep_replace_test`
- `cargo test -p harness-tools --test native_ast_grep_search_test`
- `cargo test -p harness-tools --test native_tool_parity_matrix_test`
- `cargo test -p harness-core --test permission_policy_supports_native_tool_permission_kinds_test`
- `cargo test -p harness-core --test coord_ast_grep_auth_test -- --nocapture`

## Rejected alternatives

- Letting `ast-grep` update files directly: rejected because it bypasses Harness edit artifacts, byte-range validation, and permission/replay accounting.
- Registering the tool as `codesearch` or `ReadFs`: rejected because apply mode mutates workspace files and must be controlled by `edit`.
- Applying only the capped subset of a larger match set: rejected because partial structural rewrites are surprising and hard to audit.
