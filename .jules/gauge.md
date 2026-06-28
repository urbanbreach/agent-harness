## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2025-02-14 - Eliminate string allocations in hot redact paths
**Learning:** Frequent small string allocations (like mapping `to_ascii_lowercase()` and collecting into a `Vec<String>`) in heavily called hot paths like `redact_map` processing JSON object keys introduce measurable overhead. Iterators over `&str` and applying `.eq_ignore_ascii_case()` is a zero-allocation alternative that yields correct case-insensitive behavior without needing heap allocations.
**Action:** When filtering or inspecting strings for patterns (like identifying secret key shapes), use `split` iterators over `&str` slices and inline case-insensitive checks rather than mapping to owned lowercase strings.
