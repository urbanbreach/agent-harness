# Eval routing
Use one eval cell for two or more independent tool calls whose arguments are known. Batch with `parallel(thunks)` or `Promise.allSettled` and retain each result, including failures. Workspace orientation and reading several discovered files are typical batches.
Use direct calls for isolated operations. Inspect a result before choosing any call that depends on it. Execute edits and side effects in order and inspect each result. Never guess an argument to fill a batch. Preserve failed items and retrieve truncated evidence before deciding.
