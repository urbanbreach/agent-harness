use std::collections::BTreeSet;

use harness_core::tool::{build_tool_function_name_mapping, canonical_tool_id_for};

#[test]
fn function_name_mapping_is_stable_for_single_surface_tool_ids() {
    let tool_ids = vec![
        "apply_patch",
        "bash",
        "batch",
        "codesearch",
        "edit",
        "glob",
        "grep",
        "invalid",
        "list",
        "lsp",
        "plan_exit",
        "question",
        "read",
        "skill",
        "task",
        "todoread",
        "todowrite",
        "webfetch",
        "websearch",
        "write",
    ];
    let mapping_a = build_tool_function_name_mapping(tool_ids.iter().copied());

    let mut reversed_tool_ids = tool_ids.clone();
    reversed_tool_ids.reverse();
    let mapping_b = build_tool_function_name_mapping(reversed_tool_ids.iter().copied());

    assert_eq!(mapping_a, mapping_b);
    assert_eq!(mapping_a.function_to_tool_id().len(), tool_ids.len());
    assert_eq!(mapping_a.tool_id_to_function_name().len(), tool_ids.len());

    let unique_tool_ids = tool_ids.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(unique_tool_ids.len(), tool_ids.len());

    let unique_function_names = mapping_a
        .function_to_tool_id()
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(unique_function_names.len(), tool_ids.len());

    for tool_id in &tool_ids {
        assert_eq!(canonical_tool_id_for(tool_id), Some(*tool_id));
    }

    for (function_name, tool_id) in mapping_a.function_to_tool_id() {
        assert_eq!(
            mapping_a.tool_id_for_function_name(function_name),
            Some(tool_id.as_str())
        );
        assert_eq!(
            mapping_a.function_name_for_tool_id(tool_id),
            Some(function_name.as_str())
        );
    }
}
