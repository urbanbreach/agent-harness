# Run a prompt

```bash
harness prompt --mock --text "Hello"
harness --config ./harness.json prompt "Inspect the project"
cat request.txt | harness prompt --stdin
harness prompt --prompt-file request.txt
```

Choose one input source. Prompt text must be nonempty and at most 1 MiB.
File input must name a regular file. Standard input drops trailing line endings
unless `--verbatim` is set. Relative paths use `--cwd`, or the launch directory.

`run` uses the same execution path and combines positional text, piped stdin and
`--prompt-file` with newlines. It reads stdin automatically when stdin is a pipe.
The combined input has the same 1 MiB limit, checked before trimming line endings.
`--file PATH` (`-f`) validates a file reference and appends `@PATH` to the prompt.

```bash
cat findings.txt | harness run "Check these findings" --prompt-file instructions.txt
harness run --continue "Continue the most recent resumable session"
harness run --session RUN_ID --fork-session "Try another approach"
```

`--continue` (`-c`) skips corrupt, replay-only and scenario sessions. `--session`
(`-s`) selects a session directly. Both use the prompt resume and fork behavior
described below. `--model` also accepts `-m`.

When the loaded config has no provider entries, prompt startup uses the embedded
catalog and the supplied environment or credential store to select a connected
provider. No config file is required; a config that only sets permissions or
other defaults behaves the same way. Discovery does not contact providers.
Defining any `provider` entry makes the catalog curated: configured providers
plus signed-in Codex, GitHub Copilot, and Claude subscription. Existing model
selections remain authoritative. Codex defaults to the bundled `gpt-6-astra`
entry and filters retired subscription models. With only `ANTHROPIC_API_KEY`,
the default model is a Claude Sonnet. Workspace `AGENTS.md` instructions load
with or without a config file.

The coordinator owns the session, provider requests, tool execution, and approval
decisions. A prompt completes after its agent turn reaches a terminal event.
Provider completion alone does not finish a turn that still has tools to run.

## Continue a conversation

```bash
harness --session-dir ./sessions prompt --resume RUN_ID --text "Continue"
harness prompt --resume /absolute/path/to/RUN_ID --text "Continue"
harness prompt --resume /absolute/path/to/RUN_ID --fork-session --text "Try another approach"
```

Resume restores conversation history and the recorded workspace. It does not
repeat old tools or print previous answers. The source must have a completed or
failed lifecycle and no active writer. Missing history, invalid metadata, changed
attachment blobs, and unavailable agent profiles cause an error.

`--fork-session` copies a completed history into a new sibling session before
continuing it. The source stays unchanged. `--session-id NAME` selects the new
directory name for either a fresh session or a fork. It accepts up to 128 ASCII
letters, digits, dots, underscores, and hyphens, excluding `.` and `..`.
Existing session directories are not replaced.

## Select the model and tools

| Option | Effect |
| --- | --- |
| `--profile NAME` | Select an agent profile for a new session |
| `--model PROVIDER/MODEL` | Override the selected profile's model |
| `--variant NAME` | Select a configured model variant |
| `--reasoning-effort LEVEL` | Override reasoning effort |
| `--thinking` | Request and display ephemeral reasoning summaries |
| `--max-turns N` | Limit provider iterations, including iterations after tool calls |
| `--tools read,write` | Restrict each profile to these existing tool IDs |
| `--disallowed-tools bash` | Remove the listed tools |
| `--no-subagents` | Disable child spawning; command output, wait, and cancellation tools remain available |
| `--disable-web-search` | Remove web search, code search, and web fetching |
| `--no-memory` | Remove the memory tool from profile toolsets |
| `--system-prompt-override TEXT` | Replace composed system instructions |
| `--rules TEXT` | Append instructions after the system prompt |

Tool restrictions apply to child profiles too. They cannot add a tool that the
profile did not already allow. Resume keeps the recorded profile; changing its
role is rejected. Explicit model options can change the resumed turn's model.
`--verbatim` also skips system-prompt overrides and extra rules.

## Set permissions

Headless execution denies requests that need interactive approval. Supply
permission rules in the configuration or select a prompt permission mode.

| Option | Default behavior |
| --- | --- |
| `--permission-mode default` | Ask before edits, shell commands, and network operations |
| `--permission-mode acceptEdits` | Allow edits; ask before shell and network operations |
| `--permission-mode dontAsk` | Deny edits, shell commands, and network operations |
| `--yolo` | Allow those operations and approve pending policy asks |
| `--allow edit,read` | Allow these permission kinds by default |
| `--deny bash` | Deny these permission kinds by default |

`--permission-mode yolo` selects YOLO mode too. The session remembers the mode
when resumed.
Explicit deny rules still apply. Allow and deny options change defaults and do
not erase selectors in the configuration. Deny options take precedence over allow
options for the same kind.

The legacy `--sandbox` option selects a permission preset. `readonly` denies edits,
shell commands, and network operations. `workspace` asks before edits and shell
commands and denies network operations. `danger` allows those defaults.
These presets do not provide operating-system confinement.

## Read and save output

Default output streams assistant text, then completes it from the committed
message. It withholds incomplete words and possible credentials until they can
be redacted. A long withheld tail waits for completion. `--thinking` also displays
reasoning summaries under a `[thinking]` label; they are never journaled.

`--format json` writes an array of live and committed events for the requested turn.
`--format streaming-json` writes one event per line as it arrives.
`--output-format` aliases `--format`. Each event has `delivery` and `event` fields.
Neither JSON mode retains a complete output array in memory.
Consumers should distinguish `delivery: "live"` fragments from
`delivery: "durable"` events. The committed assistant message is authoritative.
If the subscriber falls behind, output completes from durable history; lost live
reasoning is not reconstructed.

`--print-run-dir` writes the session directory to standard error.
`--out PATH` exports the journal after successful completion. Export reads one
record at a time, removes legacy provider deltas, reasoning and raw tool JSON,
and redacts values using the
run's credential registry. This also covers credentials learned after a historical
event was written. Export scans for remaining secrets before atomically replacing
the destination, which must be outside the session root. Rejected content leaves
the destination intact. Export never rewrites the source journal.
Legacy text deltas become settled text when their finished message has no parts.
Export renumbers retained records and updates rewind and compaction boundaries
to keep their meaning. Journals larger than 64 MiB are rejected.

Ctrl-C cancels active work through the coordinator and waits for cleanup.
Interrupted or failed prompts return status 1. Successful prompts return 0.
Argument parsing errors return 2. The configured `runtime.prompt.wait_timeout_ms`
is a deadline for the turn, not an inactivity timeout.

In-process callers can supply `CliIo`, environment overrides, a provider, a clock,
and cancellation through `CliDeps`. Prompt and OAuth execution also work when the
caller is inside a Tokio runtime. Borrowed input and output stay on the caller's
thread; output errors reach session cleanup.

## Generated scenarios

```bash
harness --session-dir ./fixtures run --scenario golden_path --deterministic
harness --session-dir ./fixtures run --scenario golden_path_interactive
```

The CLI scenarios request a native edit of `demo.txt` in a generated workspace
beside the session directory. `golden_path` grants that request; the interactive
variants read `allow`, `a` or `y` from stdin to grant it and otherwise deny it.
A denial returns a failed run without creating the demo file.

Deterministic runs use the configured seed, stable IDs and a fake clock. Repeating
one replaces only a verified generated fixture after acquiring its writer lock.
Another scenario writer or an ordinary session prevents replacement. Scenario
sessions are hidden from normal history and cannot be continued. In this mode,
`--print-run-dir` prints the generated directory on stdout.
