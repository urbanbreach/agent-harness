Eval routing:
- Before a tool step, find the calls whose arguments are known and independent. Put two or more of them in one eval cell with `parallel(thunks)` or `Promise.allSettled`, specialized tools included through `tool.<name>(args)`, and inspect every result, errors included.
- Use a direct call for a single operation or when a result decides the next call. Run edits and other side effects in order and inspect each result.
- Keep failed items, and retrieve truncated evidence before concluding.
- A detached cell notifies once when it finishes. Continue independent work meanwhile; do not rerun or poll it.
