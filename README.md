<div align="center">
  <h1>Harness</h1>
  <p>
    <strong>A coding agent for your terminal.</strong><br />
    You decide what it may touch, and every session leaves a trail you can replay.
  </p>
  <p>
    <a href="https://github.com/urbanbreach/agent-harness/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/urbanbreach/agent-harness/ci.yml?branch=dev&style=flat-square&label=CI" /></a>
    <a href="LICENSE"><img alt="MIT license" src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" /></a>
    <img alt="Linux x86_64 and aarch64" src="https://img.shields.io/badge/platform-linux%20x86__64%20%7C%20aarch64-informational?style=flat-square&logo=linux&logoColor=white" />
    <img alt="Rust 2024 edition" src="https://img.shields.io/badge/rust-2024%20edition-orange?style=flat-square&logo=rust" />
  </p>
  <p>
    <a href="#install">Install</a> ·
    <a href="#quick-start">Quick start</a> ·
    <a href="#connect-a-provider">Providers</a> ·
    <a href="docs/README.md">Documentation</a> ·
    <a href="docs/README.fi.md">Suomeksi</a>
  </p>
</div>

<p align="center">
  <picture>
    <source media="(prefers-reduced-motion: reduce)" srcset="docs/assets/harness-tui.png" />
    <img src="docs/assets/harness-demo.gif" alt="Harness in a terminal. The user types: cargo test is failing, find the root cause and fix it. The agent writes a todo list, runs the tests, reads the crate, explains that parse_duration drops trailing digits, patches src/lib.rs, reruns the tests and reports that all three pass." width="960" />
  </picture>
</p>

<p align="center">
  <sub>An unscripted session with GPT 6.1 Sol, recorded from the real TUI. The one-minute run is sped up and long waits are trimmed.<br />
  <a href="docs/assets/harness-tui.png">Still image</a> · <a href="docs/assets/README.md">How it was recorded</a></sub>
</p>

Harness is a coding agent you run inside a project. Give it a task and it reads
the code, runs commands, edits files and checks its own work. Every tool call
shows up in the transcript as it happens, and your permission rules decide what
runs without asking.

It is a single Rust binary with a Ratatui interface. One coordinator owns every
tool call, permission check and subagent, and records what happened in an
append-only event log. That log is why sessions can be resumed, forked, rewound
and replayed offline. Replay never reruns a tool or contacts a provider.

## What you get

| Feature | Details |
| --- | --- |
| Models you already use | Sign in with ChatGPT, GitHub Copilot or a Claude subscription, or export an API key for OpenAI, Anthropic, Google, OpenRouter and other models.dev providers. Any OpenAI-compatible endpoint works too. |
| Tools that do real work | Read, write and edit files (patches, string replacement or hashline anchors, picked per model), shell, grep, glob, web fetch and search, LSP, ast-grep, GitHub, MCP servers, persistent JavaScript and Python eval, and a background `monitor` that wakes the agent when a command or file changes. |
| Subagents | The agent can hand work to `task`, `scout`, `reviewer`, `security-reviewer` and `clerk`, or to agents you define in Markdown. Children run under the same project policy. |
| Permissions | Allow, ask and deny rules per tool, path and shell command pattern. YOLO mode when you want it, opt-in Landlock confinement for shell commands. |
| Replayable sessions | Append-only journals with resume, fork, rewind, a session tree, offline replay and redacted support bundles. Sessions from Codex and Claude Code can be imported too. |
| Your conventions | Reads `AGENTS.md` (or `CLAUDE.md`), loads skills, runs lifecycle hooks and turns Markdown files into slash commands. |
| A terminal UI worth watching | Streaming transcript with diffs, a todo pane, live tokens per second, session cost, context use, managed Git worktrees and a command palette. |

## Install

Harness runs on Linux, x86_64 and aarch64. macOS and Windows are not supported.

```bash
curl -fsSL https://github.com/urbanbreach/agent-harness/releases/latest/download/install.sh | sh
```

The script downloads the static binary for your CPU, checks it against the
release's `SHA256SUMS` and installs it to `~/.local/bin`, with third-party
notices in the sibling `harness-licenses` directory. Set
`HARNESS_VERSION=v0.1.0` to pin a release or `HARNESS_INSTALL_DIR` to install
elsewhere. To uninstall, delete both.

> [!NOTE]
> No release has been tagged yet. Until the first one is published, build from
> source.

### Build from source

You need Git and the stable Rust toolchain pinned in
[`rust-toolchain.toml`](rust-toolchain.toml). Nothing else is required to build.

```bash
git clone https://github.com/urbanbreach/agent-harness.git
cd agent-harness
cargo install --path crates/harness --locked
```

Use release builds for real sessions; debug builds burn noticeably more CPU
while streaming. [Build settings](docs/testing/build-performance.md) covers the
optional Wild linker.

### Optional tools

| For | Install |
| --- | --- |
| JavaScript eval | [Node.js 24 or newer](https://nodejs.org/en/download) on `PATH`. Harness bundles its eval scripts, so no npm install is needed. |
| Python eval | `python3` or `python` on `PATH` |
| Structural search and rewrite | `ast-grep` on `PATH` |
| Claude Pro/Max subscription | An installed, signed-in Claude Code |

## Quick start

```bash
cd /path/to/your/project
harness
```

No config file is needed. On a fresh install the login picker opens by itself.
Pick a provider, sign in, type a task and press Enter. Esc closes the picker and
`/login` brings it back.

Want to look around first? `harness tui --mock` opens the same interface with an
offline provider that answers every prompt with `Hello world`.

## Connect a provider

`/login` in the TUI lists every provider Harness knows about, with OpenAI,
ChatGPT Plus/Pro, GitHub Copilot, Anthropic, Claude Pro/Max and Google near the
top. The same flows work from the shell:

```bash
harness auth login codex     # ChatGPT Plus/Pro
harness auth list            # what is connected
harness doctor               # offline readiness check
```

An exported API key works with no login step and no config:

```bash
export ANTHROPIC_API_KEY="..."
harness
```

Harness finds stored credentials and provider API key variables through its
embedded [models.dev](https://models.dev) catalog. `harness doctor` never
contacts a provider; the first real turn is what proves your account and
endpoint work. Keep keys out of config files. See
[provider support](docs/configuration/provider-support.md) for every transport.

## Working with Harness

### In the terminal UI

| Key | Does |
| --- | --- |
| Enter | Send the prompt, or steer the running turn |
| Alt+i | Queue a follow-up for after the current turn |
| Ctrl+Enter | Cancel the running turn and send the draft instead |
| `/` | Slash commands: `/models`, `/sessions`, `/compact`, `/rewind`, `/worktree`, `/init` and more |
| `@` | Mention a file |
| Ctrl+p | Command palette |
| Ctrl+x | Shortcut menu (themes, models, status, export) |
| Shift+Tab | Cycle the model's reasoning variant |
| Ctrl+o | Toggle the approval mode |
| Ctrl+t | Show or hide the todo pane |
| Ctrl+c | Cancel the running turn |

Keys are configurable in `tui.jsonc`; the [design guide](DESIGN.md) lists them all.

### From scripts

`harness run` takes the prompt from arguments, stdin or a file, waits for the
turn to finish and prints only the final answer.

```bash
harness run "Summarize this workspace"
git diff | harness run "Review this change" --format json
harness run -c "Now add tests for it"        # continue the last session
```

Flags cover the model (`-m`), reasoning effort, tool allow and deny lists,
permission presets and attachments. See the [prompt CLI](docs/operations/prompt.md).

### Other commands

| Command | Does |
| --- | --- |
| `harness sessions list` | List saved sessions for this project |
| `harness sessions inspect <id>` | Show what happened in a session |
| `harness sessions tree` | Show forks and branches |
| `harness replay --session <id>` | Replay a session without providers or tools |
| `harness export <id>` | Write the visible conversation as redacted Markdown |
| `harness dashboard status` | Summarize sessions and local configuration |
| `harness config explain model` | Show where a setting comes from |
| `harness models` | List models, variants and token limits |
| `harness completions <shell>` | Print shell completions |

## Agents, skills and commands

The main agent delegates with `spawn_subagent`. Out of the box, every subagent
runs on your main model; Harness doesn't pick a different model for any of them.
To give `task`, `scout`, `clerk` or the reviewers their own models, set them
yourself in `harness.jsonc` or ask the agent to do it for you. See
[choosing subagent models](docs/configuration/config.md#choosing-subagent-models).
Your own agents are Markdown files with YAML frontmatter in
`.harness/agents/` (project) or `~/.harness/agents/` (personal). Project files
win over personal ones, which win over the built-ins. Details are in
[agents and tasks](docs/operations/generic-agent-and-tasks.md).

Skills load from `.harness/skills` and `.agents/skills` in the project, and from
the same two directories under your home. Markdown files in `.harness/commands/` or
`~/.harness/commands/` become slash commands, with `$ARGUMENTS` and `$1` to `$9`
filled in. They also work headless: `harness run "/name args"`. The bundled
`/init` writes or updates a short root `AGENTS.md` and keeps what you wrote. See
[extensions](docs/operations/extension-strategy.md) for skills, MCP servers and
[hooks](docs/operations/hooks.md).

## Permissions

Out of the box, ordinary tools run without asking. Harness asks before touching
paths outside the project, before repeating an identical call, and before
reading `.env` files. A personal or project config can tighten that:

```jsonc
"permission": {
  "*": "allow",
  "edit": "ask",
  "bash": {
    "*": "deny",
    "git status*": "allow",
    "cargo nextest run*": "ask"
  },
  "webfetch": "deny"
}
```

The last matching rule wins, so put broad rules first and exceptions after them.
Keep the leading `"*": "allow"`. Once a `permission` block exists, any tool
without a rule of its own (`todowrite`, `skill`, the session tools) asks every
time.

`--yolo` (or `/yolo`) approves ordinary requests for a session, and
`--sandbox readonly` is a quick read-only preset for `harness run`.

Permissions decide which tool calls run. They are not an operating-system
sandbox: an approved shell command can do anything your user can. For real
filesystem confinement of shell commands, set `HARNESS_OS_SANDBOX_POLICY` to
`workspace_write`, `read_only` or `strict` (Linux Landlock). Read the
[permission guide](docs/permissions/permissions.md) before sharing a policy with a
team or with subagents.

## Configuration

Config is optional. After your first successful sign-in Harness writes
`~/.harness/harness.jsonc` with that provider's default model, unless a user
config already exists or `HARNESS_CONFIG` or `HARNESS_CONFIG_CONTENT` is set.
Put personal defaults there and project policy in `harness.jsonc` or
`.harness/harness.jsonc` at the project root; project layers override personal
ones. Keyboard settings live in `tui.jsonc`.

```bash
harness config sources             # every file it looked for, in load order
harness config show --effective    # the merged, redacted result
harness schema                     # JSON schema for editor completion
```

[`configs/harness.example.jsonc`](configs/harness.example.jsonc) is a short,
annotated starting point. Defining any `provider` entry switches the model catalog
from automatic discovery to curated: only configured providers, plus signed-in
Codex, GitHub Copilot and Claude subscription accounts, remain. The
[config reference](docs/configuration/config.md) lists every key.

## Where your data lives

Everything Harness writes for you goes under `~/.harness/`, or under
`$HARNESS_HOME` used as-is when it is set and nonempty:

| Path | Holds |
| --- | --- |
| `harness.jsonc`, `tui.jsonc` | Personal runtime and keyboard config (`.json` also works) |
| `credentials/`, `anthropic-subscription-bindings/` | Sign-ins and subscription bindings |
| `models-cache.json`, `model.json` | Cached model metadata and the last model picked in the TUI |
| `prompts/`, `agents/`, `commands/`, `skills/` | Your prompt overrides, agents, slash commands and skills |
| `sessions/<key>/` | Session journals and artifacts |
| `projects/<key>/` | Workspace memory, code index, edit attribution and plans |
| `worktrees/<key>/` | Managed Git worktrees |

`<key>` is `v2-` plus the BLAKE3 digest of the canonical project path, so every
project gets its own fixed-length directory. Inside a project, `.harness/` holds
authored agents, commands, skills, prompt overrides, project config and
remembered approvals. Older `--path--` directories are left alone; see
[legacy storage](docs/architecture/sessions-and-replay.md#legacy-storage) to move
them. Sessions from early builds in `<project>/.agent-harness/sessions` open with
`harness sessions list --session-dir .agent-harness/sessions`.

Before you share a session, run `harness sessions export <id> --output bundle.json`.
The bundle keeps replay metadata and drops provider deltas and tool payloads. If
its secret scan finds anything, nothing is written. [Privacy and local
data](docs/permissions/privacy-and-local-data.md) explains what leaves your
machine and when.

## Documentation

| Use Harness | Work on Harness |
| --- | --- |
| [Configuration](docs/configuration/config.md) | [Architecture](docs/architecture/architecture.md) |
| [Providers](docs/configuration/provider-support.md) | [Terminal design](DESIGN.md) |
| [Native tools](docs/tools/native-tool-catalog.md) | [Testing](docs/testing/testing.md) |
| [Permissions](docs/permissions/permissions.md) | [Releasing](docs/operations/releasing.md) |
| [Sessions and replay](docs/architecture/sessions-and-replay.md) | [Changelog](CHANGELOG.md) |
| [Troubleshooting](docs/operations/troubleshooting.md) | [All documentation](docs/README.md) |

## Contributing

The workspace is seven Rust 2024 crates on the stable toolchain. Start with the
[architecture guide](docs/architecture/architecture.md) to see who owns what, and
the [terminal design guide](DESIGN.md) before touching the UI.

```bash
cargo fmt --all -- --check
scripts/test-lanes.sh fast
scripts/test-lanes.sh quality-gates
```

Tests run under nextest. The [testing guide](docs/testing/testing.md) explains the
integration, simulation, performance, PTY and live-provider lanes.

## License

[MIT](LICENSE)
