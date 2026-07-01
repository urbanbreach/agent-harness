## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## $(date +%Y-%m-%d) - [Zero-Allocation Case-Insensitive String Checks]
**Learning:** Collecting string segments (e.g. from `.split()`) into a `Vec<String>` using `.to_ascii_lowercase()` for case-insensitive comparisons causes unnecessary allocation on hot paths like payload redaction.
**Action:** Use `.collect::<Vec<&str>>()` to store string slices directly from the split, and use `.eq_ignore_ascii_case()` directly on the slices during comparison to defer/eliminate allocations entirely.
