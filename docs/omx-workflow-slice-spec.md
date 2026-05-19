# OMX-style workflow slice specification

Legacy OMO parity planning is retained as background migration evidence only and is not the product direction. The current product direction is single-operator workflow orchestration with explicit operator-owned escalation.

## Workstream J: Setup, doctor, and SSOT verification

Treat first-party workflow commands, aliases, prompts, evidence categories, doctor checks, and docs links as a small single source of truth early in the slice.

Manifest/registry verification tests for first-party commands, aliases, evidence categories, prompts, and doctor/docs links.

Required drift anchors:

- workflow commands registered
- aliases present or explicitly disabled
- First-party command/alias/evidence/doctor/docs SSOT drift guard.

Exit criteria: implementer can state what is reused, what is wrapped with workflow metadata, what is hardened later, and what is deferred.

## Current command-parity direction

The command-parity inventory at `docs/harness-omx-workflow-inventory.md` supersedes legacy OMO parity as the checked reference matrix for Harness-native command behavior.
