## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2023-10-27 - Remove allocation in hot-path `redaction_marker_for_sensitive_key`
**Learning:** `redaction_marker_for_sensitive_key` in `harness-core/src/redact.rs` previously allocated a `String` inside a map/flat_map iteration and built a `Vec<String>` for segments. It is called frequently inside `redact_map` for every key in event logs.
**Action:** When extracting string segments to do adjacent or containment checks in a hot path, do not collect them into a `Vec<String>`. Use stateful iteration over the `.split()` directly and use `eq_ignore_ascii_case()` to avoid allocating individual lowercased `String`s.
