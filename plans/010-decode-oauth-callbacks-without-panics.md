# Plan 010: Reject malformed OAuth callback encoding without panicking

> **Executor instructions:** Read the complete plan, then follow its steps and checks. Stop on the conditions below rather than expanding scope. Update this plan's execution status and its row in `plans/README.md` when finished, unless a dispatched reviewer owns those updates.
>
> **Drift check (run first):** `git diff --stat 2e342840..HEAD -- crates/harness-core/src/auth/codex.rs crates/harness-core/src/auth/codex/tests.rs`
>
> Also run `git status --short` to detect uncommitted changes. Compare changed source against the excerpts before editing. An expected prerequisite change is acceptable only after checking the stated prerequisite contract; any other material mismatch requires plan refresh.

## Status

- **Execution**: DONE
- **Audit finding**: 9 from the deep audit dated 2026-09-18
- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: None
- **Category**: bug
- **Planned at**: commit `2e342840`, 2026-09-18
- **Publication**: Published after explicit public-disclosure confirmation on 2026-09-18.
- **Issue**: https://github.com/urbanbreach/agent-harness/issues/233

## Execution evidence (2026-09-20)

- Drift check: `git diff --stat 2e342840..HEAD -- crates/harness-core/src/auth/codex.rs crates/harness-core/src/auth/codex/tests.rs` produced no differences before editing. Both source files were clean and matched the plan; both CLI callback paths use the same public completion method.
- Before the fix, `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(codex_loopback)'` exited 100: **1 passed, 1 failed**, 880 skipped. The extended rejection test reproduced the UTF-8 boundary panic in `percent_decode`; the ordinary callback control passed.
- After the fix, the same callback command exited 0: **2 passed**, 880 skipped. Eight malformed cases cover Unicode next to escapes in keys and values, incomplete escapes, invalid hex, and invalid or incomplete decoded UTF-8. Each asserts the fixed `CallbackRejected` message, zero HTTP calls, and no stored credential.
- After extending the success control, `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(auth::codex::tests::)'` exited 0: **11 passed**, 871 skipped. The mock exchange verifies normalized encoding, including plus/percent spaces, escaped delimiters, mixed-case hex, ordinary UTF-8, encoded keys/state, query-only input, fragments, and last-duplicate-key behavior. The original full callback URL, state rejection, PKCE and device-flow coverage remains passing.
- `cargo check --workspace --locked --offline` exited 0.
- `cargo fmt --all -- --check` exited 0.
- `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` exited 0.
- `git diff --check` and `git diff --cached --check` exited 0. The staged change contains only the two scoped Rust files, this execution record, and the matching plan-index row. Pre-existing user files remain unchanged except this plan's authorized execution record and index entries; unrelated index-document edits remain unstaged.
- The decoder consumes bytes without string slicing and uses strict UTF-8 conversion. All new parse failures carry only the fixed message; no dependency, callback listener, or OAuth protocol change was added.
- The full workspace test suite and repository-wide quality gates were not run; historical planning results are not claimed as current verification.

## Why this matters

The OAuth percent decoder slices a UTF-8 str with byte offsets near a percent marker. Malformed Unicode callback input can therefore panic before state validation. Decode bytes safely and reject malformed escaping through the existing callback error type before any token exchange or credential write.

## Current state

- [crates/harness-core/src/auth/codex.rs:208](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L208) — Public completion method parses the callback before checking state.
- [crates/harness-core/src/auth/codex.rs:655](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L655) — parse_query handles both query keys and values.
- [crates/harness-core/src/auth/codex.rs:691](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L691) — Percent decoder mixes byte indexing with str slicing.
- [crates/harness-core/src/auth/codex.rs:555](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L555) — Existing CallbackRejected error avoids a new public error variant.
- [crates/harness/src/auth_cmd/login.rs:649](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness/src/auth_cmd/login.rs#L649) — Loopback request targets reach the same public callback completion method.
- [crates/harness-core/src/auth/codex/tests.rs:135](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex/tests.rs#L135) — Existing rejected-callback test checks no exchange and no stored credential.

[crates/harness-core/src/auth/codex.rs:691](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L691):

```rust
fn percent_decode(value: &str) -> String {
    let mut output = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    output.push(byte);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
```

[crates/harness-core/src/auth/codex.rs:207](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L207):

```rust

    pub async fn complete_loopback_callback(
        &self,
        session: &CodexLoopbackSession,
        callback_url: &str,
        store: &CredentialStore,
    ) -> Result<StoredCredential, CodexOAuthError> {
        let query = parse_query(callback_url);
        if let Some(error) = query.get("error") {
            return Err(CodexOAuthError::CallbackRejected {
                message: query
                    .get("error_description")
```

## Conventions and exemplar

This is a Rust 2021 workspace. Runtime authority and durable event appends belong to the coordinator; providers normalize protocol events and tools return results. Match existing `Result` and `ToolResultExt` error handling. Do not add production `unwrap`, `expect`, panics, unsafe code or ignored fallible results. Tests use existing temporary fixtures, `FakeClock` where needed, and the repository's `UnwrapOrAbort` convention. Run tests with nextest.

[crates/harness-core/src/auth/codex/tests.rs:163](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex/tests.rs#L163):

```rust
        .await
        .expect_err("missing code");
    assert!(matches!(missing_code, CodexOAuthError::MissingCode));
    assert!(matches!(
        session.timeout_error(),
        CodexOAuthError::CallbackTimeout { .. }
    ));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    assert!(store.load(&ProviderId::codex()).unwrap_or_abort().is_none());
}
```

Relevant design contract: [crates/harness-core/src/auth/codex.rs:208](https://github.com/urbanbreach/agent-harness/blob/2e342840fa2d3dae7501af198166bd6742721e6f/crates/harness-core/src/auth/codex.rs#L208).

Use the existing Result/CodexOAuthError flow and MockAuthHttpClient. Preserve valid plus-as-space decoding, valid percent escapes, ordinary UTF-8, full callback URLs, query-only input, fragment handling and existing state validation. Do not change PKCE, token persistence, listener binding, issuer configuration or device flow. Diagnostics must not echo callback input, authorization codes or state.

## Commands you will need

Run commands from the repository root. The audit used the existing installed toolchain and dependencies; no dependency installation is needed.

| Purpose | Command | Expected result |
|---|---|---|
| Workspace compile | `cargo check --workspace --locked --offline` | Exit 0. |
| Focused behavior | `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(auth::codex::tests::)'` | Selected tests pass after the repair; selection must not be empty. |
| Formatting check | `cargo fmt --all -- --check` | Exit 0; do not reformat unrelated files. |
| Scoped lint | `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` | Exit 0; no blanket lint suppression. |
| Whitespace | `git diff --check` | Exit 0. |

Planning verification is not implementation verification. At the planning commit, workspace compilation and 96 previously selected core/provider tests passed. The full workspace suite and scoped lint commands above were not run for this plan. Known repository-wide gates already fail on an 823-line TUI test file and five existing branding matches in earlier planning documents. Do not repair those unrelated files or represent them as newly green. If a required command fails for an unrelated reason, preserve evidence and report the baseline blocker.

## Scope

**Allowed code, tests and documentation changes:**

- `crates/harness-core/src/auth/codex.rs`
- `crates/harness-core/src/auth/codex/tests.rs`

Administrative updates are limited to execution status/evidence in `plans/010-decode-oauth-callbacks-without-panics.md` and the matching row/dependency note in `plans/README.md`.

**Out of scope:** all other files, unrelated audit findings, generated startup probe files, real credentials, provider/model feature expansion, and generic architecture cleanup. Preserve existing user changes. In the audited working tree, `harness.jsonc` was already modified and `20260906-192230/` was already untracked; neither is an input or output of this plan. Use a clean isolated checkout if needed.

## Git workflow

- Suggested branch: `codex/plan-010-decode-oauth-callbacks-without-panics`.
- Keep this repair in one logical change; if instructed to commit, use `fix(auth): reject malformed callback encoding safely`, matching the existing `fix(scope): ...` style.
- Do not commit unrelated user work, merge, push or create a pull request without the operator's instruction.
- This document authorizes no implementation by the advisor; it is a handoff for the selected executor.

## Steps

### Step 1: Add public malformed-callback cases

Extend codex_loopback_rejects_bad_state_missing_code_and_timeout_without_storing with a table covering malformed percent sequences next to multi-byte characters in keys and values, truncated escapes, invalid hex and escaped bytes that are not valid UTF-8. Keep all values synthetic and describe cases by name rather than dumping full callback strings on failure. Assert a normal error, zero HTTP calls and an empty credential store. Retain the existing valid callback test as a control.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(codex_loopback)'` → The malformed Unicode case exposes the panic before step 2; existing ordinary callback behavior remains specified.

### Step 2: Decode bytes and propagate structured failure

Change percent_decode to return Result<String, CodexOAuthError>. Read two following bytes with bounds checks and convert ASCII hex digits without creating a str slice. Reject a missing/non-hex escape with CallbackRejected containing a constant safe message. Use String::from_utf8 at the end and map invalid decoded UTF-8 to the same error instead of lossy substitution. Make parse_query return Result and propagate it in complete_loopback_callback with ?. Keep the existing query splitting and duplicate-key behavior, plus-space conversion, state checks and successful exchange sequence.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(codex_loopback)'` → All malformed cases return an error without panic, HTTP exchange or credential writes; the valid callback case passes.

### Step 3: Verify valid encoding and adjacent auth behavior

Extend the existing success test with a valid encoded callback value whose decoded form is asserted in the mocked exchange, without weakening state validation. Run the complete Codex auth test module to cover PKCE and device-flow compatibility. No new parser dependency or custom query framework is needed.

**Verify:** `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(auth::codex::tests::)'` → All selected auth tests pass; the exchange receives the intended decoded value only after state validation.

### Step 4: Run final gates and record the result

Run workspace compilation, focused behavior, any additional behavior commands, formatting check, scoped lint and whitespace checks from the command table. Inspect `git diff --name-only` and `git ls-files --others --exclude-standard` against the allowed list and your recorded initial state. Do not accept unrelated source, fixture or lockfile changes. Record exact command outcomes and any baseline blocker in this plan, then update its index row.

**Verify:** `git diff --check` → exit 0; every command in the table has a recorded result, the behavioral criteria below pass, and the change set contains only allowed work.

## Test plan

Reuse the existing public completion-method tests and mock HTTP call counter. A private percent_decode unit test alone would not prove errors prevent credential side effects. One table covers malformed categories; do not multiply tests for many equivalent Unicode literals or install a fuzzing framework.

## Done criteria

All must hold:

- [x] Malformed percent/Unicode input produces a structured error, never a str-boundary panic.
- [x] Malformed callbacks perform zero HTTP exchanges and store no credentials.
- [x] Valid encodings and state/PKCE/device-flow tests remain passing.
- [x] New parse errors contain no callback text or credential material.
- [x] `cargo nextest run --profile ci --locked --offline -p harness-core --lib -E 'test(auth::codex::tests::)'` passes with a non-empty selection.
- [x] `cargo check --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy -p harness-core --all-targets --all-features --locked --offline -- -D warnings` and `git diff --check` pass, or a documented baseline blocker keeps this plan explicitly BLOCKED rather than DONE.
- [x] Changed paths are within the Scope list; pre-existing user files are untouched.
- [x] Execution evidence and the matching index status are updated; no implementation or verification result is invented.

## STOP conditions

Stop and report the concrete mismatch if:

- Current code materially differs from the excerpts beyond the explicitly described prerequisite changes.
- A required verification fails twice after a reasonable focused fix attempt.
- A fix requires modifying a file outside Scope, disabling a policy check, accepting changed golden output without explanation, or using actual credential material.
- The fix requires changing OAuth protocol semantics outside query decoding or adding a real callback listener to tests.
- Any error fallback silently substitutes malformed decoded bytes and then proceeds to token exchange.
- A dependency is proposed solely for a byte-safe conversion that fits the existing decoder.

## Maintenance notes

Future callback parsing changes must keep input untrusted until decoding and state validation both succeed. Both pasted and loopback callbacks use this shared boundary.
