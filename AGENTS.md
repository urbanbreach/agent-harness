<!-- AUTONOMY DIRECTIVE — DO NOT REMOVE -->
YOU ARE AN AUTONOMOUS CODING AGENT. EXECUTE TASKS TO COMPLETION WITHOUT ASKING FOR PERMISSION.
DO NOT STOP TO ASK "SHOULD I PROCEED?" — PROCEED. DO NOT WAIT FOR CONFIRMATION ON OBVIOUS NEXT STEPS.
IF BLOCKED, TRY AN ALTERNATIVE APPROACH. ONLY ASK WHEN TRULY AMBIGUOUS OR DESTRUCTIVE.
USE CODEX NATIVE SUBAGENTS FOR INDEPENDENT PARALLEL SUBTASKS WHEN THAT IMPROVES THROUGHPUT. THIS IS COMPLEMENTARY TO OMX TEAM MODE.
<!-- END AUTONOMY DIRECTIVE -->

# PROJECT KNOWLEDGE BASE

**Generated:** 2026-04-24
**Commit:** `66c6d8ae`
**Branch:** `dev`

## OVERVIEW
Rust workspace for an event-sourced agent harness: CLI entrypoint, coordinator/runtime core, provider adapters, built-in native tools, Ratatui TUI, and deterministic PTY/live visual signoff lanes.

## STRUCTURE
```text
agent-harness/
├── crates/harness/          # CLI: tui/run/prompt/replay/sessions/schema/config
├── crates/harness-core/     # event store, coordinator, permissions, config, projections
├── crates/harness-tools/    # native tool registry: fs/edit/bash/task/web/lsp/mcp
├── crates/harness-tui/      # Ratatui shell, layout, transcript, overlays
├── crates/harness-testkit/  # fixtures, secret scanner, PTY/live/native signoff tests
├── configs/                 # canonical runtime/TUI examples + generated schemas
├── docs/                    # architecture, config, testing, roadmap
├── scripts/                 # stress harness
├── .agent-harness/          # shipped agent/skill assets discovered by runtime
├── .agents/                 # maintainer skills used by coding agents
└── inspirations/            # reference/vendor material; do not treat as first-party code
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| CLI behavior | `crates/harness/src/main.rs`, `crates/harness/src/*.rs` | Bare `harness` launches TUI; subcommands own headless/replay flows. |
| Runtime invariants | `crates/harness-core/AGENTS.md` | Read before changing events, coordinator, permissions, config, replay. |
| Native tools | `crates/harness-tools/AGENTS.md` | Read before changing schemas, filesystem safety, bash, MCP, LSP, delegation tools. |
| TUI shell | `crates/harness-tui/AGENTS.md` | Compose-first/transcript-first/operator-sidebar contracts. |
| Visual/live signoff | `crates/harness-testkit/tests/AGENTS.md` | PTY, native screenshot, live proxy ordering and env guards. |
| Public config contract | `docs/config.md`, `configs/*.json`, `configs/*.jsonc` | Generated schemas are source of truth; docs must stay aligned. |
| Architecture docs | `docs/architecture.md` | Event schema and crate-boundary reference. |
| Test map | `docs/testing.md` | Drift checks and signoff lanes. |

## CODE MAP
| Crate / module | Role | Local guidance |
|----------------|------|----------------|
| `harness` | CLI binary and session/replay/prompt commands | Keep domain logic in `harness-core`; keep UI rendering in `harness-tui`. |
| `harness-core::coord` | Single scheduling authority | Only coordinator appends events and owns permission/task state transitions. |
| `harness-core::event` | Event schema v1 | Append-only source of truth; update `docs/architecture.md` + drift tests for variants. |
| `harness-core::config` | Runtime/TUI config loading + schemas | New public keys require schema/docs/tests. |
| `harness-tools` | Native provider tool surface | Stable schemas, workspace path safety, role-separated tools. |
| `harness-providers` | Mock + OpenAI-compatible streaming | Keep transport normalization provider-local. |
| `harness-tui` | Ratatui presentation | Structured state in app/view-models; layout/theme own geometry. |
| `harness-testkit` | Test-only helpers and E2E suites | Runtime-independent helpers in `src/`; workflow-heavy PTY/live code under `tests/`. |

## FIRST-PARTY SEARCH SCOPE
- Include: `crates/`, `configs/`, `docs/`, `scripts/`, `.agent-harness/`, `.agents/`, root manifests.
- Exclude by default: `target/`, `.git/`, `.omx/`, `.codex/.tmp/`, `sessions/`, `artifacts/`, `inspirations/`.
- Search `inspirations/` only when explicitly comparing reference implementations.

## COMMANDS
```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features -- --test-threads=1
cargo run -p harness -- --config configs/harness.example.jsonc config validate
scripts/stress-harness.sh --mode offline
```

Targeted checks:
```bash
cargo test -p harness --test config_schema_cli
cargo test -p harness --test config_docs_reference
cargo test -p harness --test event_docs_reference
cargo test -p harness-core
cargo test -p harness-tools --test native_tool_parity_matrix
cargo test -p harness-tui
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

## CONVENTIONS
- Workspace lints deny `unsafe_code`, `dbg_macro`, and `todo`; keep warnings at zero with clippy `-D warnings`.
- Config uses harness-centered names: runtime `harness.json{,c}`, TUI `tui.json{,c}`; legacy broad shapes are compatibility-only.
- Runtime prompt assets live in `.agent-harness/agents/*.md`; `AGENTS.md` is a separate project-instruction layer.
- Public permission names are `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`; `shell`/`network` are migration aliases only.
- Prefer small diffs and extraction over redesign; reuse existing helpers before adding abstractions.
- No new dependencies without explicit request.
- For cleanup/refactor/deslop work: write a cleanup plan, lock behavior with tests first when not already protected, prefer deletion over addition.

## INVARIANTS
- Events are the source of truth; replay must stay side-effect free and derive state from JSONL events in `seq` order.
- Coordinator is the only event append / task scheduling / permission resolution authority.
- Permission checks happen before tool execution; worker redelegation bypasses must remain blocked.
- Hashline edits validate anchors, reject overlaps, apply bottom-up, and write atomically.
- Tool outputs persist as capped event summaries plus redacted artifacts under `artifacts/toolcalls/`.
- Provider-context compaction writes checkpoint artifacts/events; it does not rewrite `events.jsonl` and is distinct from TUI memory caps.
- Unsupported config areas such as `server`, `command`, `plugin`, `share`, and `autoupdate` are rejected explicitly.

## ANTI-PATTERNS
- Do not move coordinator/runtime invariants into CLI, tools, or TUI crates.
- Do not make replay execute tools, network calls, or other side effects.
- Do not broaden config compatibility and call it the canonical public contract.
- Do not use `bash` for file reads/search/edits when native tools cover the operation.
- Do not hardcode paths outside config discovery and workspace-relative resolution helpers.
- Do not treat PTY/native/live visual artifacts as interchangeable; each lane has its own provenance contract.

## COMMIT MESSAGES
Use the Lore protocol when committing: first line explains why, body captures context, and trailers record useful constraints. Valuable trailers: `Constraint:`, `Rejected:`, `Confidence:`, `Scope-risk:`, `Directive:`, `Tested:`, `Not-tested:`.

## OMX / AGENT RUNTIME NOTES
- This file is the top-level operating contract for workspace agents; subdirectory `AGENTS.md` files narrow it and must not repeat it.
- Runtime-only workflows (`ralph`, `team`, `ultrawork`, `ecomode`) require an actual OMX runtime session; otherwise use direct execution or native subagents.
- Preserve marker-bounded runtime overlays if OMX setup regenerates them; do not hand-edit generated runtime state under `.omx/` as project source.
