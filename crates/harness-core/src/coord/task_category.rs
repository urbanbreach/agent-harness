pub const TASK_CATEGORY_FALLBACK_PROFILE: &str = "general";
pub const TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES: &[&str] = &["plan"];

pub fn task_category_fallback_profile(category: &str) -> Option<&'static str> {
    let category = category.trim();
    (!category.is_empty() && !category.eq_ignore_ascii_case(TASK_CATEGORY_FALLBACK_PROFILE))
        .then_some(TASK_CATEGORY_FALLBACK_PROFILE)
}

pub fn task_category_fallback_chain(category: Option<&str>) -> Vec<String> {
    let Some(category) = category.map(str::trim).filter(|value| !value.is_empty()) else {
        return Vec::new();
    };
    let mut chain = vec![category.to_string()];
    if let Some(fallback) = task_category_fallback_profile(category) {
        chain.push(fallback.to_string());
    }
    chain
}

pub fn task_category_fallback_disabled_for_parent(parent_profile: Option<&str>) -> bool {
    parent_profile.is_some_and(|profile| {
        TASK_CATEGORY_FALLBACK_DISABLED_PARENT_PROFILES
            .iter()
            .any(|disabled| profile.eq_ignore_ascii_case(disabled))
    })
}
