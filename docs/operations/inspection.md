# Inspect local configuration and models

```bash
harness doctor --json
harness models list --json
harness providers protocols
harness completions bash
```

Doctor validates configuration and reports provider selection and missing model
limits. It reads credential files without refreshing tokens. It does not contact
providers, start tools, create a session, or write probe files. A passing report
does not establish live authentication or operating-system sandbox enforcement.
Subscriptions without credentials fail the readiness check. Models with unknown
context or output limits produce a warning and retain unknown values.

`models` and `models list` show configured models and variants. Text output includes
each token limit and its provenance. `--json` includes the complete resolved model
metadata. Explicit configuration remains authoritative. Without configuration,
stored or environment credentials select models from the embedded catalog.
Known credential values are redacted from both output formats.

`providers protocols` lists implemented transports. Catalog membership alone does
not establish that a model accepts those transports or that an account can use it.
Shell completions support Bash, Zsh, Fish, PowerShell and Elvish.

## Configuration values and sources

```bash
harness config validate
harness config show --effective
harness config sources
harness config explain runtime.compaction.fallback_input_tokens
harness config explain provider.local.apiKey
harness config settings
```

These commands use the same configuration discovery and environment lookup as
prompt execution. `--config FILE` selects an explicit runtime file. The
`HARNESS_CONFIG_CONTENT` layer applies after files; `--session-dir` applies last
to the session directory. Separate TUI configuration is validated and appears in
the source list.

`show --effective` includes resolved defaults, profiles and redacted credentials.
`sources` lists layers in application order. `explain` shows a value and the last
input layer defining the requested path. Fields within a merged object can come
from different layers. Dotted paths accept runtime aliases; JSON Pointer paths
starting with `/` address exact keys, including names containing dots.
`settings` reports the typed settings registry without reading configuration.
Inspection does not create files or contact providers.

The executable loads `.env` without replacing values already in the process
environment. Global `--debug` writes diagnostic logs to stderr; `--debug-file FILE`
selects a private append-only file. Without `--debug`, that file uses info level.
Explicit debug routing also applies when the TUI starts. Logs redact credentials,
omit transport payload traces, and cap entries at 16 KiB and each log at 16 MiB.
Command results still go to stdout.

## Configuration schemas

```bash
harness schema > runtime-schema.json
harness schema --tui > terminal-schema.json
```

The runtime schema describes the loader's public input: provider and agent aliases,
permission modes and selector maps, tool switches, and `false` for disabling LSP
or formatters. It is generated from the configuration types and checked against
`configs/config.json`. Schema validation checks shape and field values; `config
validate` also checks model references and other relationships between fields.
The terminal command serves the unchanged `configs/tui.json`.

## Generate a catalog

```bash
harness models generated --output embedded-catalog.json
harness models probe --input models.dev.json --provider openai
harness models generate --stdin --output catalog.json < models.dev.json
```

`generated` copies the catalog embedded in the binary. `probe` and `generate`
accept a models.dev provider map or a generated Harness catalog. Choose one source:
`--input FILE`, `--stdin`, or `--url URL`. With no source option, they fetch
`https://models.dev/api.json`. These explicit commands can access the network;
doctor and model listing cannot.

`probe` writes to stdout unless `--output` is set. `generate` defaults to
`configs/provider-catalog.generated.json`. Output replacement is atomic. Invalid
input or an empty filtered catalog leaves an existing output file intact.

Inputs are limited to 16 MiB. Models need valid context and output limits. By
default, generation excludes deprecated models and models without advertised tool
support. `--include-deprecated` and `--include-non-tool` change those filters.
Repeat `--provider ID` to select providers. `generate` adds low, medium and high
presets for reasoning models without variants. `probe` adds them only with
`--emit-reasoning-variants`. Existing variants remain intact.

Generated metadata retains modalities and limit provenance. OpenAI and Anthropic
catalogs use their standard API URLs when the source omits one. Other providers
still require an endpoint compatible with an implemented transport.
