# Eval

`eval` composes tools and processes their results in a persistent local kernel.
It replaces the former parallel-call tool. Each agent has its own kernels;
JavaScript and Python are enabled by default, with Ruby and Julia available by
configuration. Calls on one language run in submission order. Different
languages can run concurrently.

Existing agent tool lists should replace `batch` with `eval`. There is no alias
for the removed tool. Review `permission.eval` when migrating: eval can execute
local code, so its approval is separate from each nested tool's approval.

JavaScript uses the V8 runtime embedded in Harness. Node.js and Bun are not
required. Python uses `python3` or `python`; Ruby uses `ruby`; Julia uses `julia`. Missing
optional interpreters are omitted from the tool schema. Kernels start on first
use and close with the run. Startup and replay do not launch them.

## Calls

```json
{
  "language": "js",
  "summary": "Read both manifests to compare their dependencies",
  "code": "var results = await parallel(['a/package.json', 'b/package.json'].map(path => () => tool.read({path}))); display(results);"
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
| `workpool(agent, name, mode?)` | Uses an active host `workpool` tool when provided by an extension. Reports unavailable when none is registered, as the upstream bridge does. |

JavaScript takes an options object and asynchronous helpers are awaited. Python,
Ruby, and Julia use the same conceptual helpers with language-native keyword
arguments. Direct code and I/O are not sandboxed. `permission.eval` asks by
default; nested tool calls still require their own approvals. Read-only agent
profiles omit eval.

JavaScript can also expose a named kernel function to a child agent:

```js
tool(async function lookup(path) {
  return (await tool.read({path})).text;
});
await agent("Read memo.txt with lookup", {tools: ["lookup", "read"]});
```

The child can call the function while the parent cell waits. Its nested host
calls retain the child's tool scope. Definitions carry a kernel generation and
revision, so reset or redefinition invalidates an older descriptor. Names may
not collide with active host tools. `agent(..., {handle: true})` returns a
background handle; pass that handle to `output()` to inspect progress.

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
    "cell_timeout_seconds": 30,
    "foreground_window_seconds": 60,
    "run_budget_seconds": 300,
    "hard_limit_seconds": 1800,
    "max_detached_cells": 15,
    "parallel_pool_width": 4,
    "output_head_bytes": 20480,
    "output_max_columns": 768,
    "status_events": true,
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
