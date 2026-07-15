## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## $(date +%Y-%m-%d) - Zero-allocation iterators over string segments for performance
**Learning:** Found an anti-pattern in `redact.rs` where `key.split().filter().map(to_ascii_lowercase).collect::<Vec<_>>` is used in a hot path for event/log payload redaction. Allocating a `Vec<String>` and strings for each lowercase transformation is unnecessary when we just need case-insensitive substring matching.
**Action:** When validating segments or scanning strings, avoid `.collect::<Vec<String>>()`. Instead, pass down an iterator directly (e.g. `impl Iterator<Item = &'_ str> + Clone`) and use `segment.eq_ignore_ascii_case(target)` with fixed string targets to evaluate without extra heap allocations.
