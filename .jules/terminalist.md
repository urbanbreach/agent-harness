## 2025-02-28 - Avoid Phantom Commands in TUI Hints
**Learning:** The model switcher's empty state text mentioned an "/auth" slash command which isn't available as a primary workflow command (only "/connect" or CLI `harness auth login`).
**Action:** Always ensure any keyboard shortcuts or slash commands shown in empty states and status lines are explicitly defined and intended to be the primary user flow. Use the existing CLI output or main status banner logic as a reference for correct command semantics.
