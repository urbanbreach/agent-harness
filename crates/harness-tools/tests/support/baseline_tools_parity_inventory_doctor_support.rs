use std::collections::{BTreeMap, BTreeSet};

#[test]
fn doctor_catalog_summary_matches_inventory_shape() {
    // arrange
    let report = doctor_report();
    let native_inventory_count = super::load_parity_inventory()
        .iter()
        .filter(|row| row.source == "harness_native")
        .count();

    // act
    let details = native_catalog_details(&report);

    // assert
    assert_eq!(
        details
            .get("catalog_source")
            .and_then(serde_json::Value::as_str),
        Some("harness_tools::tool_catalog")
    );
    assert_eq!(
        details
            .get("tool_count")
            .and_then(serde_json::Value::as_u64),
        Some(native_inventory_count as u64)
    );
    let doctor_tools = details
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("doctor native tool array");
    assert_eq!(doctor_tools.len(), native_inventory_count);
    for tool in doctor_tools {
        assert!(tool.get("canonical_id").is_some());
        assert!(tool.get("provider_function_name").is_some());
        assert!(tool.get("profile_description_overrides").is_some());
        assert!(tool.get("baseline_mapping_status").is_some());
        assert!(tool.get("baseline_equivalent_id").is_some());
    }
}

#[test]
fn inventory_active_profiles_match_doctor_resolved_route_toolsets() {
    // arrange
    let report = doctor_report();
    let active_profiles = active_profiles_by_tool(&report);

    // act
    let mismatches: Vec<_> = super::load_parity_inventory()
        .iter()
        .filter(|row| {
            row.active_profiles
                != active_profiles
                    .get(&row.canonical_id)
                    .cloned()
                    .unwrap_or_default()
        })
        .map(|row| row.canonical_id.clone())
        .collect();

    // assert
    assert!(
        mismatches.is_empty(),
        "active profile drift for tools: {:?}",
        mismatches
    );
}

#[test]
fn inventory_profile_description_overrides_match_doctor_native_catalog() {
    // arrange
    let report = doctor_report();
    let overrides = profile_description_overrides_by_tool(&report);

    // act
    let mismatches: Vec<_> = super::load_parity_inventory()
        .iter()
        .filter(|row| {
            row.profile_description_overrides
                != overrides
                    .get(&row.canonical_id)
                    .cloned()
                    .unwrap_or_default()
        })
        .map(|row| row.canonical_id.clone())
        .collect();

    // assert
    assert!(
        mismatches.is_empty(),
        "profile description override drift for tools: {:?}",
        mismatches
    );
}

fn active_profiles_by_tool(report: &serde_json::Value) -> BTreeMap<String, Vec<String>> {
    let mut active_profiles = BTreeMap::<String, BTreeSet<String>>::new();
    let routes = resolved_routes_details(report)
        .get("routes")
        .and_then(serde_json::Value::as_object)
        .expect("resolved routes object");

    for (profile_name, route) in routes {
        let toolset = route
            .get("toolset")
            .and_then(serde_json::Value::as_array)
            .expect("route toolset array");
        for tool in toolset.iter().filter_map(serde_json::Value::as_str) {
            active_profiles
                .entry(tool.to_string())
                .or_default()
                .insert(profile_name.clone());
        }
    }

    active_profiles
        .into_iter()
        .map(|(tool, profiles)| (tool, profiles.into_iter().collect()))
        .collect()
}

fn profile_description_overrides_by_tool(
    report: &serde_json::Value,
) -> BTreeMap<String, Vec<String>> {
    native_catalog_details(report)
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .expect("doctor native tool array")
        .iter()
        .map(|tool| {
            let canonical_id = tool
                .get("canonical_id")
                .and_then(serde_json::Value::as_str)
                .expect("tool canonical id")
                .to_string();
            let profiles = tool
                .get("profile_description_overrides")
                .and_then(serde_json::Value::as_array)
                .expect("tool profile description overrides")
                .iter()
                .map(|profile| {
                    profile
                        .as_str()
                        .expect("profile description override string")
                        .to_string()
                })
                .collect::<Vec<_>>();
            (canonical_id, profiles)
        })
        .collect()
}

fn native_catalog_details(report: &serde_json::Value) -> &serde_json::Value {
    doctor_check_details(report, "native_tool_catalog")
}

fn resolved_routes_details(report: &serde_json::Value) -> &serde_json::Value {
    doctor_check_details(report, "resolved_routes")
}

fn doctor_check_details<'a>(
    report: &'a serde_json::Value,
    check_name: &str,
) -> &'a serde_json::Value {
    report
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .expect("doctor report checks array")
        .iter()
        .find(|check| check.get("name").and_then(serde_json::Value::as_str) == Some(check_name))
        .and_then(|check| check.get("details"))
        .unwrap_or_else(|| panic!("{check_name} details"))
}

fn doctor_report() -> serde_json::Value {
    ensure_doctor_artifact();
    let doctor = std::fs::read_to_string(super::repo_path(
        "target/baseline-tools-parity/P0.1/doctor.json",
    ))
    .expect("read P0.1 doctor artifact");
    serde_json::from_str(&doctor).expect("parse doctor JSON")
}

fn ensure_doctor_artifact() {
    let artifact = super::repo_path("target/baseline-tools-parity/P0.1/doctor.json");
    assert!(
        artifact.exists(),
        "generate the doctor artifact first: cargo run -p harness -- --config harness.jsonc doctor --json > {artifact}",
        artifact = artifact.display()
    );
}
