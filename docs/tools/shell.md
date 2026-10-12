# Shell commands

`bash` requires Bash on `PATH` when confinement is off. It executes an approved
command in the workspace, or an approved `workdir` (`cwd` is an alias). It
supports literal arguments, pipelines, command lists and quoted here-documents.
The parser rejects expansion, substitutions, shell background syntax (`&`),
shell definitions and commands that bypass its command checks. Executable and
working-directory allowlists apply when configured. These checks are not an OS
sandbox.

Use [monitor](monitor.md) for a long build, test, server, or log whose output
will announce a condition. It returns immediately and uses the same command
permissions and sandbox policy as bash.

```json
{"command":"cargo check","description":"Check the workspace","timeout":120000}
```

Foreground calls default to a two-minute timeout. `timeout` (alias `timeout_ms`)
accepts 1 through 3,600,000 milliseconds. Each foreground output stream retains
at most 512 KiB while continuing to drain its pipe. The result reports truncation
and the exit status. Cancellation and timeout terminate the process group and
reap the child before returning. Descendants cannot keep output pipes open after
their command completes.

Set `run_in_background: true` (alias `is_background`) or `block_until_ms: 0`
to start a coordinator-owned background command. Any `block_until_ms` value
uses this path; positive values wait up to that budget, capped at one hour,
before returning a completed result or a background handle. Background execution
defaults to a ten-hour timeout when `timeout` is omitted; an explicit timeout
still has the one-hour maximum.

Background results contain `task_id` and `output_file`. Use
`get_command_or_subagent_output`, `wait_commands_or_subagents`, or
`kill_command_or_subagent` to inspect, wait for, or cancel owned commands.
Background previews retain at most 40,000 bytes, and the redacted output file
retains at most 8 MiB. Pipes continue to drain after those limits.

The child receives only `PATH`, `TERM`, `TMPDIR`, `TEMP`, `TMP`, `LANG`,
`LC_ALL` and `LC_CTYPE`. Defaults supply `PATH` and `TERM` when absent.
Credentials, shell startup variables and exported shell functions are excluded.
On Linux, the parent becomes nondumpable before spawning the command, preventing
the child from reading its environment through `/proc`.

## Filesystem confinement

`HARNESS_OS_SANDBOX_POLICY` selects optional confinement. The registry captures the
setting when constructed; CLI construction uses its supplied environment lookup.

| Policy | Workspace access |
| --- | --- |
| `off` (default) | No OS filesystem confinement |
| `workspace_write` | Read and write |
| `read_only`, `strict` | Read only |

Non-off policies require Linux Landlock filesystem ABI 5 and a Landlock-enabled
`/usr/bin/setpriv`. Unknown policy values, missing support and failed confinement
stop execution. There is no fallback to an unconfined shell.

The child can read the workspace, its session directory, a private temporary
directory and the existing system roots `/usr`, `/bin`, `/lib`, `/lib64`, `/etc`,
`/dev` and `/proc`. It can write its session and temporary directories, plus
`/dev/null` and `/dev/zero`. Workspace writes depend on the selected policy.
Executables and data outside these roots are denied. Temporary-directory
variables point to the private directory, which is removed after process cleanup.

Confinement applies in `setpriv` before it replaces itself with Bash. The parent
remains unrestricted. These policies restrict filesystem access; they do not
claim network or process isolation. See the
[setpriv documentation](https://github.com/util-linux/util-linux/blob/master/sys-utils/setpriv.1.adoc).

Native verification covers environment filtering, parent-environment protection,
outside reads, workspace writes under each policy, scratch-directory cleanup,
invalid policy rejection and cancellation of descendants:

```bash
HARNESS_BINARY_SIGNOFF=1 cargo nextest run --profile ci -p harness-tools \
  --test binary_smoke --ignore-default-filter
```
