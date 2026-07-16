## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2025-03-05 - Zero-allocation string segment checks in hot paths
**Learning:** In hot paths like event/logging redaction (`crates/harness-core/src/redact.rs`), splitting string keys by non-alphanumeric characters and converting each segment to a new lowercase `String` using `.map(|segment| segment.to_ascii_lowercase()).collect::<Vec<_>>()` introduces unnecessary heap allocations for every segment. Furthermore, the `matches!` macro cannot directly accept a runtime case-insensitive check like `.eq_ignore_ascii_case()`.
**Action:** Remove the allocation step entirely and collect segments as a `Vec<&str>`. To perform the necessary case-insensitive checks without allocation, pass `&[&str]` to helper functions and substitute the `matches!` macro block by defining an array of target string literals (`["access", "api", "auth", ...]`) and iterating over it: `targets.iter().any(|target| segment.eq_ignore_ascii_case(target))`. This preserves exact behavior while keeping the fast path zero-allocation.
