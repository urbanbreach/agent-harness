use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const AUTHORITY_PATH: &str = "configs/tui-fidelity-reference-authority.json";
pub const ACTIVE_REVISION: &str = "be713136d2a69080743a3f6b3c72077057e5948f";
pub const ACTIVE_BINARY_SHA256: &str =
    "2198bc3699b0ac76d3e3d32bf3da2277479ad244e19cfb1be7d111fc5f4b8ca2";
pub const ACTIVE_VERSION: &str = "grok 1.0.1 (be71313) [alpha]";
const HISTORICAL_REVISION_PREFIX: &str = "c1b5909ec707c069f1d21a93917af044";
const HISTORICAL_REVISION_SUFFIX: &str = "e71da0d7";
const RETIRED_REVISION_PREFIX: &str = "500129c714ad1b10e6095481f4a8387a";
const RETIRED_REVISION_SUFFIX: &str = "2ec52649";
const ACTIVE_SOURCE_PATHS: &[&str] = &[
    "configs/tui-fidelity-requirement-inventory.json",
    "crates/harness-testkit/src/bin/binary_receipt.rs",
    "crates/harness-testkit/src/bin/tui-fidelity.rs",
    "crates/harness-testkit/src/bin/tui_fidelity_commands/verify_executor.rs",
    "crates/harness-testkit/tests/binary_receipt_test.rs",
    "crates/harness-testkit/tests/source_guard_test.rs",
    "scripts/tui-fidelity/source-guard.sh",
];

pub fn authority_defects(root: &Path, authority: &Value) -> Vec<String> {
    let mut defects = Vec::new();
    let historical_revision = format!("{HISTORICAL_REVISION_PREFIX}{HISTORICAL_REVISION_SUFFIX}");
    for (pointer, expected) in [
        (
            "/schema_version",
            "harness.tui-fidelity.reference-authority.v1",
        ),
        ("/status", "active"),
        ("/reference/source_revision", ACTIVE_REVISION),
        ("/reference/binary_sha256", ACTIVE_BINARY_SHA256),
        ("/reference/binary_version", ACTIVE_VERSION),
        (
            "/historical_non_acceptance/source_revision",
            &historical_revision,
        ),
    ] {
        check_field(authority, pointer, expected, &mut defects);
    }
    if authority["historical_non_acceptance"]["acceptance_eligible"].as_bool() != Some(false) {
        defects.push("historical evidence must not be acceptance eligible".to_owned());
    }
    validate_historical_manifest(root, &mut defects);
    validate_active_sources(root, authority, &mut defects);
    validate_required_paths(root, authority, &mut defects);
    validate_scenario_paths(root, authority, &mut defects);
    let retired_revision = format!("{RETIRED_REVISION_PREFIX}{RETIRED_REVISION_SUFFIX}");
    let stale = revision_paths(root, &retired_revision);
    if !stale.is_empty() {
        defects.push(format!("retired active revision remains: {stale:?}"));
    }
    validate_historical_surface(root, authority, &historical_revision, &mut defects);
    defects
}

fn validate_historical_manifest(root: &Path, defects: &mut Vec<String>) {
    let manifest = read_json(&root.join("docs/reference/tui-reference-parity-manifest.v1.json"));
    check_field(
        &manifest,
        "/acceptance_status",
        "historical_non_acceptance",
        defects,
    );
    check_field(&manifest, "/active_authority", AUTHORITY_PATH, defects);
}

fn validate_active_sources(root: &Path, authority: &Value, defects: &mut Vec<String>) {
    let Some(sources) = authority["active_revision_sources"].as_array() else {
        defects.push("active_revision_sources must be an array".to_owned());
        return;
    };
    let declared: BTreeSet<&str> = sources
        .iter()
        .filter_map(|source| source["path"].as_str())
        .collect();
    if declared != ACTIVE_SOURCE_PATHS.iter().copied().collect() {
        defects.push(format!("active revision source paths differ: {declared:?}"));
    }
    for source in sources {
        let Some(path) = source["path"].as_str() else {
            defects.push("active revision source path is missing".to_owned());
            continue;
        };
        if let Some(constant) = source["constant"].as_str() {
            match read_constant(&root.join(path), constant) {
                Ok(observed) if observed == ACTIVE_REVISION => {}
                Ok(observed) => defects.push(format!("{path} disagrees: {observed}")),
                Err(detail) => defects.push(detail),
            }
        } else if source["revision_token"].as_bool() == Some(true) {
            let input = fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
            if !input.contains(ACTIVE_REVISION) {
                defects.push(format!("{path} has no active revision token"));
            }
        } else {
            defects.push(format!("{path} has no revision contract"));
        }
    }
}

fn validate_required_paths(root: &Path, authority: &Value, defects: &mut Vec<String>) {
    for path in strings(authority, "/required_paths", defects) {
        if !root.join(&path).is_file() {
            defects.push(format!("required authority path is absent: {path}"));
        }
    }
}

fn validate_scenario_paths(root: &Path, authority: &Value, defects: &mut Vec<String>) {
    let Some(registry_path) = authority["scenario_registry"].as_str() else {
        defects.push("scenario_registry must be a path".to_owned());
        return;
    };
    let registry = read_json(&root.join(registry_path));
    let Some(scenarios) = registry["scenarios"].as_array() else {
        defects.push("scenario registry scenarios must be an array".to_owned());
        return;
    };
    for scenario in scenarios {
        let Some(path) = scenario["path"].as_str() else {
            defects.push("scenario registry entry has no path".to_owned());
            continue;
        };
        if !root.join("crates/harness-testkit").join(path).is_file() {
            defects.push(format!("required scenario path is absent: {path}"));
        }
    }
}

fn validate_historical_surface(
    root: &Path,
    authority: &Value,
    revision: &str,
    defects: &mut Vec<String>,
) {
    let declared: BTreeSet<String> =
        strings(authority, "/historical_non_acceptance/surfaces", defects)
            .into_iter()
            .collect();
    let observed = revision_paths(root, revision);
    if observed != declared {
        defects.push(format!(
            "historical surface differs\ndeclared: {declared:?}\nobserved: {observed:?}"
        ));
    }
}

fn revision_paths(root: &Path, revision: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for top in ["scripts", "configs", "docs", "crates"] {
        collect_revision_paths(root, &root.join(top), revision, &mut paths);
    }
    paths
}

fn collect_revision_paths(
    root: &Path,
    directory: &Path,
    revision: &str,
    paths: &mut BTreeSet<String>,
) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_revision_paths(root, &path, revision, paths);
        } else if fs::read(&path).is_ok_and(|bytes| {
            bytes
                .windows(revision.len())
                .any(|window| window == revision.as_bytes())
        }) {
            paths.insert(
                path.strip_prefix(root)
                    .expect("first-party path")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
}

fn read_constant(path: &Path, constant: &str) -> Result<String, String> {
    let input = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let declaration = input
        .lines()
        .find(|line| line.contains(constant))
        .ok_or_else(|| format!("{} has no constant {constant}", path.display()))?;
    let start = declaration
        .find('"')
        .ok_or_else(|| format!("{} constant is not quoted", path.display()))?
        + 1;
    let end = declaration[start..]
        .find('"')
        .ok_or_else(|| format!("{} constant quote is not closed", path.display()))?
        + start;
    Ok(declaration[start..end].to_owned())
}

fn strings(authority: &Value, pointer: &str, defects: &mut Vec<String>) -> Vec<String> {
    let Some(values) = authority.pointer(pointer).and_then(Value::as_array) else {
        defects.push(format!("{pointer} must be an array"));
        return Vec::new();
    };
    values
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

pub fn check_field(value: &Value, pointer: &str, expected: &str, defects: &mut Vec<String>) {
    let observed = value.pointer(pointer).and_then(Value::as_str);
    if observed != Some(expected) {
        defects.push(format!(
            "{pointer} expected {expected}, observed {observed:?}"
        ));
    }
}

pub fn read_json(path: &Path) -> Value {
    let input = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&input)
        .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
