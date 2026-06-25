## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2026-06-25 - Zero Allocation Event/Logging Key Analysis
**Learning:** Hot paths like redaction can suffer from allocating `Vec<String>` and multiple intermediate `String` variables simply for analyzing a split key.
**Action:** Favor iterating directly over string segments with zero-allocation strategies. For case-insensitive segment match checks, iterate directly using `.eq_ignore_ascii_case()` instead of forcing segments into a `to_ascii_lowercase()` collection.
