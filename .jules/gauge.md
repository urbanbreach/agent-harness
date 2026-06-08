## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2024-06-08 - Avoid allocation when normalizing strings for redacted sensitive key checking
**Learning:** `redaction_marker_for_sensitive_key` previously created a new `String` for the normalized key and a `Vec<String>` for segments during event serialization, creating a hot allocation loop for JSON object fields.
**Action:** Replace `collect::<String>()` and `.collect::<Vec<_>>()` on parsed string parts with allocation-free, iterator-based scanning using `.chars()` and inline index-based matching or matching split bounds in hot redaction functions.
