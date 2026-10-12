Eval routing:
- Before each tool step, identify independent calls with known arguments. Batch two or more into one eval cell using `parallel(thunks)` or `Promise.allSettled`, including specialized calls via `tool.<name>(args)`. Inspect every returned item, failures included.
- Call directly for isolated work or when earlier output determines later arguments. Perform edits and other side effects sequentially, inspecting each result.
- Keep failed outcomes and obtain missing portions of truncated evidence before drawing a conclusion.
${% if tools.monitor %}
- Start a long wait or watch with monitor (`tool.monitor({...})` inside a cell), rather than holding or detaching the cell.
${% endif %}
- A detached cell sends one completion notice. Continue independent work while it runs; never repeat its execution or poll its status.
