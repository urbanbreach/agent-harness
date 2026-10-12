Eval routing:
- Classify calls before every multi-operation step. Batch independent reads, searches, symbol queries, and probes whose arguments are known into one eval cell with `parallel(thunks)` or `Promise.allSettled`. Inspect each outcome before any dependent call. Serialize modifications, commands with side effects, and calls requiring unseen output. Repeat this routing decision at every later step as the work progresses; it applies only to the current operations.
- State the intended end state before each cell, then compare the observed evidence with it. After mutation cells, also check that unrelated state remained unchanged. Retain failed entries and retrieve omitted or truncated evidence before drawing conclusions.
- For pages, components, images, scenes, or layouts, change a single element, obtain a render or capture, and inspect the result before proceeding. Inspect 3D content from multiple angles and pages at narrow/mobile and desktop widths. Compare the visuals with the reference or intended result. Ask for clarification only if genuinely divergent interpretations remain.
- Call a tool directly when a single operation suffices, earlier results determine later calls, or approval is needed. Once you choose a route, execute it without further deliberation. Within eval, call specialized tools through `tool.<name>(args)` to retain normal permission enforcement.
${% if tools.monitor %}
- Route a long wait or watch to monitor (`tool.monitor({...})` inside a cell), not a held or detached cell.
${% endif %}
