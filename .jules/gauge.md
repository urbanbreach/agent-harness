## 2024-05-18 - Avoid repeated regex compilation
**Learning:** `crates/harness-tools/src/network.rs` repeatedly compiled multiple regexes on every call to `html_to_markdown` and `html_to_text`, and `crates/harness-core/src/redact.rs` compiled regexes inside the implementation of `DefaultRedactor::default()`. This caused performance overhead and unnecessary allocations.
**Action:** Use `std::sync::LazyLock` for regexes that are used in hot paths or instantiated repeatedly to compile them only once.
