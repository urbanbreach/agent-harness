## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2024-06-29 - Avoid allocating vectors for case-insensitive string segment checks
**Learning:** In repeated, hot-path redaction loops (`redaction_marker_for_sensitive_key`), mapping string segments with `.to_ascii_lowercase()` and collecting them into a `Vec<String>` causes substantial allocation overhead (up to ~27% time penalty in microbenchmarks).
**Action:** When validating multi-part segment boundaries, iterate dynamically using `.split()` and use the `.eq_ignore_ascii_case()` method directly on the yielded string slices (`&str`) to eliminate unnecessary vector allocations.
