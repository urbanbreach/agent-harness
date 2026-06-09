## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2024-05-19 - Allocation-Free Redaction Key Segmentation
**Learning:** `redaction_marker_for_sensitive_key` created multiple temporary `String`s and `Vec<String>`s via `chars().filter().collect()`, `split().collect()`, and string slicing to segment key names. This was an incredibly hot path taking place for every key in deeply nested objects/events, unnecessarily thrashing the allocator.
**Action:** Replace `String`/`Vec` collections with on-the-fly iteration over `chars()` and `split()`, accumulating state linearly without heap allocations where possible.
