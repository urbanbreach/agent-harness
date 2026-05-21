# Workflow Parity Closeout Audit

Status: strict-parity closeout record after the OMX `$` workflow false-green review  
Source plan: `.omx/plans/ralplan-harness-direction-next-20260520T003515Z.md`  
Current conclusion: the Rust-native OMX workflow parity gate is **closed for the tracked `$` workflow surface**.

## What changed

- Every active OMX workflow row in `docs/workflow-parity-matrix.json` is now `selected_for_this_goal` and `native_complete`; strict doctor validates each selected row's dossier plus a generated execution proof bundle.
- OMX hard-deprecated shims are classified as `retired_with_reason` / `compat_only`, not as unfinished native workflows.
- Provider-specific advisor shims (`ask-claude`, `ask-gemini`) preserve the OMX hard-deprecated skill text while command dispatch routes provider choice through the canonical `ask` workflow.
- `workflow_skill_protocol_native` reads the reference assets under `inspirations/oh-my-codex/skills` and checks substantive normalized behavior lines, not just boilerplate section headings.
- Projection-only surfaces such as help/HUD/trace no longer receive native-complete credit through prompt/provider mutation; deprecated shims are excluded from active proof credit.

## Completion gate

Strict parity is complete only when all of these pass together:

```bash
cargo test -p harness-testkit --test simulator_e2e -- --nocapture
cargo run -p harness -- --config configs/harness.example.jsonc doctor --json --strict-parity
cargo test -p harness --test workflow_inventory
cargo test -p harness --test config_schema_cli
```

The simulator lane writes generated execution bundles under `target/harness-parity/latest/selected-workflows/<scenario>/proof-bundle.json`. The checked-in dossiers under `docs/workflow-parity-proofs/selected-workflows/` remain stable review fixtures; generated bundles are the strict execution authority.

## Remaining caveat

This audit records parity of the prompt-driven OMX workflow model on Harness-native coordinator/events/projections. It does not claim that every workflow has been rewritten as a bespoke Rust state machine; the 1:1 behavior target is the OMX skill/workflow surface executed through native Harness routing, evidence, replay, and closeout gates.
