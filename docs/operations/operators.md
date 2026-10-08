# Stdio peer diagnostic

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
