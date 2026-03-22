use std::collections::BTreeSet;

use harness_core::tool::{
    build_tool_function_name_mapping, canonical_tool_id_for, native_and_alias_tool_ids,
    native_tool_parity_matrix,
};

#[test]
fn function_name_mapping_is_stable_for_native_and_alias_tool_ids() {
    let tool_ids = native_and_alias_tool_ids();
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

    for entry in native_tool_parity_matrix() {
        assert_eq!(
            canonical_tool_id_for(entry.canonical_id),
            Some(entry.canonical_id)
        );
        for alias in entry.aliases {
            assert_eq!(canonical_tool_id_for(alias), Some(entry.canonical_id));
        }
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
