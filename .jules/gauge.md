## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2024-06-21 - Avoiding Vec<String> Allocations in Hot Path
**Learning:** Frequent instantiations of `Vec<String>` and mapping to `.to_ascii_lowercase()` inside hot paths (e.g., when iterating over every JSON key inside an event log or JSON stream) can add up and introduce significant allocation pressure.
**Action:** Replace `Vec<String>` allocations with direct iteration over `&str` splits, using `.eq_ignore_ascii_case()` and manually tracking loop state for operations like `windows(2)` to avoid heap allocations.
