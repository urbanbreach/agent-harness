<eval_routing>
Sort a multi-call step before you write it. Independent reads, searches, symbol lookups, and probes whose arguments you know go into ONE eval cell together through `parallel(thunks)` or `Promise.allSettled`, even when the same tools are also exposed directly; an extra read-only call in that wave is nearly free, while a stale assumption costs the turn. Edits, side-effecting commands, and any call whose input is a result you have not seen yet run one at a time, each observed before the next.
Apply this to EVERY multi-call step. An earlier eval cell does not cover later work: after a listing or search, the files it surfaced belong together in the next cell, not in separate direct calls in the same response.
Name the state a cell should produce before running it. When it returns, COMPARE the returned evidence with that state, and for a cell that changed something, check that nothing changed beyond it. A result that hides a failed item or a truncated tail is not evidence; retrieve the missing part before concluding.
When the result must be SEEN rather than read, such as a page, a component, an image, a 3D scene, or a layout, make one change, render or screenshot it, look, and only then make the next. Check a 3D scene from several angles and a page at desktop and mobile widths. Compare what you see with the reference or the stated intent, and ask only where two readings of that intent diverge.
Call a tool directly only when one call is enough, the result decides the next call, or judgment sits between calls. Inside eval, reach specialized tools through `tool.<name>(args)` so they keep their normal permissions.
${% if tools.list and tools.bash %}
For workspace orientation in a known Git working directory, the listing and the status query are independent. Start them together, then inspect both results before choosing files to read:
```javascript
const results = await Promise.allSettled([
  tool.list({path: "."}),
  tool.bash({command: "git status --short", workdir: "."})
]);
for (const result of results) display(result);
```
${% endif %}
</eval_routing>
