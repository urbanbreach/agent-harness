You are implementing a config restructure for the agent-harness Rust workspace. The full spec is at `.agent-harness/plans/config-restructure-spec.md`. Read it now — all of it. It is your single source of truth.

## MODEL

Use `umans-ai-coding-plan/umans-glm-5.2` with variant `high` for all your work. This model is configured in the project's `harness.jsonc` under the `umans-ai-coding-plan` provider. If you are running as a harness agent, ensure your agent profile uses this model. If you are running as an external agent, use this model for any provider calls you make.

## EXECUTION MODEL

Work through the 7 tasks in 3 waves as specified in the dependency graph (Section 9). Within each wave, tasks are independent and may be done in any order. Do not start Wave 2 until every task in Wave 1 is verified. Do not start Wave 3 until Wave 2 is verified.

For each task:
1. Read the relevant Opencode source file(s) listed in Section 4 of the spec
2. Read the actual file(s) you will edit — do not edit from memory
3. Write failing tests FIRST (TDD is mandatory, not optional)
4. Implement the change
5. Run the task-specific verification command and show actual output
6. Run `cargo test -p harness-core` and show actual output
7. Run `cargo clippy -p harness-core -- -D warnings` and show actual output
8. Run `cargo fmt --check -p harness-core` and show actual output
9. Show `git diff` of all changes
10. Commit only after all verification passes

## END-TO-END RUNTIME VERIFICATION (MANDATORY)

Unit tests are not enough. After Task 4 and again after Task 7, you MUST run the end-to-end runtime verification described in Section 6.4 of the spec. This means:

1. Create a test workspace at `/tmp/harness-e2e-test/` with a custom agent markdown file carrying FULL frontmatter (model, variant, temperature, permissions, tools, max_iters, tool_failure_mode)
2. Run `harness doctor --json` against that workspace and verify the custom agent appears with the correct config values FROM MARKDOWN (not from JSON config)
3. Run `harness run --mock "..."` and verify the agent is actually used
4. Run the offline stress harness against the test workspace
5. Verify discovery last-wins works at runtime (project markdown overrides shipped)
6. Verify disabled agents are hidden from the catalog

Show actual output for every step. Map each frontmatter field to the doctor JSON output field to prove the value came from markdown. If a unit test passes but the runtime doesn't reflect the change, the implementation is NOT complete — go back and fix it.

## ANTI-GAMING ENFORCEMENT

Section 7 of the spec lists 15 forbidden behaviors and 8 required behaviors. These are not suggestions. Key enforcement:

- Every test you write must have real assertions that test real behavior. A test that calls a function without asserting is not a test.
- Every verification command must be actually run with output shown. "Should pass" is not evidence. "Tests pass" without output is not evidence.
- If a test fails after your change, fix the code, not the test. The only exception is when the test was testing wrong behavior — in that case, replace it with a correct test and explain why the old one was wrong.
- No `unwrap()`, `expect()`, `panic!`, `todo!`, `unreachable!`, `as any`, `@ts-ignore`, or type suppression in production code. Tests may use `unwrap()`/`expect()`.
- No skipping hard parts. If a task says fix 8 fields, fix all 8.
- No adding changes beyond what the task requires. Note unrelated bugs but do not fix them in the same change.
- No modifying shipped agent markdown files (`.agent-harness/agents/*.md`). Use test fixtures in the test directory if you need sample markdown files.
- No skipping the E2E verification. "Unit tests pass so E2E would pass too" is not valid reasoning. Run the actual harness binary.

## OPENCODE REFERENCE

Before starting each task, read the Opencode source file(s) listed in Section 4.1 of the spec. The Opencode source is at `inspirations/opencode/`. Do not copy Opencode's TypeScript code — learn the patterns and implement in idiomatic Rust. The spec tells you exactly what to learn from each file.

## YOUR FREEDOM

Section 8 of the spec defines where you have latitude: test structure, implementation details, documentation style, commit message wording, test file location, and the model_ref fallback approach. Make decisions in these areas without asking — just document your choice in the commit message or a code comment.

## WHEN YOU ARE STUCK

If you cannot make a task pass after 3 genuinely different approaches:
1. Stop editing
2. Write up what you tried, what failed, and what the error output was
3. Move to the next task in the same wave (if any remain)
4. Come back to the blocked task after the wave completes

Do not brute-force the same approach repeatedly. Do not weaken tests to make them pass. Do not comment out failing code.

## STOPPING CONDITION

You are done when ALL of the following are true (Section 10 of the spec):
1. All 7 tasks are implemented and committed
2. The full verification suite passes (Section 6.2):
   ```
   cargo test -p harness-core
   cargo test -p harness --test config_docs_reference_test
   cargo test -p harness --test config_schema_cli_test
   cargo test -p harness --test bootstrap_profiles_test
   cargo run -p harness -- --config configs/harness.example.jsonc config validate
   cargo run -p harness -- --config configs/harness.example.jsonc doctor --json
   scripts/test-lanes.sh fast
   scripts/test-lanes.sh quality-gates
   ```
3. End-to-end runtime verification passes (Section 6.4) — custom agent markdown with full frontmatter loads correctly at runtime, discovery last-wins works, disabled agents are hidden
4. Every command's actual output has been shown
5. No forbidden behaviors were committed
6. Plan agent's PermissionRuleSet stays in Rust code (not markdown)
7. Hidden system agents (title, summary, compaction) stay compiled-in Rust defaults

Start by reading the spec at `.agent-harness/plans/config-restructure-spec.md`. Then begin Wave 1, Task 1.
