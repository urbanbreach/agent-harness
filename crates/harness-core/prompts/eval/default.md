Eval routing:
- Prefer eval when a step's calls are independent. One cell with `parallel(thunks)` or `Promise.allSettled` runs them together and keeps every failure in its result. Orienting in a workspace and reading several discovered files are typical batches.
- Run edits and result-dependent calls one at a time, each observed before the next. Use direct calls for isolated operations.
- Never guess an argument to fill a batch. Keep failed items, and retrieve truncated evidence before deciding.
