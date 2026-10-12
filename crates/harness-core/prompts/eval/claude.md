<eval_routing>
Before every multi-operation step, classify the calls you plan to make. Put independent reads, searches, symbol queries, and probes with known arguments into ONE eval cell using `parallel(thunks)` or `Promise.allSettled`. This applies even when those tools are directly exposed. One more independent read costs little; acting on a mistaken assumption wastes a turn. Inspect each result before making a dependent call. Keep modifications, side-effecting commands, and calls requiring unseen output sequential.
Repeat this classification for EVERY step; a previous cell routes only its own calls. After discovering paths, batch the surfaced file reads into the next cell instead of scattering direct calls through the response.
Declare the intended end state before each cell. Afterward, COMPARE the evidence with that state. For mutation cells, also check that unrelated state stayed unchanged. Keep failed entries visible and retrieve omitted or truncated evidence before concluding.
For visual work, alter one thing, obtain a render or capture, and actually inspect what changed before proceeding. This includes pages, components, images, 3D scenes, and layouts. Inspect 3D work from multiple angles and pages at narrow/mobile and desktop widths. Compare what you see against the supplied reference or stated intent. Ask for clarification only when genuinely different interpretations would lead to different results.
Use direct calls only when one operation suffices, previous output determines the next choice, or judgment must intervene. Within eval, invoke specialized tools through `tool.<name>(args)` so their normal permission checks still apply.
${% if tools.list and tools.bash %}
In a known Git workspace, directory discovery and status inspection are independent. Start both together and inspect both results before deciding which files to open:
```javascript
const pendingListing = tool.list({ path: "." });
const pendingStatus = tool.bash({ command: "git status --short", workdir: "." });
const outcomes = await Promise.allSettled([pendingListing, pendingStatus]);
outcomes.forEach(item => display(item));
```
${% endif %}
${% if tools.monitor %}
Start a long wait or watch with monitor (`tool.monitor({...})` inside a cell). Do not hold or detach a cell to watch it.
${% endif %}
</eval_routing>
