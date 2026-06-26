## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## $(date +%Y-%m-%d) - Caching Secret Scanner Regex Patterns
**Learning:** Frequent calls to `default_forbidden_patterns()` inside the `secret_scanner.rs` creates a new vector of `ForbiddenPattern` each time. This includes instantiating new `Regex::new` patterns on each scan, creating large allocation and compilation overhead in the hot path.
**Action:** Use `std::sync::LazyLock` to statically compile and cache the vector of patterns once and return it as a slice `&[ForbiddenPattern]`, eliminating re-allocations and re-compilations on every directory or file scan.
