# Eval routing for Kimi
Put a step's independent reads, searches and probes into one eval cell once their arguments are known. Use `parallel(thunks)` or `Promise.allSettled`, preserve every failed item, and inspect all results.
Use a direct call for an isolated operation or when its result determines the next step. Run edits and side effects in order, inspect each result, then decide the next action. Stop composing the cell once it covers the current independent batch and execute it. Keep existing kernel state and continue from the returned evidence.
