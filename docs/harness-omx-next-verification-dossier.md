# Harness OMX Next Verification Dossier

This dossier records the G007 verification evidence for Harness-native OMX command parity. It is evidence for the current parity slice, not final project closeout: G008 still owns the mandatory cleanup, post-clean verification, and clean code-review gate before the aggregate Codex goal may be completed.

## Evidence artifacts

- Targeted verification log: `target/ultragoal/G007-targeted-verification-final.log`
- Fast lane artifact root: `target/test-lanes/20260519-015140`
- Fast lane summary: `target/test-lanes/20260519-015140/summary.txt`
- Integration lane artifact root: `target/test-lanes/20260519-015158`
- Integration lane summary: `target/test-lanes/20260519-015158/summary.txt`
- Deterministic PTY signoff artifact root: `target/test-lanes/20260519-013914`
- Deterministic PTY signoff summary: `target/test-lanes/20260519-013914/summary.txt`
- Workflow inventory fixture: `crates/harness/tests/fixtures/harness_omx_workflow_inventory.json`
- Completion map: `docs/harness-omx-next-completion-dossier.md`

## Inventory proof

The checked inventory currently contains 189 rows: 188 `present` and 1 `non_applicable`. Present coverage includes all applicable reference `$` commands, 30 slash-agent commands, 33 copied oh-my-codex prompt assets, and registry-backed workflow rows. The lone non-applicable row is the `worker` team-internal protocol, which is intentionally not a user-facing dollar command.

Every present row is expected to carry the five proof categories below:

1. **Discovery** — source references in the inventory fixture plus drift tests for shipped skill/agent assets.
2. **Dispatch** — runtime semantics from `harness-core::command_registry`, TUI `/` and `$` derivation, and CLI workflow routing tests.
3. **Behavior/state** — coordinator-owned workflow, task, permission, question, and replay projections; no TUI-owned runtime authority.
4. **Docs/schema** — generated schemas, config docs, architecture docs, README anchors, and the completion dossier stay aligned.
5. **Verification** — targeted unit/integration tests plus the canonical `fast` lane pass listed below.

## Targeted verification

The latest targeted run passes the command-parity gates:

- `cargo fmt --all -- --check`
- `cargo check -p harness-core -p harness-tui -p harness`
- `cargo test -p harness-core command_registry --lib` — 6 passed
- `cargo test -p harness-core agent_catalog --lib` — 4 passed
- `cargo test -p harness-tui --lib slash_agent` — 2 passed
- `cargo test -p harness-tui --lib dollar_command` — 6 passed
- `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e pty_e2e_session_shell_primary -- --nocapture`
- `RUST_TEST_THREADS=1 cargo test -p harness-testkit --test pty_e2e pty_e2e_tui_interactive_prompt_streams_response -- --nocapture`
- `RUST_TEST_THREADS=1 cargo test -p harness-tui --test pty_e2e pty_e2e_snapshots_are_stable -- --nocapture`
- `cargo test -p harness slash_agent`
- `cargo test -p harness --test workflow_inventory` — 8 passed
- `cargo test -p harness --test config_docs_reference` — 9 passed
- `cargo test -p harness --test config_schema_cli` — 56 passed
- `cargo test -p harness --test event_docs_reference` — 1 passed
- `cargo run -p harness -- --config configs/harness.example.jsonc config validate`
- `git diff --check`

## Broad lane verification

`scripts/test-lanes.sh fast` passed with artifact root `target/test-lanes/20260519-015140`:

- `fmt` PASS
- `check` PASS
- `harness_tui_lib` PASS
- `harness_tui_model_switcher_metadata` PASS
- `harness_tui_session_navigation_keybindings` PASS

`scripts/test-lanes.sh integration` passed with artifact root `target/test-lanes/20260519-015158`.

`scripts/test-lanes.sh signoff-pty` passed with artifact root `target/test-lanes/20260519-013914`:

- `harness_testkit_pty_e2e` PASS
- `harness_tui_pty_e2e` PASS

## Snapshot drift note

The completed assistant footer now preserves the profile label (`Assistant`/`Worker`) before the model id, matching the PTY live-shell marker contract. The final TUI library run passed 620 tests, and no `*.snap.new` files remain under `crates/harness-tui` or `crates/harness-testkit`.

## Branding scan note

`python3 scripts/check-forbidden-branding.py` is covered by the broad `cargo test -p harness` gate. Legacy `.omo/**` evidence and the tracked historical musings document are explicitly quarantined as migration material so the scan can keep enforcing the public Harness surface without requiring local evidence deletion.
