# Troubleshooting

Start with `harness doctor`, the offline readiness check. It works without a
config file and never contacts providers. A passing report confirms local
readiness, not account access or a reachable endpoint. After connecting, run
`harness run "Hello"` to check a live turn.

Config files are optional. With no runtime files, `harness config validate`
exits successfully and prints:

```text
config valid: no configuration files; using built-in defaults
```

`harness config sources` includes `searched`, every candidate runtime path in
merge order, even when a file is absent. Personal files are under a nonempty
`HARNESS_HOME`, used as-is, or `~/.harness` otherwise. Project policy loads after
personal defaults. First sign-in can create `<home>/harness.jsonc` without
overwriting an existing user config, unless `HARNESS_CONFIG` or
`HARNESS_CONFIG_CONTENT` is set. With no runtime layers, the `note` is:

> No configuration files found. Harness uses built-in defaults and providers connected through /login, `harness auth login`, or provider API key environment variables.

`harness config show --effective` and `harness config explain model` work with
built-in defaults and `primary_path: null`. An explicit `--config` path must
still exist.

When no provider is connected, doctor reports:

> No provider connected. Run `harness auth login <provider>`, use /login in the TUI, or set a provider API key such as OPENAI_API_KEY or ANTHROPIC_API_KEY.

A provider-less config keeps automatic credential discovery from the embedded
models.dev catalog. Defining any `provider` entry limits the catalog to
configured providers plus signed-in Codex, GitHub Copilot, and Claude
subscription. Check this first if an exported key seems ignored.

| Problem | What to check |
| --- | --- |
| A setting seems ignored | Run `harness config sources`, then `harness config explain <path>`. A later layer may override the value. |
| A custom slash command is missing | Check `.harness/commands/<name>.md` or `<home>/commands/<name>.md`, the file-stem pattern, and YAML frontmatter. Built-in names and aliases are reserved; discovery warnings appear on CLI stderr or in a TUI toast. |
| A custom agent or skill is shadowed | Check nearer project definitions and Harness roots first. User agent definitions override built-ins; `.agents/skills` ranks below Harness skill roots. |
| A child uses the wrong model or variant | Check the per-call selection, role/persona defaults, `subagents.models`, definition and parent. Unset `@smol`/`@slow` roles inherit the parent; unknown variants are ignored with a runtime warning. |
| Credentials are missing | Use `/login` in the TUI, `harness auth login <provider>`, or export a catalog provider API key such as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`. |
| The provider rejects credentials | Check the selected account and endpoint. Keep the sanitized error and a support export. |
| The provider rate-limits requests | Wait or reduce request volume. Check the error category before changing credentials. |
| A local proxy fails | Compare the configured `baseURL` with the proxy's listening address. |
| An MCP server is unavailable | Confirm the entry is enabled and its executable exists. Doctor checks configuration; it does not connect to MCP. |
| An LSP operation is unavailable | Read the tool's structured error and check the server executable. Use compiler diagnostics when the server cannot report them. |
| A tool is denied | Check the tool's permission kind and matching rule order. The last matching rule wins. Inspect the workspace diff if the request involved an edit. |
| A provider stream fails | Keep the error category and support bundle. Do not share raw provider payloads. |
| A session cannot resume | Run `harness sessions inspect <run-id-or-path> --json` and read `resume_disabled_reason`. |
| Replay fails | Check that `events.jsonl` exists and its sequence is valid. Inspect the reported error before attempting recovery. |
| The TUI renders incorrectly | Retry with `harness tui --mock`. Record the terminal emulator, dimensions, and a screenshot. `Ctrl+p` opens the command palette. |

## Share a support bundle

```bash
harness sessions export --session-dir <session-dir> --output support-bundle.json <run-id>
```

The exporter redacts known secret formats, then scans for credentials and hidden
prompt or config values. A failed scan prevents it from writing the bundle.
Share the bundle rather than raw events. See [privacy and local data](../permissions/privacy-and-local-data.md).
