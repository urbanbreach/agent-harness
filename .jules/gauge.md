## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2025-02-18 - Avoid Vector Allocations for Key Segment Checking in Redaction
**Learning:** In hot paths like event payload redaction where `redaction_marker_for_sensitive_key` is called repeatedly for every map key, using `.split().map().collect::<Vec<_>>()` causes unnecessary allocations.
**Action:** Use an iterator over `.split()` directly and keep track of state variables (e.g. `prev_segment`) to test against adjacent segments without collecting the pieces into a `Vec<String>`.
