# File formatting

Approved writes, exact edits, hashline edits and patches format their proposed
contents before recording the final diff and digest. Undo therefore covers the
formatted result. Registry construction, replay and inspection never start a
formatter.

```json
{
  "formatter": {
    "rustfmt": {"disabled": true},
    "project": {
      "command": ["my-formatter", "$FILE"],
      "extensions": [".custom"],
      "environment": {"MODE": "strict"}
    }
  }
}
```

`formatter: false` disables formatting. `true` or an omitted key enables built-in
discovery. An object can set `enabled`, `experimentalOxfmt` and named overrides.
Overrides can disable a formatter, replace its extensions or command, and add
environment values. An explicit command bypasses discovery. Commands are argument
arrays, not shell expressions. `$FILE` is replaced in each argument; when absent,
the target path is appended. The working directory is the run's workspace.

Each formatter receives a copy in a private temporary directory beside the target.
The copy keeps the target's filename and extension. `$FILE` names this copy, not
the final destination. Temporary output and backup files are removed afterward.
A failed formatter produces a redacted warning and retains the previous proposed
contents. Cancellation stops the process group and prevents the edit from being
committed. A target changed by another editor while formatting is refused.

The formatter configuration is captured for each coordinator, including the CLI
scenario runner. Configured environment values join that run's secret registry.
These commands run as native processes; edit permissions are not an OS sandbox.

## Built-in discovery

| Names | Discovery |
| --- | --- |
| `gofmt`, `mix`, `zig`, `ktlint`, `rubocop`, `standardrb`, `htmlbeautifier`, `dart`, `terraform`, `latexindent`, `gleam`, `shfmt`, `nixfmt`, `rustfmt`, `ormolu`, `cljfmt`, `dfmt` | Executable on `PATH` |
| `prettier`, `oxfmt` | Project dependency in `package.json`, then a local `node_modules/.bin` executable or `PATH` |
| `biome` | `biome.json` or `biome.jsonc`, then a local executable or `PATH` |
| `clang-format`, `ocamlformat` | Project configuration file and an executable on `PATH` |
| `ruff` | Project Ruff configuration or dependency and an executable on `PATH` |
| `uv` | Available `uv format`, used when Ruff was not selected |
| `pint` | Laravel Pint in `composer.json` and `vendor/bin/pint` |
| `air` | Executable whose help identifies the R formatter |

Discovery searches from the target's parent up to the workspace root. Discovery does not
install missing tools. Oxfmt requires `experimentalOxfmt: true`. Disabling either
Ruff or uv disables both Python alternatives, matching the existing configuration
contract. Rustfmt receives `skip_children=true` to keep module children unchanged.
Web formatters receive `BUN_BE_BUN=1` unless overridden.

Matching built-ins run in the order listed in the configuration reference; custom
names follow in sorted order. The legacy `uvformat` name maps to `uv`, and
`languages: {"EXT": {"command": [...]}}` maps to a named extension override.

Input and formatted files are limited to 8 MiB. Commands have a 30-second deadline
and bounded output capture. Discovery help checks have a five-second deadline;
configuration files used for discovery are limited to 128 KiB. Native checks cover
the installed rustfmt, unchanged module children, cancellation and process cleanup.
