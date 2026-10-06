# Permissions

The coordinator checks tool membership and policy before execution. Permissions
are application approval checks. They do not confine an approved process at the
operating-system boundary.

## Rules and defaults

Rules use `allow`, `ask` or `deny`. The last matching rule wins within each policy.
Shared policy and the acting agent's role policy then combine: deny wins, followed
by ask, then allow. A grant or YOLO mode cannot override a deny.

Permission names are `bash`, `edit`, `read`, `question`, `task`, `webfetch`,
`websearch`, `codesearch`, `lsp`, `external_directory`, and `doom_loop`. Native
session and skill tools also check their own IDs. Stdio MCP checks `bash`;
HTTP MCP checks `network`. The legacy `shell` configuration name maps to `bash`.

Omitting `permission`, or using scalar `allow`, allows ordinary operations with
these defaults:

- Outside-workspace paths and repeated identical calls require approval.
- Reads matching `*.env` or `*.env.*` require approval; `*.env.example` is allowed.
- Questions stay denied unless the acting profile enables them.

Scalar `ask` or `deny` applies across the public permission kinds. Pattern maps
are available for bash, edit, task, read and external directories:

```jsonc
{
  "permission": {
    "bash": { "*": "deny", "git status*": "allow" },
    "edit": { "*": "ask", "docs/**": "allow" }
  }
}
```

JSON/JSONC source order is significant. Later configuration layers append their
patterns after inherited entries. Replacing a pattern moves it to the later
position; a scalar replaces that kind's map. Legacy rule arrays retain their
explicit order. An unmatched selector defaults to ask.

Tools denied for every argument are omitted from provider tool lists. A partial
allowance keeps the tool visible; execution still checks its actual arguments.

## Approvals and remembered grants

An ask commits a permission request and waits. A configured timeout expires as a
denial. Cancellation resolves waiting approvals and prevents the tool from
starting. Enabling YOLO releases eligible pending requests and handles
future ordinary asks. Questions, sensitive reads, repeated-call checks and
outside-workspace access keep their own approval requirements.

`harness --yolo` starts with YOLO enabled. The TUI shows the active mode
in the composer. Mode changes are recorded in the session journal,
so resuming the session restores the last choice. Use `/yolo` or Ctrl+O to toggle
it. New sessions use the launch flag or configuration rather than another session's
choice.

Remembered approvals have run, session or workspace scope. Run grants end with
the run. Session grants restore from that session's journal. Workspace
grants use a private `.agent-harness/permission-grants.json` file. Tools cannot
edit that file or managed session storage.

The matcher depends on the approved operation:

| Operation | Remembered match |
| --- | --- |
| One unaliased workspace file | Later calls of that tool on the same resolved file, with different arguments |
| Aliased paths, multiple targets, or a dynamic refactoring request | The exact request digest |
| Bash | The same command or approved parsed command prefixes; every command in a list or pipeline must match |
| Outside-workspace paths | The requested directory, or a requested file's parent directory, across tools; checked by path components, never a bare root prefix |
| Repeated-call approval | Future repetition checks, subject to the acting role's deny rules |

External approvals are separate from a tool's ordinary approval. A saved file or
shell grant cannot approve a new outside directory. Remembered directory access
applies across tools within that directory. Each tool still passes its ordinary
permission checks, and deny rules still win. File and shell grants remain tied
to the approved tool. Expiring grants are not reused.

A third consecutive identical tool request triggers `doom_loop`. Allowing once
resets the streak. Remembering that approval suppresses later repetition asks;
a child role's denial still wins. The streak resets when the run ends.
YOLO alone does not disable this guard.

There are at most 4,096 retained grants. Workspace storage is limited to 1 MiB.
Grant descriptions that would expose a registered secret use an opaque request
digest instead. Large shell matchers also fall back to a digest.

## Paths and delegated work

File rules check the requested path and its resolved target. A deny on either
wins. Native tools bind approved arguments to those targets and reject a changed
symlink when resolving the path for use. Refactoring tools approve newly discovered
targets before editing them. Broad searches omit files needing another read
approval. These checks do not provide race-free filesystem confinement.

The caller needs the `spawn_subagent` tool and `task` permission to start a child
or resume completed child context.
Each child uses its own role policy and tools under the shared project policy.
A parent's role restrictions are not copied into the child. Nested eval tool calls
use the same coordinator checks. Skills provide instructions, not authority.
See [agents and tasks](../operations/generic-agent-and-tasks.md).

Approving bash permits host commands within the configured shell parser and
allowlist. Network tools can transmit data to their configured services. Optional
[OS confinement](../tools/shell.md) remains separate from operator approval.

## Eval permission

`permission.eval` accepts `allow`, `ask`, or `deny` and defaults to `ask`.
An approval permits arbitrary local code, imports, subprocesses, and direct
filesystem/network access from a persistent interpreter. It is not a sandbox.
The shipped read-only profiles do not include eval. Calls through `tool.<name>`
still use the caller's normal toolset and permission checks; an eval approval
does not grant any nested tool permission. See [eval](../tools/eval.md).
