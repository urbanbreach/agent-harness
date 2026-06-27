## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2025-02-12 - Avoid unnecessary String allocations in hot redaction path
**Learning:** In hot paths like event and logging redaction (`redact.rs`), splitting strings and immediately mapping them into `.to_ascii_lowercase()` `Vec<String>` collections introduces unnecessary allocation and copying overhead for every key inspected.
**Action:** Use iterators that operate directly on `&str` and utilize `.eq_ignore_ascii_case()` for zero-allocation, case-insensitive string comparisons.
