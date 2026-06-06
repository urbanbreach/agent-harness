## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2025-05-19 - [Redaction Micro-Optimization]
**Learning:** In `crates/harness-core/src/redact.rs`, checking if sensitive keys match redactor patterns previously iterated character by character using `collect::<String>()` or `collect::<Vec<_>>()`, which causes a noticeable amount of allocation on every key string evaluated for redaction. Keys in JSON responses can be numerous, meaning these allocations add up rapidly in hot loops.
**Action:** Replace `collect::<String>()` with `String::with_capacity(key.len())` and iterate inline or use direct iterators. Removing `collect::<Vec<_>>()` by rewriting `adjacent_segments` and `credential_key_segments` to take advantage of iterating strings using `split` over character sequences provides a measurable performance gain.
