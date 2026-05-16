## 2025-05-16 - [Regex Compilation in `DefaultRedactor`]
**Learning:** `Regex::new` in `crates/harness-core/src/redact.rs` was being called during every instantiation of `DefaultRedactor::default()`, which happens frequently during event logging and parsing. Since `Regex` compilation is expensive, doing this in a hot path creates significant overhead (~150µs per call vs ~1µs when cached).
**Action:** When a struct only needs stateless regexes, store them in `std::sync::OnceLock` (or `LazyLock`) rather than compiling them dynamically inside `Default::default()` or hot methods.
