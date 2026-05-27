## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-24 - DefaultRedactor repeated Regex compilation
**Learning:** `DefaultRedactor::default()` creates new instances of `Regex` on every instantiation, which causes repeated and expensive compilation overhead, particularly because it's called repeatedly across tests and some operations.
**Action:** Use `std::sync::LazyLock` to statically cache and reuse compiled regular expressions inside stateless components like redactors to avoid repeated compilation and allocation.
