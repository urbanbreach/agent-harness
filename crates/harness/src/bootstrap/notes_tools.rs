/// The notebook and rollover tools exist only with the setting on, and only an
/// unrestricted primary profile that reads, searches and can change things receives them.
/// A restricted list is never widened.
pub(super) fn gate(toolset: &mut Vec<String>, enabled: bool, primary: bool) {
    const NOTES: [&str; 2] = ["context_notes", "new_context"];
    toolset.retain(|tool| enabled || !NOTES.contains(&tool.as_str()));
    let has = |tools: &[String], name: &str| tools.iter().any(|tool| tool == name);
    let unrestricted = ["edit", "write", "apply_patch", "bash"]
        .iter()
        .any(|name| has(toolset, name));
    if enabled && primary && unrestricted && has(toolset, "read") && has(toolset, "grep") {
        for name in NOTES {
            if !has(toolset, name) {
                toolset.push(name.to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::gate;

    #[test]
    fn notes_tools_follow_the_setting_and_never_widen_a_restricted_list() {
        let tools = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        let full = ["read", "grep", "edit"];
        for (name, list, enabled, primary, expected) in [
            (
                "on",
                tools(&full),
                true,
                true,
                tools(&["read", "grep", "edit", "context_notes", "new_context"]),
            ),
            ("off", tools(&full), false, true, tools(&full)),
            (
                "explicit but off",
                tools(&["read", "grep", "edit", "new_context"]),
                false,
                true,
                tools(&full),
            ),
            (
                "read-only",
                tools(&["read", "grep"]),
                true,
                true,
                tools(&["read", "grep"]),
            ),
            (
                "no search",
                tools(&["read", "edit"]),
                true,
                true,
                tools(&["read", "edit"]),
            ),
            ("subagent", tools(&full), true, false, tools(&full)),
        ] {
            let mut list = list;
            gate(&mut list, enabled, primary);
            assert_eq!(list, expected, "{name}");
        }
    }
}
