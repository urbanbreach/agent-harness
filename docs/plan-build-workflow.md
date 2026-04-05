# Canonical plan -> build workflow

The shipped example config now exposes the intended user-facing split:

- `plan`: read-only planning mode with `plan.exit`
- `build`: implementation mode that receives the approved handoff
- `tool_audit`: signoff/profile-surface verification
- `deep_compat`: compat-path parity regression coverage

Primary profiles:

- `plan` — default first-run lane
- `build` — post-approval implementation lane

Secondary profiles:

- `tool_audit` — evidence-first signoff lane
- `deep_compat` — compat-path parity regression lane

Blessed default path:

- provider: `default` via the local CLIProxy-compatible endpoint at `http://127.0.0.1:8317/v1`
- model: `gpt-5.4-mini`
- entrypoint: `cargo run -p harness -- --config configs/harness.example.jsonc`

## First run

Validate the shipped config:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc config validate
```

Launch the interactive UI with the canonical plan -> build split:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc
```

`ui.default_profile` is `plan`, so the first session starts in planning mode.
The shipped `plan` profile uses `default:gpt-5.4-mini`, and `plan.exit` hands off into the `build` profile on the same default provider/model family.

## Handoff contract

1. Start in `plan`.
2. Produce or refine the implementation plan without file edits.
3. When the user approves implementation, call `plan.exit`.
4. Confirm the handoff prompt.
5. The runtime switches to `build` and submits the synthetic implementation prompt.

`plan.exit` is only available from plan-mode profiles. In the shipped config, `plan.exit` resolves from `plan` to `build`.

## Direct prompt entrypoints

Run a one-shot planning session:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc prompt --profile plan --text "Plan the change before editing."
```

Run a one-shot implementation session:

```bash
cargo run -p harness -- --config configs/harness.example.jsonc prompt --profile build --text "Implement the approved plan and verify it."
```

## Verification surfaces

- `cargo test -p harness --test config_schema_cli`
- `cargo test -p harness-tools --test native_control_plane_tools`
- `cargo test -p harness-tools --test opencode_compat_live`
- `cargo test -p harness-testkit live_proxy_e2e`
