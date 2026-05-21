## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2024-05-18 - Avoid repeated `Regex::new` inside `Default::default()`
**Learning:** `DefaultRedactor::default()` repeatedly instantiating `Regex::new` in `crates/harness-core/src/redact.rs` introduced heavy allocation overhead, especially because `DefaultRedactor` is created frequently in many tests and application cycles.
**Action:** Always prefer statically caching standard library-type initializations, such as regex patterns, with `std::sync::LazyLock` in globally used instances or types that might be instantiated repetitively via `.default()`.
