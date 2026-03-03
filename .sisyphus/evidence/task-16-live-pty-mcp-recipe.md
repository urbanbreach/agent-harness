# Task 16 live CLIproxyAPI TUI evidence capture (pty-mcp)

This is the reproducible, agent-driven procedure for generating the required artifacts:

- `.sisyphus/evidence/task-16-live-tui.png`
- `.sisyphus/evidence/task-16-live-tui-finished.png`
- `.sisyphus/evidence/task-16-live-log.txt`

## Preconditions

1. CLIproxyAPI is running locally and reachable at the base URL in your config.
2. Config enables Responses mode (`api_mode: "responses"`).
3. `OPENAI_API_KEY` (or equivalent provider token env var) is set if required by your proxy.

## 1) Capture headless prompt command output log

Run:

```bash
cargo run -p harness -- prompt --text "Say hello" --config configs/harness.example.jsonc |& tee .sisyphus/evidence/task-16-live-log.txt
```

Expected: exit code `0`, and the log includes a successful run completion line.

## 2) Capture live TUI screenshots with pty-mcp

### 2.1 Spawn TUI session

Use `pty-mcp_terminal_spawn` with:

- `cwd`: repo root
- `shell`: `/bin/bash`
- `args`: `[-lc, "cargo run -p harness -- tui --config configs/harness.example.jsonc"]`

### 2.2 Submit prompt

Use `pty-mcp_terminal_write` to send:

```text
Say hello\r
```

### 2.3 Wait for streaming to begin and capture screenshot

1. Poll with `pty-mcp_terminal_wait` + `pty-mcp_terminal_screenshot(format="text")` until output visibly includes streamed delta text or provider activity markers.
2. Call `pty-mcp_terminal_screenshot(format="png")`.
3. Save returned PNG bytes to:

`.sisyphus/evidence/task-16-live-tui.png`

### 2.4 Wait for completion and capture finished screenshot

1. Poll with `pty-mcp_terminal_wait` + text screenshot until run completion marker appears (for example, request finished / run finished status in UI).
2. Call `pty-mcp_terminal_screenshot(format="png")` again.
3. Save returned PNG bytes to:

`.sisyphus/evidence/task-16-live-tui-finished.png`

### 2.5 Exit cleanly

Send:

```text
q
```

Then kill the PTY session with `pty-mcp_terminal_kill`.

## Notes

- Keep this workflow manual/gated; do **not** add it to default CI.
- If streaming never starts, verify proxy reachability and config `api_mode` first.
