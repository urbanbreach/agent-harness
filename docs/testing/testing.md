# Testing

Use nextest. Backend tests inject providers, clocks, environment lookups and
workspaces. HTTP fixtures listen on loopback. Eval checks may start local
interpreters. Native signoff, public services and PTY captures have separate lanes.

Before adding a test, find the existing behavior check and extend it when possible.
Start with a failing assertion for a plausible regression, make it pass, and stop.
Do not add coverage for getters, derived types, delegation, or impossible states.

## Everyday checks

Install Node.js 24 or newer on `PATH` for JavaScript eval tests and Python for
Python eval tests. `scripts/test-lanes.sh eval` opts into the ignored kernel and
tool-composition tests with `HARNESS_EVAL_SIGNOFF=1`. These use JavaScript and
Python by default. Set `HARNESS_EVAL_LANGUAGES=js,py,rb,jl` to include Ruby and
Julia, with those interpreters installed. Missing runtimes fail the selected
checks. No npm packages are needed for the eval runtime.

```bash
python3 scripts/check-test-suite-gates.py --format
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --profile ci --workspace --all-features
```

The format gate checks backend Rust without traversing the retained TUI modules.
It requires backend files below 500 lines, rejects process-global environment and
cwd mutation and sleeps in non-native tests, checks native/live opt-in code,
verifies snapshot source paths and scans cassettes for credential-shaped values.
Its parser checks run with `--self-test`; `--json` provides a machine-readable report.

Tests that discover configuration need an isolated Harness home to avoid loading
personal settings. Use a temporary directory and an injected `HARNESS_HOME` lookup
through `CliDeps::with_env` or `ConfigLoadContext`; do not mutate global environment
state from Rust tests. For command-line checks, set `HARNESS_HOME` to a fresh
temporary directory outside the test process.

The `ci` profile uses CPU-count parallelism and no retries. Its default filter
excludes performance, binary, live, PTY, and native visual targets. An explicit
`--test` alone does not override that filter; opt-in commands need
`--ignore-default-filter`.

## GitHub Actions

`.github/workflows/ci.yml` runs on pushes, pull requests, and manual dispatches
using Ubuntu 24.04. It runs test-suite, branding and release-script checks,
workspace formatting and Clippy with warnings denied, locked development/release
builds, deterministic nextest, serial testkit/TUI PTY lanes, CLI binary smoke and
the coverage ratchet. Rust comes from `rust-toolchain.toml`; Node.js 24 and
Python 3.12 support eval and
subprocess fixtures. Gate scripts also require ripgrep. No Clang or Wild install
is needed. JUnit reports, PTY visuals, coverage, and binaries are retained for
seven days, including test artifacts on failure. The six-hour performance lane
is in `perf.yml`, scheduled weekly or launched manually, not on each push.

Secret scanning uses the free, checksum-pinned Gitleaks CLI with redacted output,
not its licensed organization action. It extends the default rules via
`.gitleaks.toml`, excludes test-only paths, and records reviewed historical
fixture fingerprints in `.gitleaksignore`. History scanning also opens nested
archives, up to five levels deep. Production-source and documentation
paths are not broadly exempted. The previous template-based SAST job was dropped:
GitHub CodeQL has no Rust analysis equivalent. Clippy is a code-quality gate,
not a substitute for security analysis.

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

CI runs all four CLI smoke cases with:

```bash
HARNESS_BINARY_SIGNOFF=1 cargo nextest run --profile ci -p harness --test binary_smoke --ignore-default-filter
```

These tests are not ignored; `--run-ignored only` would select none. The full
`harness-core` binary target stays in local native signoff, not hosted CI. Its
Landlock check requires filesystem ABI 5 ([Linux 6.10 or newer](https://docs.kernel.org/6.10/userspace-api/landlock.html)), which Ubuntu
24.04 hosted runner kernels do not guarantee; its reflink check also needs a
supporting filesystem. A runner label is not proof of either capability. Native
`harness-tools` signoff additionally requires the ast-grep CLI on `PATH`; the
deterministic suite does not run those environment-gated structural-edit checks.

## Performance and sustained runs

```bash
scripts/test-lanes.sh perf
scripts/test-lanes.sh stress-offline --harness-bin target/release/harness
```

`perf` runs all workspace tests selected by the release `perf` profile and the backend
loopback probe with five repetitions. The probe checks exact streamed output and
durable completion, then records startup, idle CPU, streaming latency and peak
resident memory. `stress-offline`
repeats that fixture twenty times. Both lanes also check 256 completed child
sessions, retained descriptors and SIGINT cleanup through a local provider. These measurements do not predict network or
model latency. See [the measured workloads](../performance/backend-rewrite-2026-09-26.md).

Optional `scripts/test-lanes.sh coverage` uses cargo-llvm-cov with nextest and writes
LCOV plus a line-coverage summary. A missing baseline is recorded on the first run;
the lane fails if line coverage, rounded to two decimal places, drops below
`docs/testing/test-suite-coverage-baseline.txt`. Coverage is not a reason to add
low-value tests.

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

`signoff-pty` records every failed stage and returns a nonzero lane status. It
continues after stage failures and writes `pty-lane-verdict.txt` under
`signoff-pty/stages/harness_tui_happy_path_pty/artifacts/`. Missing prerequisites
fail the lane rather than silently skipping it.

The lane runs PTY tests single-threaded and sets `HARNESS_TUI_PTY_SIGNOFF=1`
for the TUI owners. Many owner functions return without a capture when that
variable is absent, so a direct test success without it is not signoff. The
`pty_e2e` binary is excluded by the nextest default filter; direct commands need
`--ignore-default-filter`. Testkit uses `HARNESS_VISUAL_ARTIFACT_DIR`, defaulting
to `target/pty-visual-artifacts/`; recorded TUI owners use the stage directories
listed below. Do not parallelize PTY signoff.

Stages:

| Stage | What it proves |
|-------|----------------|
| `pty_prerequisites` | checks owner files, `cargo`, util-linux `script`, Node.js, npm and `/usr/bin/chromium`; missing prerequisites fail the lane |
| `harness_testkit_pty_e2e` | testkit PTY E2E + visual artifact provenance |
| `harness_tui_pty_e2e` | harness-tui PTY E2E under `HARNESS_TUI_PTY_SIGNOFF=1`, including reply-capable emulation and canonical P0-06 artifacts |
| `harness_tui_p0_01_pty_recorded` | dashboard round-trip restores the detached anchor and focus |
| `harness_tui_p0_02_pty_recorded` | dense navigation, reflow, detached append and helper cleanup |
| `harness_tui_p0_03_pty_recorded` | recorded markdown and event-driven streaming fence |
| `harness_tui_p0_04_pty_recorded` | recorded multiline composer shortcuts |
| `harness_tui_p1_01_pty_recorded` | slash-command completion and required arguments |
| `harness_tui_p1_02_pty_recorded` | modal chrome at 80x24, 120x40 and 160x50 |
| `harness_tui_p1_03_pty_recorded` | records startup-reveal terminal states under the lane-provided artifact directory |
| `harness_tui_p1_04_pty_recorded` | records responsive-feedback terminal states under the lane-provided artifact directory |
| `harness_tui_happy_path_pty` | compiled `harness` CLI mock happy path (`pty_happy_path_recorded`) |
| `p0_06_xterm_dependencies` | installs the pinned QA npm dependencies |
| `p0_06_xterm_tests` | runs `npm test --prefix scripts/qa` (all QA `*.test.mjs` files) |
| `p1_02_xterm_tests` | dedicated JS owner for the shipped-binary P1-02 scenario, canonical close coordinates, and interaction contract |
| `p1_03_xterm_tests` | dedicated JS owner for the P1-03 startup-reveal scenario, staged capture contract, and never-blocked input assertion |
| `p1_04_xterm_tests` | dedicated JS owner for the P1-04 capability, resize ordering, structured-cell, and live-PTY resize contract |
| `xterm_harness_binary` | builds `harness` before browser capture; each browser scenario separately builds and hashes the executable it runs |
| `p0_06_xterm_80x24`, `p0_06_xterm_120x40`, `p0_06_xterm_160x50` | the compiled Harness mock TUI driven through a native PTY into xterm.js at each canonical size |
| `p1_02_xterm_80x24`, `p1_02_xterm_120x40`, `p1_02_xterm_160x50` | `target/debug/harness` driven through util-linux `script` into Chromium+xterm.js; captures modal/tab/breadcrumb/footer states and keyboard/mouse restoration history at each canonical size |
| `p1_03_xterm_80x24`, `p1_03_xterm_120x40`, `p1_03_xterm_160x50` | the copied P1-03 integration-test owner in Chromium+xterm.js; waits for the complete welcome surface, captures it, types a CJK draft and captures the composer |
| `p1_03_xterm_basic_ascii` | the same owner at 120x40 with a dumb terminal, no color and reduced motion; types an ASCII draft after the `Beta` marker appears |
| `p1_04_xterm_80x24`, `p1_04_xterm_120x40`, `p1_04_xterm_160x50` | the copied `p1_04_pty_recorded` integration-test owner driven through util-linux `script` into Chromium+xterm.js with the Unicode responsive-feedback contract; each journey ends its resize burst at 120x40 |

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

Repeat the browser command with `--cols 80 --rows 24` and `--cols 160 --rows 50`
for the other sizes. Install the pinned xterm.js and Playwright packages with
`npm ci --prefix scripts/qa`. Prerequisites are Linux PTY support, `cargo`,
Node.js 20 or newer with npm, util-linux `script` and executable
`/usr/bin/chromium`. The browser runner needs a clean source tree. Automated
assertions check interaction and artifact contracts; review the PNGs for visual
parity.

Browser stages write to `signoff-pty/stages/<stage>/artifacts/` under the lane
artifact root. Successful captures include PNGs, `terminal.ansi`,
`terminal-ansi.txt`, `terminal.txt`, `buffer.json`, `interactions.json`,
`metadata.json`, `harness-binary-provenance.txt`, `artifact-manifest.json`,
`PASS.json` and `cleanup.json`. The browser runner refuses a dirty source tree.
It builds and copies the selected executable, checks the source tree again after
cleanup, and verifies that the executable copy did not change during the run.

The P0-06 native owner uses a reply-capable vt100 emulator, including cursor-position
replies, cells, wrapping and scrollback. `HARNESS_P0_06_ARTIFACT_DIR` selects its
artifact directory. The lane also sets `HARNESS_P1_03_ARTIFACT_DIR` and
`HARNESS_P1_04_ARTIFACT_DIR` for the recorded startup and responsive-feedback
owners. Native receipts identify the integration-test executable and production
TUI entrypoint. Browser receipts identify either the copied CLI or the copied
integration-test owner; they are not interchangeable.

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


The committed deterministic snapshots and generated PTY captures have different
owners. Update a snapshot only after checking the rendered cells and the test
that records them; a passing PTY lane does not rewrite committed snapshots.

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

This lane selects the ignored `native_visual_ghostty_smoke` test single-threaded.
That test checks only `HARNESS_NATIVE_VISUAL=1` and the presence of `DISPLAY`; it
does not launch a terminal or capture screenshots. A passing lane is environment
preflight, not visual signoff. Use `signoff-pty` for deterministic UI checks and
review the generated screenshots separately.

Current stage command:

- `cargo nextest run -p harness-testkit --test native_visual_e2e --ignore-default-filter --run-ignored only --test-threads 1`
