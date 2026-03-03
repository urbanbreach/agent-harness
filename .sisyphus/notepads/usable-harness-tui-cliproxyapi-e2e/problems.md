# Unresolved Problems / Technical Debt

- 2026-03-03: Interactive PTY teardown currently uses forced child termination in test cleanup instead of graceful `q`-driven exit; investigate upstream TUI shutdown path to restore clean exit assertions.
- 2026-03-03: Diff tab in this tree reports `diff artifact missing:` during golden-path PTY capture, indicating artifact resolution behavior differs from prior expectations (`@@ -1,3 +1,3 @@` view).
