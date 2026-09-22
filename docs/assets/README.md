# README media

[`harness-demo.gif`](harness-demo.gif) records the real Harness TUI in a
100-column, 28-row terminal. It shows a prompt, the scripted `Hello world` reply,
the command palette, an edit approval, and the resulting diff.
[`harness-tui.png`](harness-tui.png) is a still frame from the edit scene. Both
READMEs use these files.

The recording uses `tui --mock` and the built-in `golden_path_interactive` scenario
in temporary workspaces. It does not use live credentials or make provider
requests. Frame timing is set for readability; the animation is not a latency
measurement.

## Regenerate

On Linux, install Chromium, FFmpeg, Node.js, util-linux's `script`, and the
Adwaita Mono font. The existing capture helper reads the font at
`/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf`.

From the repository root:

```bash
cargo build -p harness --locked
npm ci --prefix scripts/qa
node scripts/qa/capture-readme-demo.mjs
```

Set `HARNESS_QA_BROWSER` if Chromium is installed somewhere other than
`/usr/bin/chromium`. The script reuses the xterm.js and PTY helpers in
[`scripts/qa/lib`](../../scripts/qa/lib), checks the reply and edited file appear, scans
the terminal output for secrets, and replaces the two media files only after
encoding succeeds. It removes the temporary workspace and browser profile.

Inspect the animation and still image before committing them. Keep the mock
caption and image descriptions accurate if the recorded sequence changes.
The READMEs request the still image for reduced-motion preferences and provide
a direct still-image link for viewers that do not support the media query.
