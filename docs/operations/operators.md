# Stdio peer diagnostic

```bash
harness --cwd ./project agent stdio --command 'my-peer' --json
```

This command executes the supplied shell command in the selected workspace. Use
only a command you trust. It binds a local diagnostic session, sends
`{"method":"initialize"}` as a newline-delimited frame, reads one reply and
closes the peer. Frames are limited to 1 MiB and each exchange has a 30-second
timeout. Shutdown terminates the process group and waits for the direct child.

The result reports connection, binding and exchange success. The legacy
`meets_agent_mode_contract` field covers those transport checks. It does not prove
that the peer implements an ACP handshake or can execute an agent task. Command
credentials are redacted from reports. Success returns 0; a failed exchange
prints the report and returns 1. Argument parsing errors return 2.

## Managed worktrees

```bash
harness worktree list --workspace ./project
harness worktree list --workspace ./project --all
harness worktree remove SLUG --workspace ./project --keep-branch
harness worktree cleanup --workspace ./project
```

All worktree commands return JSON and use Git. Listing normally includes only
managed checkouts; `--all` includes other registered worktrees. Remove selects a
managed slug, while cleanup attempts every managed checkout and reports both
successes and failures. Primary and unmanaged worktrees cannot be removed.

Removal refuses dirty worktrees unless `--force` is set. It also deletes the
checkout's `harness/wt-*` branch unless `--keep-branch` is set. Cleanup can remove
some checkouts before reporting a failure; it is not an all-or-nothing operation.
Inspect the list before removing anything. See
[workspace data](workspace-data.md#worktrees) for storage paths.

## Local support diagnostics

Use `dashboard status` for catalog and configuration status, `sessions inspect`
for one session's readiness, and `sessions crash-scan` before explicit recovery.
None of those commands starts provider work. `sessions reopen` may repair saved
history, while `sessions continue` opens it for live TUI work.

For a support report, prefer `sessions export` or `trace` over copying raw
session files. They redact known credentials and omit raw tool payloads.
`wrap` also packages workspace files, so review what the workspace contains
before sharing its output. These commands write locally, not to a remote
service. See [saved sessions](sessions.md) for flags and output formats.
