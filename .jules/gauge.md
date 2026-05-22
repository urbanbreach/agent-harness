## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2024-05-19 - Caching Hot-Path Regexes in Redaction
**Learning:** Frequent instantiations of `DefaultRedactor::default()` caused compilation and allocation of repeated `Regex::new` objects on every call across tests and runtime.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead.
