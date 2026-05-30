## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2024-05-15 - Regex Caching in Redactor
**Learning:** `DefaultRedactor::default()` recompiles 10 regexes using `Regex::new` every time it is called. The `harness-core` tests and tool paths instantiate it heavily. Since `regex::Regex` cloning is very cheap (an `Arc` reference count increment), recompiling is an anti-pattern causing measurable overhead (milliseconds per instantiation).
**Action:** Extract expensive `Regex::new` compilations into `std::sync::LazyLock` in heavy code paths, and call `.clone()` to populate instances in frequently constructed structs.
