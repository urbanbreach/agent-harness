# Remove the test-only completion stub

The predecessor is `fdeee80bfee91d0c66be899b75988878c2d97307`.
`app/composer_editing.rs` had a completion implementation compiled only for tests:
a thread-local replacement range, eleven helper methods and ten self-tests.
Every caller of those methods was inside that same test module. Production
completion instead routes through `AppState::composer_*`, `ComposerSlice` and
`completion_controller`; slash commands have their own real application path.

Remove the stub and its ten tests: 349 test-only lines, no production changes.
The remaining file is 293 lines and is byte-identical to the predecessor's
non-test prefix. No public API, production behavior or reference snapshot changes.
No new tests are needed; existing behavioral coverage stays in place.

Before removal, 57 selected completion, slash-command and stub tests pass.
Temporarily changing real `insert_completion` to return the unchanged editor
makes two existing public-boundary tests fail: insertion preserving attachments,
and acceptance through the production application's Enter handler. All ten stub
tests still pass under that mutation. The source is restored before removal.
This demonstrates the difference between the retained production checks and the
removed implementation-only checks; it is not a claim of exhaustive coverage.

After removal, all 1,658 remaining TUI tests pass, including the recorded reference
journeys. Workspace compilation, Clippy, formatting and suite gates pass. Commands
and results are in `checks.json`; raw pre-change and mutation logs are retained.
`red.py` reproduces the mutation on the predecessor and restores its source in
`finally`. On the cleaned-up version it exercises only the surviving checks.
The source patch, before/after hashes and artifact manifest identify reviewed bytes.

No release benchmark or fresh PTY/browser capture is required for this test-only
deletion; no runtime or resource improvement is claimed. The preceding snapshot
ownership commit contains the latest release and terminal evidence. Whole TUI
source is now 166,604 lines (8.4% below the original), including test code under
`src`. The 90,941-line target and full replacement remain incomplete. Existing
CLI failures, timing uncertainty, legacy state/text engines and unverified
platforms remain documented in the main rewrite report.
