## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2025-02-14 - Avoiding String Allocation in Hot Paths
**Learning:** In hot paths such as event and logging redaction (`redaction_marker_for_sensitive_key`), repeatedly allocating `String` or `Vec<String>` to segment keys introduces a significant overhead. The project avoids unsafe code and clever micro-optimizations, but converting to character iteration and string slice splits brings significant performance improvements.
**Action:** For performance optimization in hot paths, avoid creating and allocating `Vec<String>` or `String` when segmenting keys. Rely on iterators directly over string splits and use inline case-insensitive comparisons to prevent excessive allocation overhead.
