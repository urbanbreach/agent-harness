Eval helper reference. JavaScript helpers take one trailing options object and return promises to await. Python, Ruby and Julia accept the same options as keyword arguments.

Subagents
- agent(prompt, options?) runs a native subagent and returns its result. Options: agent (role), model, label, schema (parsed JSON result), tools (a subset of the child's permitted tools), handle, plus isolated, apply and merge when the spawn tool supports worktree isolation.
- With handle: true it returns a record at once. In JavaScript and Python, record.control offers status(), output(), send(text, delivery?), cancel() and wait(options?). send requires subagent messaging. A record stops controlling the child once a message restarts it.
- output(ids, options?) reads owned command or subagent output by ID, handle record or agent:// URL. Options: format (raw or tail), offset, limit.
- wait(handles, options?) waits on 1 to 20 IDs or handle records. mode all (default) raises when any task fails; any returns the first success and raises if none succeed; settled returns every outcome. timeout is 0 to 3600 seconds, default 60. The result is {done, mode, results}; a timeout returns done: false and leaves the work running. Host waits pause the cell's own budget.
- workpool(agent, name, options?) runs pushed items through fresh native children. agent is a spawn object such as {subagent_type, prompt}. Options: width (1 to 256, default the smaller of 4 and the subagent limit), mode "fresh", tools. pool.push([{key, input}]) admits a batch with unique keys; pool.close() seals input, and one aggregate notice arrives after every item settles; pool.inspect() reports state; pool.cancel() stops running and queued items. workpool.open(pool_id) reattaches after a reset. Limits: 64 pools per run, 128 items per pool.

Model calls
- completion(prompt, options?) makes one tool-free model request. schema returns parsed JSON; model accepts default, smol or slow.

Kernel-defined tools (JavaScript and Python)
- JavaScript tool(async function name(args) {...}) and Python @tool publish a kernel function. A child agent may call it when agent(..., tools) names it; the parent cell keeps waiting meanwhile. Python infers the schema from parameters, defaults and annotations; both languages accept an explicit description and schema. tool.defined() lists definitions and tool.undefine(name) removes one. Reset or redefinition invalidates older descriptors, and names cannot shadow host tools.

Data and environment
- pipeline(items, ...stages) passes every item through each stage in turn, running a stage's items with parallel().
- read(path, offset?, limit?) and write(path, content) do direct text I/O. local:// paths resolve to session artifacts and survive resets.
- env(key?, value?) reads or sets kernel environment variables.

Cell magics (each in its own JavaScript or Python cell)
- %load "path" runs a local script in the current kernel and keeps its variables.
- %pip install <spec> and %npm install <spec> install into a session-local environment without touching project files. Modules already imported keep the old version until reset.
