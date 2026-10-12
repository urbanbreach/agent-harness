Eval routing:
- Prefer batching independent calls in eval with `parallel(thunks)` or `Promise.allSettled`, keeping failed items. For example, batch workspace discovery, then batch reads of several discovered files.
- Serialize changes and result-dependent calls. Inspect each outcome before continuing. Use direct calls for isolated work.
- Never invent arguments to fill a batch. Preserve failures and retrieve truncated output before making a decision.
${% if tools.monitor %}
- Use monitor for a long wait or watch (`tool.monitor({...})` inside a cell), rather than holding or detaching the cell.
${% endif %}
