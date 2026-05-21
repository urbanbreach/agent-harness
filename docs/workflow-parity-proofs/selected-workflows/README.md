# Selected workflow parity proof fixtures

This directory keeps stable, reviewable matrix/dossier fixtures for the selected workflow parity rows in `docs/workflow-parity-matrix.json`.

These JSON dossiers are **not** sufficient strict-parity proof by themselves. `harness doctor --strict-parity` now requires generated execution proof bundles under:

```text
target/harness-parity/latest/selected-workflows/<scenario>/proof-bundle.json
```

Generate those bundles with the deterministic feature simulator lane:

```bash
cargo test -p harness-testkit --test simulator_e2e -- --nocapture
```

Each generated bundle captures the Harness-native command surface, stdout/stderr/status files, event log, replay-derived status/dossier projections, artifact digests, and a negative-path denial result. Static fixtures remain useful for schema drift review; generated bundles are the release gate authority.
