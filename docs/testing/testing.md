# Testing

Use nextest. Backend tests inject providers, clocks, environment lookups and
workspaces. HTTP fixtures listen only on loopback. Real processes, public services,
and terminal captures have separate opt-in lanes.

Before adding a test, find the existing behavior check and extend it when possible.
Start with a failing assertion for a plausible regression, make it pass, and stop.
Do not add coverage for getters, derived types, delegation, or impossible states.

## Everyday checks

```bash
python3 scripts/check-test-suite-gates.py --format
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
```

The format gate checks backend Rust without traversing the preserved TUI modules.
It also enforces files below 500 lines, injected process state, bounded event waits,
explicit native/live opt-in, snapshot ownership, and credential-free cassettes.
Its parser checks run with `--self-test`; `--json` provides a machine-readable report.

Tests that discover configuration need an empty `XDG_CONFIG_HOME` to avoid loading
personal runtime settings. Set it outside the test process; do not mutate global
state from Rust tests. During this rewrite, checks use
`XDG_CONFIG_HOME=/tmp/agent-harness-empty-test-config`.

The `ci` profile uses CPU-count parallelism and no retries. Its default filter
excludes performance, binary, live, PTY, and native visual targets. An explicit
`--test` alone does not override that filter; opt-in commands need
`--ignore-default-filter`.

## Lanes

```bash
scripts/test-lanes.sh quality-gates
scripts/test-lanes.sh fast
scripts/test-lanes.sh integration
scripts/test-lanes.sh simulation
```

`fast` runs the everyday checks. `integration` divides the same deterministic suite
into two hash partitions. `simulation` runs the CLI scenario tests: identical
journals for repeated mock runs, denied edits, continuation, and source-preserving
forks. These use the runtime directly; there is no separate simulation engine.

Each lane writes its command, output, exit status and summary under
`target/test-lanes/<timestamp>/`. Use `--artifact-dir PATH` to choose another
location and `--dry-run` to inspect commands. `all-deterministic` runs the static,
scenario, fast, integration and supported PTY lanes.

## Backend test map

| Behavior | Checks |
| --- | --- |
| Run lifecycle, permissions, bounded scheduling, cancellation, recovery | `cargo nextest run -p harness-core --lib coord` |
| Journal locking, partial-tail recovery, replay and forks | `cargo nextest run -p harness-core --lib store`; core coordinator tests |
| Transport serialization, streaming, retry and attachments | `cargo nextest run -p harness-providers` |
| Files, tools, MCP, LSP, tasks and child journals | `cargo nextest run -p harness-tools` |
| CLI input, prompt streaming, authentication, configuration and session commands | `cargo nextest run -p harness` |

Tests assert externally visible behavior through the coordinator, CLI, transport or
storage boundary. Long-lived children, demotion and continuation share the same
checks as task results and journal recovery.

## Native backend checks

```bash
scripts/test-lanes.sh signoff-binary
```

This sets `HARNESS_BINARY_SIGNOFF=1` and runs the `binary_smoke` targets from
`harness`, `harness-core` and `harness-tools` serially. They exercise process cleanup,
MCP stdio, Git worktrees, reflinks, formatting, structural edits, filesystem
confinement, executable replacement and a real language server. Missing native
prerequisites fail the lane. Without the opt-in variable, tests return errors.

## Performance and sustained runs

```bash
scripts/test-lanes.sh perf
scripts/test-lanes.sh stress-offline --harness-bin target/release/harness
```

`perf` runs the preserved release TUI performance tests and the backend loopback
probe. The probe checks exact streamed output and durable completion, then records
startup, idle CPU, streaming latency and peak resident memory. `stress-offline`
repeats that fixture twenty times. Both lanes also check 256 completed child
sessions, retained descriptors and SIGINT cleanup through a local provider. These measurements do not predict network or
model latency. See [the measured workloads](../performance/backend-rewrite-2026-09-26.md).

Optional `scripts/test-lanes.sh coverage` uses cargo-llvm-cov with nextest and writes
LCOV plus a line-coverage summary. A missing baseline is recorded on the first run;
coverage is a diagnostic, not a reason to add low-value tests.

## Offline CLI smoke

```bash
bash scripts/harness-qa-dogfood.sh --self-test
```

The shell smoke checks compiled CLI workflows. Backend regression ownership stays
with the in-process tests above. It does not establish live-provider or visual
behavior.

## Deterministic signoff PTY lane

Run the PTY lane when changing TUI rendering, transcript behavior, viewport-sensitive flows, or
anything that needs the deterministic headless UI oracle:

```bash
scripts/test-lanes.sh signoff-pty
```

`signoff-pty` is a strict fail-closed lane (no soft `|| true` stages). Missing owners,
missing `cargo`, stage failures, or dual-binary journey failures fail the run and write
`pty-lane-verdict.txt`. Silent skip is forbidden.

This lane runs the PTY E2E tests single-threaded and writes manifest-backed visual evidence under
the configured artifact root. Legacy committed harness-testkit PTY snapshots were removed during
T5 slimming; current PTY evidence is generated under `target/pty-visual-artifacts/`, while retained
committed snapshots are owned by harness-tui deterministic snapshot tests. The harness-tui PTY test
target is fail-closed behind `HARNESS_TUI_PTY_SIGNOFF=1`, and its test binaries are excluded by the
workspace nextest default filter, so any direct invocation must pass `--ignore-default-filter` to
run real tests; without it nextest selects zero tests and exits non-zero. The signoff lane opts into
the real PTY captures. Do not parallelize PTY signoff.

Fail-closed stages (no `|| true`):

| Stage | What it proves |
|-------|----------------|
| `pty_prerequisites` | owner files exist; `cargo` on `PATH` (missing owner = FAIL) |
| `harness_testkit_pty_e2e` | testkit PTY E2E + visual artifact provenance |
| `harness_tui_pty_e2e` | harness-tui PTY E2E under `HARNESS_TUI_PTY_SIGNOFF=1`, including reply-capable emulation and canonical P0-06 artifacts |
| `harness_tui_p0_01_pty_recorded` | P0-01 full-surface dashboard round-trip: detach, open dashboard over the transcript, resize while owned, close, and restore the detached anchor display column and focus without losing the composer |
| `harness_tui_p0_02_pty_recorded` | P0-02 dense transcript navigation, reflow, detached-append, and helper lifecycle PTY regression |
| `harness_tui_p0_03_pty_recorded` | P0-03 boxed markdown, OSC-8, and event-driven streaming-fence PTY regression |
| `harness_tui_p0_04_pty_recorded` | P0-04 persistent multiline, queued send, interject, cancel-and-replace, and malformed-input decoder-coherence PTY regression |
| `harness_tui_p1_01_pty_recorded` | P1-01 slash-command palette with Tab text-accept and required-argument supply PTY regression |
| `harness_tui_p1_02_pty_recorded` | P1-02 reply-capable native PTY journey for Commands -> Settings chrome, tabs, restoration, stale pointer input, six-cell close target, and 80x24/120x40/160x50 alignment |
| `harness_tui_p1_03_pty_recorded` | serialized P1-03 native PTY owner for the staged startup reveal: first-seen identity/affordance/changelog ordering in every geometry, reduced-motion freeze on the complete frame, early CJK input during the reveal with double-width cell proof, and Unicode + Basic/Ascii artifacts at 80x24/120x40/160x50 |
| `harness_tui_p1_04_pty_recorded` | serialized P1-04 native PTY owner for Unicode and Basic/Ascii capability variants, following/detached/resize-burst-final/reduced-motion states, and 80x24/120x40/160x50 artifacts |
| `harness_tui_happy_path_pty` | compiled `harness` CLI mock happy path (`pty_happy_path_recorded`) |
| `p0_06_xterm_tests` | xterm.js structured collector, canonical viewport, runtime-branding, and evidence contract tests in real Chromium |
| `p1_02_xterm_tests` | dedicated JS owner for the shipped-binary P1-02 scenario, canonical close coordinates, and interaction contract |
| `p1_03_xterm_tests` | dedicated JS owner for the P1-03 startup-reveal scenario, staged capture contract, and never-blocked input assertion |
| `p1_04_xterm_tests` | dedicated JS owner for the P1-04 capability, resize ordering, structured-cell, and live-PTY resize contract |
| `xterm_harness_binary` | validates the shipped Harness build before browser capture; each scenario rebuilds from the recorded clean tree, copies, pre-hashes, executes, and post-hashes an isolated tested binary |
| `p0_06_xterm_80x24`, `p0_06_xterm_120x40`, `p0_06_xterm_160x50` | the compiled Harness mock TUI driven through a native PTY into xterm.js at each canonical size |
| `p1_02_xterm_80x24`, `p1_02_xterm_120x40`, `p1_02_xterm_160x50` | `target/debug/harness` driven through util-linux `script` into Chromium+xterm.js; captures modal/tab/breadcrumb/footer states and keyboard/mouse restoration history at each canonical size |
| `p1_03_xterm_80x24`, `p1_03_xterm_120x40`, `p1_03_xterm_160x50` | the exact compiled P1-03 integration-test owner executed in Chromium+xterm.js with the `p1-03-startup-reveal` unicode contract (welcome-complete and after-input captures including a CJK draft typed after the reveal settles; the sub-second transient frames are proven by the native PTY owner and the insta frame-sequence snapshots, which a browser harness cannot time) at each canonical size |
| `p1_03_xterm_basic_ascii` | the same P1-03 owner executed with the `basic-ascii` contract (dumb terminal, no color, reduced motion): an ascii draft typed immediately after first paint dismisses the reveal and echoes in the composer |
| `p1_04_xterm_80x24`, `p1_04_xterm_120x40`, `p1_04_xterm_160x50` | the shipped `harness tui --mock --deterministic` binary driven through util-linux `script` into Chromium+xterm.js with the P1-04 responsive-feedback contract at each canonical size |

The dedicated P1-02 owners can also be run directly:

```bash
env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 \
  cargo nextest run -p harness-tui --test p1_02_pty_recorded --test-threads 1 --ignore-default-filter
node --test scripts/qa/p1-02-modal-chrome.test.mjs
cargo build -p harness
node scripts/qa/web-terminal-visual-qa.mjs \
  --scenario p1-02-modal-chrome \
  --evidence-dir .omo/evidence/p1-02-modal-chrome-120x40 \
  --cols 120 --rows 40
```

Repeat the browser command with `80 24` and `160 50` for the other canonical geometries. Prerequisites are Linux PTY support, `cargo`, Node.js 20 or newer with npm, util-linux `script`, and executable `/usr/bin/chromium`; `npm ci --prefix scripts/qa` installs the pinned local xterm.js/Playwright packages. The scenario opens Settings only through Commands, verifies Harness-owned frame/title, Runtime/TUI tabs, breadcrumb, shortcut footer, Tab/Shift+Tab, Escape restoration, stale outside input, and mouse-close restoration. Automated assertions establish interaction and evidence contracts; visual parity still requires reviewing the emitted screenshots.

Each P1-02 and P1-04 xterm stage writes into the lane's ignored `target/test-lanes/.../signoff-pty/stages/<stage>/artifacts/` tree. Evidence includes indexed and final PNGs, `terminal.ansi`, `terminal-ansi.txt`, `terminal.txt`, `buffer.json`, `interactions.json`, `metadata.json`, `harness-binary-provenance.txt`, `artifact-manifest.json`, `PASS.json`, and `cleanup.json`; P1-04 captures also retain per-state buffer/text receipts. The binary receipt ties a just-completed `cargo build -p harness` to a stable clean HEAD/tree sampled before build and after PTY/browser termination, proves the built binary matches the isolated copy before launch, and post-hashes that copy after termination; the receipt itself is manifest-hashed. PASS is refused unless that chain remains unchanged, PTY/browser/profile/temp-root cleanup receipts are complete, visible runtime evidence contains Harness. The lane atomically allocates owner-only security-allowlisted `/tmp/harness-xterm-p1-02-*`, `/tmp/harness-xterm-p1-03-*`, and `/tmp/harness-xterm-p1-04-*` roots and trap-cleans them after evidence is copied without replacing the owned root inode during preparation.

The P0-06 owner feeds native PTY output into a reply-capable vt100 emulator and forwards generated cursor-position reports to the child. It asserts cells, cursor position/visibility, alternate-screen transitions, input modes, wrapping, and emulator scrollback. Under the lane-provided absolute `HARNESS_P0_06_ARTIFACT_DIR`, it records before/after ANSI, text, and structured-screen captures at 80x24, 120x40, and 160x50 plus a hash-verified manifest and aggregate cleanup receipt. Native captures truthfully identify the hashed `harness-tui` integration-test executable and its direct production entrypoint, `harness_tui::run_tui_with_options`; they do not relabel that owner binary as the shipped `harness` CLI. The P1-03 native owner writes its `manifest.json`, per-variant/per-size state receipts (`first-paint`, `complete`, `after-input`, `early-input`, `reduced-motion-first-paint`), per-capture `reveal-timeline.json` first-seen ordering receipts, `cleanup.json`, and aggregate `cleanup.json` under the lane-provided absolute `HARNESS_P1_03_ARTIFACT_DIR`; it proves the versioned identity row precedes the affordance rows, which precede the changelog, in every geometry and variant, that reduced motion freezes on the complete frame, that typing CJK text onto the alternate screen during the reveal dismisses the welcome and renders at double width, and that the child is reaped on every exit path. The P1-04 native owner separately writes its `manifest.json`, per-variant/per-size state receipts, `brand.json`, `cleanup.json`, and aggregate `cleanup.json` under the lane-provided absolute `HARNESS_P1_04_ARTIFACT_DIR`; its manifest hashes the actual owner executable and records the native PTY/emulator resize APIs. The P1-04 xterm stages compile, copy, hash, and execute that exact P1-04 integration owner in Chromium at 80x24, 120x40, and 160x50 through `p1-04-responsive-feedback`. They persist screenshot/ANSI/text/structured evidence plus a binary-provenance sidecar, require a newer parsed frame and coherent transcript row before each post-input capture, and fail when collected runtime state lacks Harness branding. The separate `xterm_harness_binary` and P0-06 xterm stages retain shipped `target/debug/harness` build/capture ownership.

For a combined deterministic closeout, use:

- `env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p0_03_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p0_04_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test p1_02_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_P1_03_ARTIFACT_DIR=<dir> cargo nextest run -p harness-tui --test p1_03_pty_recorded --test-threads 1 --ignore-default-filter`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_P1_04_ARTIFACT_DIR=<dir> cargo nextest run -p harness-tui --test p1_04_pty_recorded --test-threads 1 --ignore-default-filter`
- `node --test scripts/qa/p1-02-modal-chrome.test.mjs`
- `node --test scripts/qa/p1-03-startup-reveal.test.mjs`
- `node --test scripts/qa/p1-04-responsive-feedback.test.mjs`
- `env RUST_TEST_THREADS=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=<dir> cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 --ignore-default-filter --run-ignored only -E 'test(=scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit)'`


Snapshot reconciliation note: the `command_palette_renders_without_pty` and
`tool_lifecycle_rows_stay_ordered_without_pty` snapshots were reconciled to
match current render behavior (live composer placeholder line and ordered tool
lifecycle rows). The committed snapshots predated the current output. Updating them did not change
runtime behavior.

```bash
scripts/test-lanes.sh all-deterministic
```

`all-deterministic` runs `quality-gates`, then `simulation`, then `fast`, then `integration`, then `signoff-pty` only when PTY support checks
pass. Its PTY gate requires `cargo` on `PATH`, both PTY test files to exist, and
`HARNESS_TEST_LANES_SKIP_PTY` not set to `1`.

## Reader shutdown and terminal restoration

The offline Linux PTY check covers idle reader wakeups, shutdown during incomplete
UTF-8 and paste input, resize without keyboard input, terminal hangup, preserved
session handoffs, and initialization failures. It records raw terminal output and
termios comparisons. Hangup must return a typed reader error rather than exit by
signal; restoration cannot be inspected after the PTY master has closed.

```bash
cargo build --release -p harness-tui --all-features --example resource_probe --example rewrite_probe
python3 scripts/check-tui-restoration.py --binary target/release/examples/resource_probe \
  --live-binary target/release/examples/rewrite_probe --output /tmp/tui-restoration
```

Use `--record-reference-defects` only to retain a failing reference observation.
Zero context switches alone does not establish low CPU use: pair this check with
the serial runtime-resource measurements described in `docs/tui-rewrite.md`.
The reader's existing ordered-burst and bounded-queue nextest checks remain part
of verification. The new wake path has Linux evidence; other systems require
their own terminal checks.

## Live provider checks

```bash
HARNESS_LIVE_PROXY=1 \
HARNESS_LIVE_PROXY_CONFIG=/absolute/path/to/config.jsonc \
HARNESS_LIVE_PROXY_PROVIDER=your-provider \
HARNESS_LIVE_PROXY_MODEL=your-model \
scripts/test-lanes.sh signoff-live
```

The lane preserves the prompt/TUI prerequisite wrappers, then runs the bounded
`PONG` provider smoke and the public Exa MCP search check. Missing credentials,
configuration or explicit opt-in fails closed. The prerequisite wrappers alone do
not execute a provider turn. Do not treat their success as live signoff.

To check only public MCP discovery and web/code search:

```bash
HARNESS_MCP_LIVE_SIGNOFF=1 cargo nextest run --profile ci \
  -p harness-tools --test live_proxy_e2e --ignore-default-filter
```

The provider smoke stores redacted evidence under `artifacts/qa-evidence/` and uses
isolated sessions. `stress-live` reuses it with the same explicit environment.

## Native visual lane

Native visual signoff is local, ignored by default, and env-gated:

```bash
HARNESS_NATIVE_VISUAL=1 \
DISPLAY=<display> \
scripts/test-lanes.sh signoff-native
```

This lane runs the native visual tests single-threaded. In the current slim T5 surface it fails
closed unless `HARNESS_NATIVE_VISUAL=1` and `DISPLAY=<display>` are present, and preserves the native
visual metadata/artifact-root contract for local signoff tooling. Treat native screenshots as local
visual evidence, not a portable hash oracle. If native prerequisites are unavailable, use `signoff-pty`
for deterministic UI signoff.

Current stage command:

- `cargo nextest run -p harness-testkit --test native_visual_e2e --ignore-default-filter --run-ignored only --test-threads 1`
