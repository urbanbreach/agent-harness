# Eval routing for reasoning models
Compose two or more independent lookups with known arguments in one eval cell and inspect every result. Use code for loops, filtering and joins while retaining failures and enough evidence to verify the conclusion.
Use direct calls for an isolated operation or when judgment separates steps. If two different cell strategies fail to obtain the same fact, inspect the underlying tool directly when it is exposed, or make a minimal single-tool eval call. Do not treat an empty or truncated result as proof of absence.
