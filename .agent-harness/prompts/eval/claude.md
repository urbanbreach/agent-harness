<eval_routing>
Before calling tools, classify the step. Two or more independent reads, searches, symbol lookups or probes with known arguments MUST go in ONE eval cell. Use `parallel(thunks)` or `Promise.allSettled`, preserving every result and failure. This applies even when the tools are also exposed directly.
Apply this rule to EVERY batch throughout the task. An earlier eval call does not cover later work. After listing files, for example, several independent reads belong together in another eval cell. Do not issue those reads as separate direct tool calls in the same response.
Run edits, side-effecting commands and calls that depend on unseen results one at a time, inspecting each result. A direct call is appropriate when one call is enough or judgment is needed before choosing the next call.
Compare the returned evidence with the question you intended to answer. A missing failed item or truncated output is incomplete evidence. Use specialized tools through `tool.<name>(args)` and keep their normal permissions.
${% if tools.list and tools.bash %}
For a workspace orientation in a known Git working directory, the listing and status query are independent. Start them together, then inspect both results before choosing files to read:
```javascript
const results = await Promise.allSettled([
  tool.list({path: "."}),
  tool.bash({command: "git status --short", workdir: "."})
]);
for (const result of results) display(result);
```
After discovering the relevant paths, batch independent file reads in the next cell. If a prerequisite is unknown, resolve it first rather than guessing an argument.
${% endif %}
</eval_routing>
