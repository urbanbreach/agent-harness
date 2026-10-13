# README media

[`harness-demo.gif`](harness-demo.gif) is a real session in the Harness TUI at
200 columns by 54 rows, or 1920 by 1080 pixels, which is what a maximized
terminal shows on a 1080p screen. Claude Sonnet 5.5 at medium effort works in a
checkout of this repository and gets one prompt, "The docs have drifted a lot
from the code. Go through every page in docs/ and fix anything that's out of
date."

The prompt doesn't mention skills or subagents. In the recorded run the agent
loads the `unslop` skill because `AGENTS.md` asks for it on prose work, splits
`docs/` into five sets of pages and gives each set to a `task` subagent, then
sends three more after pages the first round left. While they work, the
Subagents pane names each child by its agent type and shows its model, tokens
per second and cost, and a card shows the parent waiting on them. The children
run on Claude Sonnet 5.5 at medium effort because that is the `task` model in
the config described below; a `scout` or `reviewer` would show its own
configured model.

Two things in the recording are scripted input rather than model output. Once
the wait card appears, the script clicks the first subagent in it, shows that
child's live transcript for 20 seconds and presses Esc to return. Then it types
a steering message, "In your final reply, put the changes in a table with one
row per file.", while the subagents are still working. The agent reads it once
the wait ends, and its final reply is that table. The whole turn took about five
and a half minutes. [`harness-tui.png`](harness-tui.png) is the final frame.
Both READMEs use these files.

The checkout is pinned to `ce530837`. Its docs lag the code, and the next
commit, `8512d08f`, fixed them by hand, so the agent has real drift to find
on every run.

The recording runs the release binary against a throwaway clone and session
directory. It differs from a normal session in these ways:

- `HOME` points at the temporary directory, so the header reads
  `~/agent-harness`. Credentials still come from your real Harness home. The
  project's `.harness/skills` and your `~/.agents/skills` are copied in, so
  the agent sees the skills it would see in your own checkout.
- Your gitignored `harness.jsonc` (or `harness.json`) in this repository is
  passed in as config, so each subagent runs the model you set for its agent
  type, with your providers and MCP servers. The demo then sets the main model
  itself: it drops the config's `agent.default` and, if the config has a
  provider map, adds the Claude subscription to it, since a model set in config
  must come from a provider the map names.
- LSP is off. A cold rust-analyzer only answers "server cancelled the request"
  on a workspace this size.
- Eval is denied, so the transcript shows individual tool cards instead of
  code cells.
- Paths outside the checkout and repeated identical calls are denied instead of
  asking, because nobody is there to answer a permission prompt. The script
  fails if one appears anyway.
- Cargo builds, if the agent starts any, go to `target/readme-demo` in this
  repository rather than the temporary directory.

The GIF is a time-lapse. The script takes a frame whenever the screen's text
changes (timers and token rates aside), at most twice a second, and each frame
plays for a fifth of a second, or two fifths inside the subagent's view. Past
300 such frames it keeps only every second, third or nth one, so the live part
never runs longer than a minute. It is not a latency measurement.

## Regenerate

You need Linux with Chromium, FFmpeg, Node.js, util-linux's `script`, a Rust
toolchain, the Adwaita Mono font at
`/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf`, and a Claude subscription
signed in to Harness.

From the repository root:

```bash
cargo build --release -p harness --locked
npm ci --prefix scripts/qa
node scripts/qa/capture-readme-demo.mjs
```

The model defaults to `anthropic-subscription/claude-sonnet-5-5/medium`. Set
`HARNESS_DEMO_MODEL` to another `provider/model[/variant]` reference
(`harness models` lists them) to record a different model. Set
`HARNESS_QA_BROWSER` if Chromium is not at `/usr/bin/chromium`. A run takes
about five minutes and uses your provider account.

The script reuses the xterm.js and PTY helpers in
[`scripts/qa/lib`](../../scripts/qa/lib). It fails unless the agent waits on
subagents and the turn finishes. Sonnet sometimes keeps a docs pass to itself,
so run the script again when that happens. It scans every frame and
the raw terminal output for secrets, and replaces the two files only after
encoding succeeds. Afterwards it removes the temporary checkout and browser
profile, the edit-attribution directory the run created under your Harness
home, and the Claude subscription restart binding of every session it started.

Model output differs between runs. Watch the result before committing it, and
update the caption and the image descriptions in both READMEs if the session
changes. The READMEs serve the still image to readers who prefer reduced motion
and link to it for viewers that ignore the media query.
