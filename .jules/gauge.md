## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2024-05-24 - Zero-allocation case-insensitive segment checks
**Learning:** For performance optimizations in hot paths (like event/logging redaction logic), avoid creating and allocating `Vec<String>` or `String` when segmenting keys. Rely on iterators directly over string splits and use inline case-insensitive comparisons to prevent excessive allocation overhead. When matching string slices case-insensitively in Rust, the `matches!` macro cannot be used directly with `.eq_ignore_ascii_case()`. Instead, define an array of target strings and use `.iter().any(|target| segment.eq_ignore_ascii_case(target))` for zero-allocation matching.
**Action:** When working on similar hot paths, always use `&str` and case-insensitive comparison methods like `eq_ignore_ascii_case` over converting and allocating `String`s.
