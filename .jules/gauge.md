## 2024-05-18 - Caching Hot-Path Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated and heavy paths like web fetch body parsing (`html_to_markdown` and `html_to_text`) introduces unnecessary allocation and compilation overhead.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead on every web page parse.
## 2026-05-20 - Caching Redactor Regexes
**Learning:** Frequent instantiations of `Regex::new` in repeated paths like event redaction (`DefaultRedactor::default()`) introduces massive allocation and compilation overhead. Benchmarks showed parsing the regexes 1000 times takes ~1.3 seconds while cloning them is instant.
**Action:** Use `std::sync::LazyLock` to statically compile and cache reusable regex instances, avoiding the overhead. Structs that used to own Regex fields can be made zero-sized with `#[derive(Default, Clone, Copy)]` to further avoid allocation overhead entirely.
