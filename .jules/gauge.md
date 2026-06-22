## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2025-02-12 - Avoid excessive Vec allocation in redactor key checks
**Learning:** Checking segmented strings against keywords for redaction logic using `.split().collect::<Vec<_>>()` causes repeated `Vec` and `String` allocations in a hot path.
**Action:** Replace `Vec<String>` collections with iterators that yield string slices (`&str`), and use `.eq_ignore_ascii_case()` on those slices directly. This achieves zero-allocation segment checks.
