# HARNESS TOOLS SOURCE GUIDE

## OVERVIEW

Score 13: 38 direct Rust files, 14 subdirectories, a `lib.rs` boundary, and measured high symbol/export density make this the workspace's native-tool implementation hub.

## STRUCTURE

```text
src/
|- lib.rs              # registry assembly and public transport/catalog seams
|- native_tools/       # provider-facing wrappers and argument schemas
|- fs_read/            # bounded text reads and hashline rendering
|- skill_catalog/      # precedence, frontmatter, and resource activation
|- apply_patch_tool/   # parse and preflight before sequential mutation
|- shell_run/          # process execution, confinement, and output limits
|- session_tools/      # replay-only session projections
|- network/            # fetch and remote-search transports
|- lsp_support/        # JSON-RPC process/session support
`- code_lsp_rename/    # UTF-16 rename planning and application
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Add or expose a native tool | `lib.rs`, `native_tools.rs`, `tool_catalog.rs` | Keep registry, canonical ID, actor availability, and schema aligned. |
| Change file operations | `fs_read.rs`, `fs_grep.rs`, `workspace_paths.rs`, edit modules | Preserve workspace containment and artifact behavior. |
| Change process execution | `shell_safety.rs`, `shell_run.rs`, `shell_run/` | Validation and Linux confinement fail closed. |
| Change delegation | `agent_ops.rs`, `agent_ops/`, `control_plane.rs` | Coordinator owns permissions, lineage, and event ordering. |
| Change language intelligence | `code_lsp.rs`, `lsp_support.rs`, `code_lsp_rename.rs` | Public positions are 1-based; protocol positions are 0-based. |
| Change extension transport | `mcp.rs`, `mcp_session.rs`, `mcp_render.rs` | Support configured stdio and HTTP sessions. |

## CONVENTIONS

- Implement tools through `harness_core::tool::Tool`; use strict serde arguments and provider-safe object-root schemas.
- Pair model-facing display text with structured JSON; spill bounded overflow to artifacts with digest/path metadata.
- Canonicalize paths before access, reject escapes, and preserve deterministic ordering with sorting or ordered maps.
- Editing paths preflight current content, preserve UTF-8/BOM/line endings where promised, and emit mutation evidence.
- Keep most implementation types crate-visible; export only registry configuration, injectable transports, and stable catalog contracts.

## ANTI-PATTERNS

- Never bypass workspace checks, symlink checks, permission routing, atomic-edit paths, or stale-anchor validation.
- Never use shell as a substitute for native read/list/glob/grep/edit tools; shell policy intentionally rejects expansion and compound syntax.
- Never execute providers, tools, hooks, MCP, network, or CLI while replaying or inspecting sessions.
- Do not apply truncated ast-grep results, overlapping edits, stale rename ranges, or unsupported patch moves.
- Do not leak credentials, URL query strings, raw attachment bytes, reasoning, or unredacted event payloads into artifacts.
