## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.

## 2024-05-18 - Caching Redactor Regex Compilation
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like `DefaultRedactor::default()` introduces unnecessary allocation and compilation overhead. `regex::Regex` cloning in Rust is incredibly fast since it internally increments an atomic reference count.
**Action:** Use `std::sync::LazyLock` to compile regex patterns statically once, then `clone()` the cached `LazyLock` reference in the `Default` trait implementation.
## 2026-06-17 - String Substring Iteration
**Learning:**  allocates under the hood or ignores strict constraints when combined with  when evaluating strings. In the specific context of evaluating sensitive object keys within  during heavy execution, this dynamically allocates a large quantity of short-lived  buffers that can negatively impact performance.
**Action:** Replace heap allocating  methods and logic with zero-allocation in-line comparisons leveraging iteration directly over string splits and string slices.

## $(date +%Y-%m-%d) - String Substring Iteration
**Learning:** `std::str::contains` string manipulation techniques when combined with `chars().filter().flat_map().collect::<String>()` dynamically allocate a large quantity of short-lived `String` buffers that can negatively impact performance, particularly when evaluating a high volume of payload keys for redaction.
**Action:** Replace heap allocating `collect::<String>()` methods and logic with zero-allocation in-line comparisons leveraging iteration directly over string splits and string slices.
