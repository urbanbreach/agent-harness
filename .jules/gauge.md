## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2025-06-13 - Avoid allocating Vec<String> when parsing token segments in hot paths
**Learning:** In `crates/harness-core/src/redact.rs` the hot path `redaction_marker_for_sensitive_key` originally allocated a `String` to track normalized keys and a `Vec<String>` using `.collect::<Vec<_>>()` simply to iterate over split keywords. Doing this repetitively in redaction sweeps creates enormous allocation churn.
**Action:** Replace `collect::<Vec<String>>()` over `str::split` with inline iteration using iterators or `for` loops directly, and track adjacent context via a `prev` reference. Avoid allocating new `String` instances wherever an inline iterator over chars or slices provides the necessary context.
