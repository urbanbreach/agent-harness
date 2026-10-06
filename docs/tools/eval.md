# Eval

`eval` composes tools and processes their results in a persistent local kernel.
It replaces the former parallel-call tool. Each agent has its own kernels;
JavaScript and Python are enabled by default, with Ruby and Julia available by
configuration. Calls on one language run in submission order. Different
languages can run concurrently.

Existing agent tool lists should replace `batch` with `eval`. There is no alias
for the removed tool. Review `permission.eval` when migrating: eval can execute
local code, so its approval is separate from each nested tool's approval.

JavaScript requires Node.js 24 or newer on `PATH`. Install the current Node.js
LTS release and verify it with `node --version`. Harness ships the worker scripts
and an MIT-licensed Acorn parser, so eval needs no npm install. A missing or older
Node installation produces an actionable error when JavaScript is first used.
Python uses `python3` or `python`; Ruby uses `ruby`; Julia uses `julia`. Missing
optional interpreters are omitted from the tool schema. Kernels start on first
use and close with the run. Startup and replay do not launch them.

The JavaScript process persists between cells. A supervisor keeps the control
channel responsive during blocking code and captures native stdout/stderr writes
separately from protocol messages. Both processes close with the kernel. The
worker ignores `NODE_OPTIONS`; project imports use Node's module resolution and
TypeScript support. JSX and TypeScript syntax that requires a separate compiler
must be compiled by the project before import.

## Calls

```json
{
  "language": "js",
  "summary": "Read both manifests to compare their dependencies",
  "code": "var paths = ['a/package.json', 'b/package.json']; var results = await parallel(paths.map(path => () => tool.read({path}))); display(results.map((r, i) => ({path: paths[i], text: r.text, error: r.hasError})));"
}
```

`language`, `code`, and a one-line `summary` are required for a run. `action`
defaults to `run`. State survives between cells. JavaScript supports imports,
top-level `await` and `return`; a final expression is displayed automatically.
Use `reset: true` to clear only the selected language. Reset refuses a running
or queued kernel.

`parallel(thunks)` preserves input order and uses four workers by default.
`pipeline(items, ...stages)` applies stages in order, with a barrier between
stages. JavaScript also supports ordinary `Promise.all` and `Promise.allSettled`.
Use the latter when one rejected tool call should not reject the whole result.

### Tool visibility

`eval.route_tools` optionally lists tool IDs or discovery-catalog IDs to remove
from the model's direct tool definitions while eval is available to that agent.
For example, `"route_tools": ["bash", "grep", "mcp.example.tool.call"]` routes
those tools and the MCP catalog's discovered tools through `tool.<name>(args)`.
The default empty list keeps every authorized tool directly visible.

Routing changes visibility only. Tools remain in the registry with the same
profile authorization, permission checks, and coordinator lifecycle. The eval
catalog and `tool_schema()` still include them, including newly discovered MCP
tools. If a profile omits eval or denies it, routed tools stay directly visible.
`eval` and `question` cannot be routed. Do not remove routed tools from the
profile's tool list: that list controls authorization for nested calls too.

## Helpers

| Helper | Behavior |
| --- | --- |
| `tool.<name>(args)` | Calls the active native or MCP tool through coordinator permissions, scheduling, cancellation, and journaling. Returns `text`, `details`, `images`, and `hasError`. Recursive eval is rejected. |
| `tool_schema(name?)` | Returns a tool schema, or the available tool names when omitted. |
| `display(value)` | Displays text, JSON, markdown, image bytes, a data URL, or a tool result with images. Images reach the model only when displayed. |
| `print(...)`, `log(...)`, `phase(...)` | Emits output or progress using the engine's language-specific helpers. |
| `read(path, offset?, limit?)`, `write(path, content)` | Direct local text I/O, including `local://` files under the agent's session artifacts. These files survive kernel resets and run shutdown. |
| `env(key?, value?)` | Reads or updates the kernel environment. Kernels inherit Harness's filtered process environment and a snapshot of the session ID, journal path, working directory, provider, model, and configured reasoning level. |
| `completion(prompt, options?)` | One tool-free model request through Harness's provider, cancellation, budget, and usage accounting. `schema` requests parsed JSON. `model` accepts `default`, `smol`, or `slow`; the latter two use configured agent model targets with those names. |
| `agent(prompt, options?)` | Uses `spawn_subagent`; supports agent/model selection, labels, JSON results, background handles, and a subset of the child's permitted `tools`. |
| `output(ids, options?)` | Retrieves owned command/subagent output, with raw/tail format and line slicing. Accepts returned IDs or `agent://` handles. |
| `wait(handles, options?)` | Waits on owned native tasks: `all` stops on failure, `any` waits for a success, `settled` collects terminal outcomes. Returns `done`, `mode`, and ordered `results`; a timeout returns `done: false` without cancelling work. |
| `workpool(agent, name, options?)` | Creates a bounded pool through native subagent admission. Exposes `pool_id`, `push`, `close`, `inspect`, and `cancel`. An installed host `workpool` tool takes precedence. |

JavaScript takes an options object and asynchronous helpers are awaited. Python,
Ruby, and Julia use the same conceptual helpers with language-native keyword
arguments. Direct code and I/O are not sandboxed. `permission.eval` asks by
default; nested tool calls still require their own approvals. Read-only agent
profiles omit eval.

### Kernel-defined tools

JavaScript and Python can expose named kernel functions to child agents:

```js
tool(async function lookup(path) {
  return (await tool.read({path})).text;
});
await agent("Read memo.txt with lookup", {tools: ["lookup", "read"]});
```

```python
@tool
def lookup(path: str):
    """Read a project file."""
    return tool.read(path=path)["text"]

agent("Read memo.txt with lookup", tools=["lookup", "read"])
```

Python infers object schemas from named parameters, defaults, and supported type
annotations (`str`, `int`, `float`, `bool`, lists, string-keyed dictionaries,
unions, and literals). Both languages accept an explicit description and schema.
Python accepts synchronous or async functions. `tool.defined()` lists definitions;
`tool.undefine(name)` removes one. A name defined in both kernels must be resolved
before publication. Ruby and Julia do not define kernel tools.

The child can call the function while the parent cell waits. Its nested host
calls retain the child's tool scope. Definitions carry a kernel generation and
revision, so reset or redefinition invalidates an older descriptor. Names may
not collide with active host tools. `agent(..., {handle: true})` returns a
background handle; pass that handle to `output()` to inspect progress. In
JavaScript and Python it also has a `control` attribute with `status()`,
`output()`, `send(text, delivery?)`, `cancel()`, and `wait(options?)`. These route
through the ordinary native tools and their permissions; `send` also requires
subagent messaging to be enabled. Handle records use `run_epoch: 0` for the
initial execution in the current run. The coordinator rejects them after a
native message restarts that child; an old record cannot control the successor
execution. Raw native IDs retain native restart behavior.

`wait` accepts 1–20 IDs or handle records, with a timeout of 0–3,600 wall-clock
seconds (default 60). Host waits pause the cell's own execution budget. `all`
raises when any task fails or is cancelled; `any` raises if every task settles
without success. `settled` returns failures as outcomes. Ruby and Julia use the
global `wait` and `output` helpers with the returned records.

### Worker pools

```js
var pool = await workpool(
  {subagent_type: "general-purpose", prompt: "Inspect the assigned file"},
  "file-review",
  {width: 2, mode: "fresh", tools: ["read"]}
);
var receipt = await pool.push([
  {key: "api", input: "Read src/api.rs"},
  {key: "db", input: "Read src/db.rs"}
]);
await pool.close();
display((await pool.inspect()).details);
```

Each item starts a fresh native child. Width defaults to the smaller of four
and the configured subagent limit, and accepts 1–256; the coordinator's global
limit still applies. Native `isolation: "worktree"` can be set in the agent
object. `keep_alive` is not supported. There are at most 64 pools per agent run
and 128 items per pool; keys must be unique. Push validates a whole batch before
admission and reports rejected admissions as failed items.

Closing seals input. Once every item settles, a pool created during a model
turn delivers one aggregate notice with redacted output previews, capped at
1,024 characters per item. Its queue slot is reserved at creation. Individual
worker notices do not wake the parent. Use `output()` for full native results.
`cancel()` uses native cancellation for running and queued workers. Pools belong
to their creating agent and run; another agent cannot inspect or control them.

JavaScript `workpool.open(pool_id)` and Python `workpool.open(pool_id)` restore
an adapter after a kernel reset. Pool state lasts for the coordinator run;
pool IDs do not survive process restart. Native child histories remain durable.
Finished pool children cannot be restarted; push a new item for another execution.

### Scripts and managed packages

Run each command in its own JavaScript or Python cell:

```text
%load "scripts/analyze data.py"
%pip install pandas==2.3.3
%npm install lodash@4.17.21
```

`%load` executes a local UTF-8 regular file of at most 8 MiB in the current
kernel, preserving variables and using the script's location for relative
imports. JavaScript accepts JavaScript scripts and Python accepts Python scripts.
It does not fetch remote scripts.

`%pip install` and `%npm install` create session-local environment revisions.
They accept package specifications, including local paths, without installer
flags or shell expansion. The project files and lockfiles stay untouched.
A successful revision becomes the import fallback; project resolution retains
precedence. Failure or cancellation leaves the previous revision active. Reset
reloads the active revision, but already imported modules keep their interpreter
cache until reset. Earlier revisions remain in session artifacts; installing
large environments copies the previous revision and can use substantial disk.

The Python interpreter must have pip, and npm must be on `PATH`. Installer
configuration and caches are separate from user configuration. npm lifecycle
scripts are disabled; pip builds can execute package code under eval's existing
local-code authority. Installation is unavailable inside child-called kernel
functions. Project-environment mutation and Bun-specific commands are not part
of this interface.

### Isolated JavaScript

Enable `eval.sandbox.enabled`, then use `"language": "js", "isolate": true`.
Each cell gets a fresh QuickJS context with no ambient process, filesystem,
network, timers, or module imports. Only `tool`, `tools`, `tool_schema`, output
helpers, and ordinary JavaScript built-ins are available. Explicit host tools
still use coordinator permissions. This does not change normal Node/Python cells.

The default heap limit is 64 MiB and execution budget is 300 seconds, further
limited by the cell's own timeout. Cancellation interrupts CPU-bound code;
queued cells and host waits retain the ordinary wall-clock limit. The Node
kernel's variables survive isolated cells, including resource-limit failures.
`reset` cannot be combined with isolation. Host calls in isolated cells currently
execute serially, including calls inside `Promise.all`.

## Background execution

Interactive calls return after 30 seconds of the cell's own work, or a 60-second
foreground window including tool waits. The cell continues under coordinator
ownership and sends one completion notification. The receipt contains its
`cell_id`; do not repeat the original code.

```json
{"action":"list"}
{"action":"peek","cell_id":"the-returned-cell-id"}
{"action":"stop","cell_id":"the-returned-cell-id"}
```

`peek` returns the current or retained result. `stop` handles queued and running
cells. A JavaScript cell that cannot acknowledge cancellation has its worker
replaced; that clears its globals. Python interruption normally keeps globals.

`timeout` is the cell's own execution budget in seconds (default 300), paused
during host tool calls. It does not move the detach point. A wall-clock limit
of 1,800 seconds also includes queue/tool waits; a larger `timeout` raises it.
`on_timeout: "error"` waits for settlement and is the default for noninteractive
CLI calls. Explicit `"detach"` enables the interactive behavior.

There are at most 15 detached cells per agent and 32 retained cell results, with
a 32 MiB result budget and 256 MiB retained-image budget. A full agent prompt
queue prevents detachment instead of losing a completion notification. Queued
user input can detach an interactive cell early so the agent can respond.

## Output and configuration

The transcript shows a summary, language, state, tool count, and elapsed kernel
time. Input and output have separate arrows. `Ctrl+e` expands a selected cell;
`Enter` opens its output viewer. Live output is transient; replay uses redacted
settled events and never re-executes code.

Output uses head/tail truncation and redacted overflow artifacts, subject to
Harness's 8 MiB retained-text limit. Images use the normal attachment boundary:
at most 2,000 pixels per side and 4.5 MiB of base64 per image, further constrained
by the provider's aggregate attachment limits. Oversized images are resized;
unsupported delivery formats are converted.

```json
{
  "permission": {"eval": "ask"},
  "eval": {
    "languages": ["js", "py", "rb", "jl"],
    "route_tools": [],
    "cell_timeout_seconds": 30,
    "foreground_window_seconds": 60,
    "run_budget_seconds": 300,
    "hard_limit_seconds": 1800,
    "max_detached_cells": 15,
    "parallel_pool_width": 4,
    "output_head_bytes": 20480,
    "output_max_columns": 768,
    "status_events": true,
    "sandbox": {
      "enabled": false,
      "memory_limit_mb": 64,
      "timeout_seconds": 300
    },
    "memory": {
      "gc_watermark_mb": 256,
      "notice_mb": 1024,
      "ceiling_mb": 2048,
      "retained_results_mb": 32,
      "retained_images_mb": 256
    }
  }
}
```

Zero disables a memory threshold. Nonzero thresholds must be ordered
watermark ≤ notice ≤ ceiling. Settings are loaded through Harness's strict
configuration loader; upstream configuration files and environment overrides
are not read.

Native extensions can implement `Tool::kernel_prelude()` to install JavaScript
and Python helpers while their tool is active. Helpers should call the ordinary
`tool` bridge. Their documentation appears in `tool_schema(name)`; names that
shadow built-in eval helpers or start with `__` are rejected. Results retain
the detected interpreter name, version, and path for inspection.

## Verification

```sh
scripts/test-lanes.sh eval
HARNESS_EVAL_LANGUAGES=js,py,rb,jl scripts/test-lanes.sh eval
node scripts/qa/capture-eval.mjs .omo/evidence/eval
node scripts/qa/benchmark-eval.mjs .omo/evidence/eval/performance
```

The runtime lane requires installed interpreters and fails when they are missing.
The xterm runner records real coordinator/kernel events with a scripted provider,
then exercises the native TUI in a PTY at 40, 80, and 120 columns. Performance
evidence records the release native crate. An optional standalone reference runner
can be passed as a second argument to compare identical workloads on Node and Bun.
Engine measurements exclude coordinator and provider latency; the integration lane
exercises those boundaries separately.

The implementation lives in `crates/harness-eval`. Comparison runners are supplied
separately and are not linked into Harness.
The [verification record](../performance/eval-2026-10-05.md) includes native test
results, xterm.js captures, benchmark samples, and the measured comparison scope.

The [code-mode follow-up record](../performance/eval-codemode-2026-10-05.md)
separates the new capability checks from live-provider workflow measurements.
Those measurements found no consistent latency or token savings; tool routing
remains opt-in.
