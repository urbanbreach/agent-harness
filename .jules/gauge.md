## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-29 - Caching Secret Scanning Regexes
**Learning:** Instantiating new regex objects every time `DefaultRedactor::default()` is called incurs a significant performance penalty (dynamically re-compiling up to 10 regexes per instantiation).
**Action:** Use `std::sync::LazyLock` in secret redaction paths (like `crates/harness-core/src/redact.rs`) to pre-compile and cache regexes globally to prevent recompilation, especially given how often redaction is performed on messages/logs.
