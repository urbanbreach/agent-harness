## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2024-05-18 - Avoid repeatedly building Regex for redact_value
**Learning:** In `crates/harness-core/src/redact.rs`, `DefaultRedactor` is instantiated many times throughout testing and core tasks to redact event payloads. It created two new `Regex` structures on every `default()` instantiation, allocating repeatedly.
**Action:** Use `std::sync::LazyLock` to statically cache expensive initialization (e.g., `Regex::new` compilations) to avoid repeated dynamic allocation overhead in `redact.rs`.
