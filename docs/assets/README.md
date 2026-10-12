# README media

[`harness-demo.gif`](harness-demo.gif) is a real session in the Harness TUI at
100 columns by 32 rows. A signed-in model gets one prompt, "cargo test is
failing. Find the root cause and fix it.", in a small Rust crate called `tempo`
whose duration parser drops trailing digits. Nothing the agent does is
scripted: it plans, runs the tests, reads the code, patches `src/lib.rs` and
checks its work. [`harness-tui.png`](harness-tui.png) is the final frame. Both
READMEs use these files.

The recording runs the release binary against a throwaway Git repository and
session directory. It differs from a normal session in three ways:

- `HOME` points at the temporary directory, so the header reads `~/tempo`.
  Credentials still come from your real Harness home.
- LSP is off. A cold rust-analyzer only answers "server cancelled the request"
  within a one-minute run.
- Eval is denied, so the transcript shows individual tool cards instead of
  code cells.

Frames are sampled about four times a second and played back faster. When the
screen's text stops changing (timers and token rates aside), the script keeps
under a second of frames, so long model waits shrink. The animation is not a
latency measurement.

## Regenerate

You need Linux with Chromium, FFmpeg, Node.js, util-linux's `script`, a Rust
toolchain, the Adwaita Mono font at
`/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf`, and a provider you have
signed in to.

From the repository root:

```bash
cargo build --release -p harness --locked
npm ci --prefix scripts/qa
HARNESS_DEMO_MODEL=openai-codex/gpt-6.1-sol node scripts/qa/capture-readme-demo.mjs
```

`HARNESS_DEMO_MODEL` takes a `provider/model` reference (`harness models` lists
the available models); leave it unset to use your default. Set
`HARNESS_QA_BROWSER` if Chromium is not at `/usr/bin/chromium`. The run takes
one to two minutes and uses your provider account.

The script reuses the xterm.js and PTY helpers in
[`scripts/qa/lib`](../../scripts/qa/lib). It fails unless the turn finishes and
`cargo test` passes afterwards, scans every frame and the raw terminal output for
secrets, and replaces the two files only after encoding succeeds. Afterwards it
removes the temporary workspace, the browser profile and the edit-attribution
directory the run created under your Harness home.

Model output differs between runs. Watch the result before committing it, and
update the model name in the README caption and the image descriptions in both
READMEs if the session changes. The READMEs serve the still image to readers who
prefer reduced motion and link to it for viewers that ignore the media query.
