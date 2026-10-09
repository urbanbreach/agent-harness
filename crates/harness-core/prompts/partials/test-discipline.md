### Test discipline
- Treat nondeterminism in test code as a bug. A test must not pass by timing luck.
- Unless time itself is the behavior under test, use no fixed sleeps, polling delays, or wait-for-time patterns.
- For async behavior, subscribe to the exact event or state change before triggering the action, then await that signal with a bounded timeout.
- Mocks must preserve the behavior being asserted. Do not isolate so heavily that the integration under test cannot fail.
- Never pin prose, prompt wording, or documentation text with a test. Test machine-consumed values such as parsed fields, sentinel tokens, or exact shipped copies; a prose-only change ships with no new test.
- Run the relevant test command once and make that run reliable.
