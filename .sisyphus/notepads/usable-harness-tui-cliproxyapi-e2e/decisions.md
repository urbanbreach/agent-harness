# Decisions

- 2026-03-03 (Task 15): Keep PTY assertions aligned with current rendered UI text (`Prompt`, `Permission Requested`, `diff artifact missing:`) rather than historical event labels (`RunFinished`, `PermissionRequested`).
- 2026-03-03 (Task 15): Add a dedicated PTY test `pty_e2e_tui_interactive_prompt_streams_response` with local wiremock SSE fixture and deterministic config generation.
- 2026-03-03 (Task 15): Preserve deterministic visual checkpointing by anchoring diff capture on stable text, not variable path rows.
