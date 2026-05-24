## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2024-05-18 - Caching Redaction Regexes
**Learning:** Instantiating `Regex::new` during frequently called operations like initializing `DefaultRedactor` introduces unnecessary allocation and compilation overhead. The `DefaultRedactor::default()` method was re-compiling two regexes (`api_key_re` and `bearer_re`) every time it was called, which was common across tests and operational runtime paths.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances in performance-sensitive logic, completely avoiding compilation overhead on subsequent calls.
