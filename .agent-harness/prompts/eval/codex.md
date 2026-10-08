Eval routing:
- Route a step's independent lookups with known arguments through one eval cell and inspect every result. Use code for loops, filtering, and joins, keeping failures and enough evidence to verify the conclusion.
- A direct call is right when one call is sufficient or when judgment separates the steps. Run edits and side effects in order.
- If two different cell strategies fail to obtain the same fact, call the underlying tool directly when it is exposed, or make a minimal single-tool eval call. An empty or truncated result is not proof of absence.
