### Dependable tests

- Treat nondeterminism as a test defect. A passing result must not depend on a lucky execution order.
- Never add fixed sleeps, polling delays, or elapsed-time waits unless the test specifically exercises time.
- For asynchronous behavior, register for the precise transition you need before performing its trigger. Await the resulting signal with a finite deadline.
- Mocking must leave the asserted behavior intact. Never sever the tested integration so completely that it cannot fail.
- Never assert narrative wording, prompts, or documentation. Assert machine-consumed data, such as parsed values, sentinels, or exact shipped artifacts. Do not add tests for prose-only changes.
- Execute the relevant test command a single time. That invocation must be dependable.
