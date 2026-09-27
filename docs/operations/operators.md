# Update and peer commands

## Local updates

```bash
harness update check
harness update download --url file:///tmp/new-harness --expected-sha256 SHA256
harness update apply --artifact-path ./downloaded-harness --target ./harness
harness update restart --target ./harness
harness update run --target ./harness
```

Check reads `.agent-harness/update-manifest.json` and writes an update-check
receipt. The manifest needs a semantic `version`; it may also contain `channel`,
`min_version`, `download_url` and `sha256`. No network request occurs during check.
An unavailable or invalid check returns a nonzero status with a structured report.

Download accepts HTTPS, loopback HTTP and local file URLs. The maximum artifact
size is 256 MiB. A supplied SHA-256 must match before the artifact is published.
Use `--dest-dir` to change the default `.agent-harness/downloads` directory.

Apply stages the replacement beside the target and preserves ordinary executable
permission bits. It writes `TARGET.backup` and refuses to overwrite an existing
backup. A failed final directory sync triggers restoration from that backup.
The target defaults to the current executable.

Restart replaces the current process on Unix. It starts the target in the selected
workspace with no update arguments, so it cannot repeat the update command.
Run checks, downloads, applies and restarts in that order; an unsuccessful step
stops the pipeline. These operations require explicit commands. Nothing updates
the binary during startup or session inspection.

## Stdio peer diagnostic

```bash
harness --cwd ./project agent stdio --command 'my-peer' --json
```

This command launches the supplied shell command, binds a local diagnostic
session, exchanges a newline-delimited initialization frame and closes the peer.
Frames are limited to 1 MiB and each exchange has a 30-second timeout. Shutdown
terminates the process group and waits for the direct child. Process-tree control
is currently available on Unix; other platforms report unavailable.

The result reports connection, binding and exchange success. The legacy
`meets_agent_mode_contract` field covers those transport checks. It does not prove
that the peer implements an ACP handshake or can execute an agent task. Command
credentials are redacted from reports. Failed exchanges return a nonzero status.
