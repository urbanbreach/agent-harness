# Build performance

Measured on 2026-09-23 at commit `4f966051d3ceb1209f8feb7cf4afbd5c61666f63`.

## Selected settings

Builds and tests use Wild 0.10.0 by default on x86-64 GNU/Linux, with
`debug = "line-tables-only"` for local development. Nextest remains the test runner.
Wild was fastest for executable and test recompilation at every debug level.
For a TUI library edit, its advantage over mold was small.

The dev profile now uses line tables, inherited by the test profile. This retains
file/line backtraces but omits local-variable debugging. Full debug information
remains available with `CARGO_PROFILE_DEV_DEBUG=2` or `CARGO_PROFILE_TEST_DEBUG=2`.

Run these commands from the repository root:

```bash
cargo build -p harness
cargo nextest run --profile ci --workspace --all-features
```

`.cargo/config.toml` selects Wild on x86-64 GNU/Linux and requires `clang`
and `wild` on PATH. Other targets retain their toolchain defaults.
This machine has the verified upstream Wild 0.10.0 binary
installed at `~/.local/bin/wild`. For another machine, install the
[upstream release](https://github.com/wild-linker/wild/releases/tag/0.10.0),
or build the pinned version with `cargo install --locked --version 0.10.0 wild-linker`.
CI installs the pinned Wild release with a SHA-256 check before compilation.
All Rust test jobs use nextest, including the explicit PTY and binary-smoke jobs.

To temporarily use the bundled LLD on this target:

```bash
RUSTFLAGS='-C linker=cc' cargo nextest run --profile ci --workspace --all-features
```

`RUSTFLAGS` replaces the configured Wild flags. Switching linkers causes an
initial rebuild.

Release optimization, incremental compilation, and Cargo's default job count
are unchanged. Release optimization and CI caching were not benchmarked here.

## Results

Wall-clock seconds, median of three measured runs after one warm-up per
configuration. These include Cargo and compilation, not just the linker.

| Debug information | Linker | CLI main edit | TUI render-function edit | Three test binaries recompiled |
| --- | --- | ---: | ---: | ---: |
| Full, previous default | LLD | 1.237 | 4.107 | 1.605 |
| Full | mold | 0.956 | 3.509 | 1.148 |
| Full | Wild | 0.699 | 3.428 | 0.974 |
| Line tables | LLD | 0.782 | 3.509 | 0.946 |
| Line tables | mold | 0.530 | 3.215 | 0.769 |
| Line tables | Wild | 0.465 | 3.180 | 0.682 |
| None | LLD | 0.730 | 3.361 | 0.827 |
| None | mold | 0.486 | 3.147 | 0.698 |
| None | Wild | 0.414 | 3.055 | 0.604 |

The selected settings reduced CLI-edit time by 62%, TUI-edit time by 23%, and
test-recompilation time by 58% relative to full debug information with LLD.
Disabling debug information entirely was fastest, but saved only another
0.05–0.13 seconds in these workloads while losing file/line backtraces.

The CLI binary shrank from 533.9 MiB with full debug information and LLD to
212.2 MiB with line tables and Wild. With no debug information and Wild it was
125.4 MiB. These are unoptimized development binaries.

Empty-target CLI builds with LLD took 47.07 seconds with full debug information,
45.06 seconds with line tables, and 42.65 seconds without debug information.
Each was measured once with warm filesystem and dependency-download caches.
Those observations do not establish a reliable clean-build ranking. The
baseline Cargo timing report attributes 18.84 seconds to harness-core and
14.61 seconds to harness-tui; a faster linker cannot eliminate that compilation.

## Method and evidence

- Intel i7-12800H, 20 logical CPUs, 62 GiB RAM, AC power, powersave governor.
- Rust 1.98.0, Cargo 1.98.0, bundled LLD 22.1.8, mold 2.42.1, Wild 0.10.0.
- Sequential workloads, 20 Cargo jobs, incremental compilation enabled.
- A frozen `git archive` snapshot and separate target directories for each
  debug level. Existing build outputs were not cleaned or used as the baseline.
- Linkers downloaded from upstream releases; archive SHA-256 digests checked
  against GitHub release metadata. No network during the measured builds.
- Every linker used the same Clang driver through a small logging wrapper.
  `RUSTFLAGS` remained fixed while the wrapper selected the linker. This reuses
  identical dependencies and avoids charging a flags-induced rebuild to a linker.
  The wrapper's process overhead is included for all candidates.
- CLI edits changed a `black_box` constant inside `main`. TUI edits changed a
  `black_box` constant inside `render_app`, rebuilding the dependent CLI.
  Edits existed only in the snapshot, and were restored afterward.
- Test recompilation changed comments in `composer_editing_test`,
  `attachment_lifecycle_test`, and `tool_order_capture_test`. This measures
  incremental test compilation and relinking, not a change to test logic.
- Linker order was shuffled deterministically each round. All nine configurations
  ran the 21 selected tests successfully, and every rebuilt CLI passed `--version`.
- One no-debug/LLD library sample was invalidated by a Cargo configuration change.
  The raw sample is retained; `library-correction.json` records its replacement.
- Differences of a few hundredths of a second should not be treated as stable
  rankings. In particular, mold and Wild were close on library edits.

Raw scripts, per-command logs, link timings, environment details, Cargo timing
reports, and JSON results are in `target/build-benchmark-2026-09-23/` on the
benchmark machine. `summary.json` contains the corrected samples and medians.
Temporary compilation caches were removed after validation; Cargo timing HTML
was preserved under `timings/`. These local artifacts are ignored by Git and
will be removed by `cargo clean`.

## Validation

The full deterministic run compiled 200 test binaries and ran 4,687 tests:
4,685 passed, two failed, and 13 were skipped. Both failures reproduce on the
untouched snapshot using LLD and full debug information:

- `permissions_docs_state_file_workspace_and_network_implications`
- `sessions_replay_docs_name_session_tools_and_no_side_effect_contract`

They assert existing documentation wording; no failure unique to Wild was found.
Logs are `validation.log` and `baseline-failures.log` in the evidence directory.
Formatting and static test-suite gates passed.

The Wild release build passed with the existing fat-LTO release settings.
The binary passed `--version`, and its ELF
comment confirmed Wild 0.10.0. This was a compatibility check, not a comparison
of release optimization settings.

The direct Wild configuration also passed all 21 selected tests without the
benchmark linker wrapper.

After making Wild the default, ordinary `cargo nextest run` passed the same 21
tests. Small executable probes confirmed both the default Wild linker and the
documented bundled-LLD fallback. CI YAML, linker setup in every compiling job,
nextest installation in each test job, shell syntax, and the release download,
checksum, and installation were checked locally. The CI package-install step
and GitLab pipeline were not executed locally.
