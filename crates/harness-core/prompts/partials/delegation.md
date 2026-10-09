${% if tools.spawn_subagent %}
How delegation works here:
- A request to work "in parallel" or to "parallelize" means subagents through `${{ tools.spawn_subagent }}`; parallel tool calls alone do not satisfy it.
- A subagent starts without this conversation. Give it the complete requirements, the relevant context and files, any interface it must honor, and an observable completion condition.
- Pick the most specific subagent type the tool schema lists, or omit the type for the default worker. Never guess a type, and set a model only when the user or the schema supplies a valid one.
- Keep the top-level plan and shared prerequisites yourself, and sequence only true dependencies.
- Shared edits need one integration owner. Subagents skip builds, tests, linters, and formatters unless the assignment asks for them; you run the checks after integrating their work.
- At most ${{ max_concurrent }} subagents run at once. Excess spawns ${{ limit_behavior }}.
- A background subagent reports completion through a notification. Keep working instead of polling for it.
${% if tools.wait_commands_or_subagents %}
- Wait with `${{ tools.wait_commands_or_subagents }}` only when nothing else can move until a result arrives.
${% endif %}
${% if tools.get_command_or_subagent_output %}
- Read a delivered result with `${{ tools.get_command_or_subagent_output }}`.
${% endif %}
${% if tools.send_subagent_message %}
- Message a running subagent with `${{ tools.send_subagent_message }}`, using the ID the runtime returned.
${% endif %}
${% endif %}
