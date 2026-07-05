# Permissions guide and V1 threat model

Harness permissions are an operator approval layer, not a sandbox. They decide whether the coordinator may run a requested tool action; they do not confine the operating system, container, shell, provider, or editor once an operator approves a dangerous action.

## Permission names

The public V1 permission names are:

- `bash`
- `edit`
- `question`
- `task`
- `webfetch`
- `websearch`
- `codesearch`
- `lsp`

Legacy internal names such as shell/network may appear in compatibility code, but docs, prompts, and public config should use the V1 names above.

## Allow, ask, deny

`allow` lets the coordinator run the tool without another prompt. `ask` records a permission request and waits for operator approval. `deny` blocks the tool before execution and records the denial. Profile overrides, defaults, and selector rules are resolved by the coordinator before any native tool code runs.

## Mutable surfaces

Approving `edit` can change workspace files. Approving `bash` can run host commands inside the configured workspace and can indirectly mutate files; bash approvals may be scoped to reusable command patterns such as `cargo nextest run *`. Approving `task` can spawn child agents or control background work. Network permissions (`webfetch`, `websearch`, `codesearch`) can send request data to configured services. `question` can interrupt the operator flow. `lsp` can inspect code and, through rename-capable routes, may require edit permission for mutations.

## Runtime-enforced vs behavioral promises

The runtime-enforced vs behavioral split is explicit:

| Promise | Enforced by runtime? | Notes |
|---|---:|---|
| Tool availability per profile | yes | Coordinator filters toolsets before execution. |
| Permission decision before execution | yes | Permission policy returns allow/ask/deny before tool code runs. |
| Plan may edit only the active plan file | yes | Plan tools are constrained by runtime policy and plan handoff tools. |
| Explore is read-only | yes for configured tools | The shipped Explore profile lacks write/network/task tools. |
| Category routes deny recursion | yes by default config | Category profiles deny recursive `task` unless config changes. |
| Ask one question / keep responses concise | behavioral | Prompt guidance only; not a sandbox. |
| Prefer small changes and manual QA | behavioral | Verified by review/tests, not by permission policy. |

## Fixture cross-link

WS6 couples these promises to runtime behavior through the permission-promise fixture: prompt-claimed restrictions for plan, explore, general, and category routes must match coordinator-denied tools.
