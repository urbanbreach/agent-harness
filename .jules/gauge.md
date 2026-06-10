## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2024-06-10 - Avoid Vec allocations when analyzing string segments
**Learning:** In hot paths (like `redaction_marker_for_sensitive_key`), mapping string segments to lowercase and storing them in a `Vec<String>` causes significant allocation overhead.
**Action:** Yield iterators directly over string splits and use `eq_ignore_ascii_case` or inline character matching instead of allocating `Vec<String>` to dramatically reduce execution time.
