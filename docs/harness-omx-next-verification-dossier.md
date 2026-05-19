# Harness OMX Next Verification Dossier

This dossier records the G007 verification evidence for Harness-native OMX command parity. It is evidence for the current parity slice, not final project closeout: G008 still owns the mandatory cleanup, post-clean verification, and clean code-review gate before the aggregate Codex goal may be completed.

## Evidence artifacts

- Targeted verification log: `target/ultragoal/G007-targeted-verification-final.log`
- Fast lane artifact root: `target/test-lanes/20260518-233338`
- Fast lane summary: `target/test-lanes/20260518-233338/summary.txt`
- Workflow inventory fixture: `crates/harness/tests/fixtures/harness_omx_workflow_inventory.json`
- Completion map: `docs/harness-omx-next-completion-dossier.md`

## Inventory proof

The checked inventory currently contains 158 rows: 110 `present`, 45 `partial`, and 3 `non_applicable`. Present coverage includes 30 slash-agent commands and registry-backed workflow rows. Partial rows remain fail-closed or blocked/staged; they are not counted as native parity.

Every present row is expected to carry the five proof categories below:

1. **Discovery** — source references in the inventory fixture plus drift tests for shipped skill/agent assets.
2. **Dispatch** — runtime semantics from `harness-core::command_registry`, TUI `/` and `$` derivation, and CLI workflow routing tests.
3. **Behavior/state** — coordinator-owned workflow, task, permission, question, and replay projections; no TUI-owned runtime authority.
4. **Docs/schema** — generated schemas, config docs, architecture docs, README anchors, and the completion dossier stay aligned.
5. **Verification** — targeted unit/integration tests plus the canonical `fast` lane pass listed below.

## Targeted verification

The final targeted run in `target/ultragoal/G007-targeted-verification-final.log` passed:

- `cargo fmt --all -- --check`
- `cargo check -p harness-core -p harness-tui -p harness`
- `cargo test -p harness-core command_registry --lib` — 6 passed
- `cargo test -p harness-core agent_catalog --lib` — 4 passed
- `cargo test -p harness-tui --lib slash_agent` — 2 passed
- `cargo test -p harness-tui --lib dollar_command` — 5 passed
- `cargo test -p harness slash_agent`
- `cargo test -p harness --test workflow_inventory` — 8 passed
- `cargo test -p harness --test config_docs_reference` — 9 passed
- `cargo test -p harness --test config_schema_cli` — 56 passed
- `cargo test -p harness --test event_docs_reference` — 1 passed
- `cargo run -p harness -- --config configs/harness.example.jsonc config validate`
- `git diff --check`

## Broad lane verification

`scripts/test-lanes.sh fast` passed with artifact root `target/test-lanes/20260518-233338`:

- `fmt` PASS
- `check` PASS
- `harness_tui_lib` PASS
- `harness_tui_model_switcher_metadata` PASS
- `harness_tui_session_navigation_keybindings` PASS

## Snapshot drift note

The TUI snapshot drift from the model-first assistant footer contract was accepted and reverified. The final TUI library run passed 615 tests, and no `*.snap.new` files remain under `crates/harness-tui`.

## Known non-claim

`python3 scripts/check-forbidden-branding.py` is not claimed as passing in this dossier. Earlier runs identified pre-existing failures tied to `.omo/**` evidence and `docs/harness-opencode-omx-musings.md`; that cleanup is outside G007 and remains a separate follow-up unless G008 chooses to address it.
