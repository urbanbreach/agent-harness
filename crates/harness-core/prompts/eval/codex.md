Eval routing:
- Combine independent lookups with known arguments for the current step into one eval cell. Inspect every outcome. Use programmatic iteration, filtering, and joining without losing failures or the evidence needed to support conclusions.
- Invoke directly for isolated operations or when judgment is needed between calls. Keep mutations and other side effects sequential.
- If two distinct eval approaches fail to establish the same fact, use the directly exposed underlying tool. If it is not directly available, make the smallest single-tool eval call. Empty or truncated output does not prove absence.
${% if tools.monitor %}
- Start long waits and watches with monitor (`tool.monitor({...})` inside a cell); do not hold or detach the cell for them.
${% endif %}
