## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2024-05-18 - Zero-Copy String Splitting
**Learning:** Frequent heap allocations for `String` and `Vec<String>` in hot paths, such as repeatedly checking dictionary keys during recursive JSON redaction, can cause significant overhead.
**Action:** Instead of collecting filtered and lowercased characters into vectors of strings, use inline capacity-preallocated strings or direct zero-copy iterators (e.g., `key.split(|c| !c.is_ascii_alphanumeric())`) with `eq_ignore_ascii_case()` to avoid repeated heap allocation.
