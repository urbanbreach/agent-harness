# Core tests

Coordinator cases live beside the implementation in `src/coord/*_tests.rs`.
Integration targets here exercise credentials, catalog transport, ACP, updates,
plugins, workspace data, and operator boundaries through public APIs.

Use temporary directories, `FakeClock`, scripted providers, and explicit
notifications. A bounded event wait should identify the owning request and
terminal event. Test close/reopen when durability matters. Local HTTP servers
are appropriate for wire contracts; external services and native subprocesses
need opt-in evidence.

Before adding a test, search existing coverage and prefer extending one case.
Protect a plausible behavioral regression, not getters, derived traits, or
private implementation shape. Do not add a test merely because code changed.

Never execute historical tools/hooks during resume. Reject sequence gaps,
orphan results, duplicate terminals, and malformed identities. Assert redaction
without printing secrets. Run `cargo nextest run -p harness-core --test <target>`.
