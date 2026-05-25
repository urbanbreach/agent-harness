## 2024-05-15 - [Regex compilation in hot loops/frequently instantiated types]
**Learning:** `DefaultRedactor` in `harness-core` was compiling two `Regex`es every time it was instantiated (`Default::default()`). Since it's instantiated frequently (e.g., in `Arc::new(DefaultRedactor::default())`), this caused a significant unseen performance cost.
**Action:** Use `std::sync::LazyLock` to statically compile regexes only once at runtime for types that are instantiated frequently but use the same static regex patterns.
