<!-- AUTONOMY DIRECTIVE — DO NOT REMOVE -->
YOU ARE AN AUTONOMOUS CODING AGENT. EXECUTE TASKS TO COMPLETION WITHOUT ASKING FOR PERMISSION.
DO NOT STOP TO ASK "SHOULD I PROCEED?" — PROCEED. DO NOT WAIT FOR CONFIRMATION ON OBVIOUS NEXT STEPS.
IF BLOCKED, TRY AN ALTERNATIVE APPROACH. ONLY ASK WHEN TRULY AMBIGUOUS OR DESTRUCTIVE.
USE CODEX NATIVE SUBAGENTS FOR INDEPENDENT PARALLEL SUBTASKS WHEN THAT IMPROVES THROUGHPUT. THIS IS COMPLEMENTARY TO legacy runtime TEAM MODE.
<!-- END AUTONOMY DIRECTIVE -->

# PROJECT KNOWLEDGE BASE

**Generated:** 2026-05-19
**Commit:** `cb144ee`
**Branch:** `single-workflow-experiment`

## OVERVIEW
Rust workspace for an event-sourced agent harness: CLI entrypoint, coordinator/runtime core, provider adapters, native tools, Ratatui TUI, and PTY/live/native signoff lanes.

## STRUCTURE
```text
agent-harness/
├── crates/harness/          # CLI: tui/run/prompt/replay/sessions/schema/config
├── crates/harness-core/     # event store, coordinator, permissions, config, projections
├── crates/harness-providers/# mock + OpenAI-compatible streaming providers
├── crates/harness-tools/    # native tool registry: fs/edit/bash/task/web/lsp/mcp
├── crates/harness-tui/      # Ratatui shell, layout, transcript, overlays
├── crates/harness-testkit/  # fixtures, secret scanner, PTY/live/native signoff tests
├── configs/                 # canonical examples, generated schemas, bundled model catalog
├── docs/                    # architecture/config/testing contracts + workflow dossiers
├── scripts/                 # canonical lane runner, stress harness, branding scan
├── .agent-harness/          # shipped runtime agents/skills/prompts discovered by harness
└── .agents/                 # maintainer skills used by coding agents
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| CLI behavior | `crates/harness/AGENTS.md` | Subcommands, bootstrap, prompt/replay/session/config flows. |
| Runtime invariants | `crates/harness-core/AGENTS.md` | Read before changing events, coordinator, permissions, config, replay. |
| Provider transport | `crates/harness-providers/AGENTS.md` | Mock fixtures and OpenAI-compatible Chat/Responses streaming. |
| Native tools | `crates/harness-tools/AGENTS.md` | Read before changing schemas, filesystem safety, bash, MCP, LSP, delegation tools. |
| TUI shell | `crates/harness-tui/AGENTS.md` | Compose-first/transcript-first/operator-sidebar contracts. |
| Visual/live signoff | `crates/harness-testkit/tests/AGENTS.md` | PTY, native screenshot, live proxy ordering and env guards. |
| Testkit helpers/fixtures | `crates/harness-testkit/AGENTS.md` | Secret scanner, workflow simulator, native visual helper, fixtures. |
| Public config contract | `docs/AGENTS.md`, `configs/AGENTS.md` | Generated schemas are source of truth; docs must stay aligned. |
| Architecture docs | `docs/architecture.md` | Event schema, replay, hooks, permissions, and crate-boundary reference. |
| Test map and scripts | `docs/testing.md`, `scripts/AGENTS.md` | Canonical lane runner, artifacts, env gates, stress lanes. |
| Runtime assets | `.agent-harness/AGENTS.md` | Shipped agents, category profiles, skills, native-agent prompts, and runtime prompt templates. |

## CODE MAP
| Crate / module | Role | Local guidance |
|----------------|------|----------------|
| `harness` | CLI binary and session/replay/prompt commands | Keep domain logic in `harness-core`; keep UI rendering in `harness-tui`. |
| `harness-core::coord` | Single scheduling authority | Only coordinator appends events and owns permission/task state transitions. |
| `harness-core::event` | Event schema v1 | Append-only source of truth; update `docs/architecture.md` + drift tests for variants. |
| `harness-core::config` | Runtime/TUI config loading + schemas | New public keys require schema/docs/tests. |
| `harness-core::workflow*` | Workflow, closeout, goal, mission, wiki projections | Status/read/dossier surfaces remain projection-only unless a command explicitly records evidence. |
| `harness-providers` | Mock + OpenAI-compatible streaming | Normalize transport details into `ProviderStreamEvent` here, not in core. |
| `harness-tools` | Native provider tool surface | Stable schemas, workspace path safety, role-separated tools. |
| `harness-tui` | Ratatui presentation | Structured state in app/view-models; layout/theme own geometry. |
| `harness-testkit` | Test-only helpers and E2E suites | Runtime-independent helpers in `src/`; workflow-heavy PTY/live code under `tests/`. |

## FIRST-PARTY SEARCH SCOPE
- Include: `crates/`, `configs/`, `docs/`, `scripts/`, `.agent-harness/`, `.agents/`, root manifests.
- Exclude by default: `target/`, `.git/`, `.harness/`, `.codex/.tmp/`, `.omo/`, `.sisyphus/`, `.gnhf/`, `sessions/`, `artifacts/`, `.agent-harness/sessions/`.

## COMMANDS
Use `scripts/test-lanes.sh` as the canonical runner; it records command/status/env evidence under the artifact root.
```bash
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh signoff-pty
scripts/test-lanes.sh signoff-browser
scripts/test-lanes.sh signoff-live
scripts/test-lanes.sh signoff-native
scripts/test-lanes.sh stress-offline
scripts/test-lanes.sh stress-live
scripts/test-lanes.sh all-deterministic
scripts/test-lanes.sh fast --dry-run
```

Useful direct checks when a full lane is too broad:
```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p harness -- --config configs/harness.example.jsonc config validate
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
cargo test -p harness --test config_docs_reference
cargo test -p harness --test event_docs_reference
RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e
```

## CONVENTIONS
- Use the rust-best-practices skill every time when writing Rust code.
- Workspace lints deny `unsafe_code`, `dbg_macro`, and `todo`; keep warnings at zero with clippy `-D warnings`.
- Config uses harness-centered names: runtime `harness.json{,c}`, TUI `tui.json{,c}`; legacy broad shapes are compatibility-only.
- Runtime prompt assets live in `.agent-harness/agents/*.md`; `AGENTS.md` is a separate project-instruction layer.
- Public permission names are `bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`; `shell`/`network` are migration aliases only.
- `task` calls require `run_in_background` and `load_skills`; category routes are ordinary profile-backed subagents and deny recursive delegation by default.
- `configs/provider-catalog.generated.json` is checked in and bundled; update it through `cargo run -p harness -- models generate`.
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
- Workflow status, dossier, snapshot, goal, mission, and wiki reads derive from recorded events; replay/read surfaces must not append events or rerun hooks/tools.
- Unsupported config areas such as `server`, `command`, `plugin`, `share`, and `autoupdate` are rejected explicitly.

## ANTI-PATTERNS
- Do not move coordinator/runtime invariants into CLI, tools, providers, or TUI crates.
- Do not make replay execute tools, network calls, provider calls, hooks, or other side effects.
- Do not broaden config compatibility and call it the canonical public contract.
- Do not use `bash` for file reads/search/edits when native tools cover the operation.
- Do not hardcode paths outside config discovery and workspace-relative resolution helpers.
- Do not treat PTY/native/live visual artifacts as interchangeable; each lane has its own provenance contract.
- Do not treat `.omo/**`, parity ledgers, or external migration material as product direction unless the task is explicit migration/comparison work.

## COMMIT MESSAGES
Use the Lore protocol when committing: first line explains why, body captures context, and trailers record useful constraints.
Valuable trailers: `Constraint:`, `Rejected:`, `Confidence:`, `Scope-risk:`, `Directive:`, `Tested:`, `Not-tested:`.
