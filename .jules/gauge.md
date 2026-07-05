## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.

## 2024-05-19 - Safe Substring Slicing for Performance
**Learning:** Slicing `&str` directly via computed index without first checking `.is_char_boundary()` creates panic vulnerabilities on non-ASCII strings.
**Action:** When manually checking string prefixes or suffixes, either use `as_bytes()` to work safely on bytes, or `is_char_boundary()` before taking a UTF-8 slice.
