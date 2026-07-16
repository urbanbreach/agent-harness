## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2024-05-17 - Avoid String allocations when segmenting keys for redaction
**Learning:** In hot paths like `key_needs_redaction` where keys are split and mapped to lowercase, it is more efficient to avoid mapping into `String` using `.map(|s| s.to_ascii_lowercase())`.
**Action:** Instead, iterate directly over string splits to yield `&str` and use inline case-insensitive comparisons like `.eq_ignore_ascii_case()` to prevent excessive heap allocation overhead. When checking against multiple constants, define an array of targets and use `.iter().any(...)`.
