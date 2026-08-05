use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

use crate::tui_fidelity_matrix::{CoverageManifest, RequirementInventory};
use crate::tui_fidelity_obligation::ObligationType;

const SCHEMA: &str = "harness.tui-fidelity.dependency-cones.v1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyCones {
    schema_version: String,
    cones: Vec<DependencyCone>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyCone {
    paths: Vec<String>,
    obligations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConeSelection {
    pub requirement_ids: BTreeSet<String>,
    pub fell_back_to_all: bool,
    pub unknown_paths: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DependencyConeError {
    Invalid(String),
    Json(String),
    Git(String),
}

impl fmt::Display for DependencyConeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "dependency cone: {detail}"),
            Self::Json(detail) => write!(formatter, "dependency cone JSON: {detail}"),
            Self::Git(detail) => write!(formatter, "Git changes: {detail}"),
        }
    }
}

impl std::error::Error for DependencyConeError {}

impl DependencyCones {
    pub fn from_json(input: &str) -> Result<Self, DependencyConeError> {
        let cones: Self = serde_json::from_str(input)
            .map_err(|error| DependencyConeError::Json(error.to_string()))?;
        if cones.schema_version != SCHEMA || cones.cones.is_empty() {
            return Err(DependencyConeError::Invalid(
                "unsupported schema or empty cone list".to_owned(),
            ));
        }
        for cone in &cones.cones {
            if cone.paths.is_empty()
                || cone.obligations.is_empty()
                || cone.paths.iter().any(|path| path.trim().is_empty())
                || cone
                    .obligations
                    .iter()
                    .any(|selector| selector.trim().is_empty())
            {
                return Err(DependencyConeError::Invalid(
                    "cone paths and obligations must be non-empty".to_owned(),
                ));
            }
        }
        Ok(cones)
    }

    pub fn select(
        &self,
        paths: &[PathBuf],
        inventory: &RequirementInventory,
        manifest: &CoverageManifest,
    ) -> Result<ConeSelection, DependencyConeError> {
        let all = inventory
            .requirements
            .iter()
            .map(|requirement| requirement.id.clone())
            .collect::<BTreeSet<_>>();
        let mut selected = BTreeSet::new();
        let mut unknown = Vec::new();
        for path in paths {
            let normalized = normalize(path)?;
            let matching = self.most_specific(&normalized);
            if matching.is_empty() {
                unknown.push(normalized);
                continue;
            }
            for cone in matching {
                for selector in &cone.obligations {
                    selected.extend(resolve_selector(selector, inventory, manifest)?);
                }
            }
        }
        if unknown.is_empty() {
            Ok(ConeSelection {
                requirement_ids: selected,
                fell_back_to_all: false,
                unknown_paths: Vec::new(),
            })
        } else {
            unknown.sort();
            unknown.dedup();
            Ok(ConeSelection {
                requirement_ids: all,
                fell_back_to_all: true,
                unknown_paths: unknown,
            })
        }
    }

    fn most_specific(&self, path: &str) -> Vec<&DependencyCone> {
        let mut matches = BTreeMap::<usize, Vec<&DependencyCone>>::new();
        for cone in &self.cones {
            if let Some(length) = cone
                .paths
                .iter()
                .filter(|pattern| path_matches(pattern, path))
                .map(|pattern| pattern.trim_end_matches("/**").len())
                .max()
            {
                matches.entry(length).or_default().push(cone);
            }
        }
        matches.pop_last().map_or_else(Vec::new, |(_, cones)| cones)
    }
}

pub fn parse_git_changes(
    tracked: &[u8],
    untracked: &[u8],
) -> Result<Vec<String>, DependencyConeError> {
    let mut paths = BTreeSet::new();
    let fields = nul_fields(tracked)?;
    let mut index = 0;
    while index < fields.len() {
        let status = fields[index];
        index += 1;
        if let Some((status, path)) = status.split_once('\t') {
            paths.insert(path.to_owned());
            if status.starts_with('R') || status.starts_with('C') {
                let next = fields.get(index).ok_or_else(|| {
                    DependencyConeError::Git("rename destination is missing".to_owned())
                })?;
                paths.insert((*next).to_owned());
                index += 1;
            }
            continue;
        }
        let path = fields.get(index).ok_or_else(|| {
            DependencyConeError::Git(format!("path is missing after status {status}"))
        })?;
        paths.insert((*path).to_owned());
        index += 1;
        if status.starts_with('R') || status.starts_with('C') {
            let destination = fields.get(index).ok_or_else(|| {
                DependencyConeError::Git("rename destination is missing".to_owned())
            })?;
            paths.insert((*destination).to_owned());
            index += 1;
        }
    }
    paths.extend(nul_fields(untracked)?.into_iter().map(str::to_owned));
    Ok(paths.into_iter().collect())
}

fn resolve_selector(
    selector: &str,
    inventory: &RequirementInventory,
    manifest: &CoverageManifest,
) -> Result<BTreeSet<String>, DependencyConeError> {
    if selector == "*" {
        return Ok(inventory
            .requirements
            .iter()
            .map(|requirement| requirement.id.clone())
            .collect());
    }
    if let Some(prefix) = selector.strip_prefix("prefix:") {
        return Ok(inventory
            .requirements
            .iter()
            .filter(|requirement| requirement.id.starts_with(prefix))
            .map(|requirement| requirement.id.clone())
            .collect());
    }
    if let Some(scenario) = selector.strip_prefix("scenario:") {
        return Ok(manifest
            .rows
            .iter()
            .filter(|row| row.scenario_id.starts_with(scenario))
            .map(|row| row.requirement_id.clone())
            .collect());
    }
    if let Some(kind) = selector.strip_prefix("type:") {
        let expected: ObligationType = serde_json::from_str(&format!("\"{kind}\""))
            .map_err(|_| DependencyConeError::Invalid(format!("unknown selector {selector}")))?;
        return Ok(inventory
            .requirements
            .iter()
            .filter(|requirement| requirement.obligation.obligation_type() == expected)
            .map(|requirement| requirement.id.clone())
            .collect());
    }
    if inventory
        .requirements
        .iter()
        .any(|requirement| requirement.id == selector)
    {
        Ok(BTreeSet::from([selector.to_owned()]))
    } else {
        Err(DependencyConeError::Invalid(format!(
            "selector {selector} matches no requirement"
        )))
    }
}

fn path_matches(pattern: &str, path: &str) -> bool {
    pattern
        .strip_suffix("/**")
        .map_or(pattern == path, |prefix| {
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}

fn normalize(path: &std::path::Path) -> Result<String, DependencyConeError> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') || normalized.split('/').any(|part| part == "..") {
        Err(DependencyConeError::Invalid(format!(
            "changed path {normalized} is not repository-relative"
        )))
    } else {
        Ok(normalized.trim_start_matches("./").to_owned())
    }
}

fn nul_fields(bytes: &[u8]) -> Result<Vec<&str>, DependencyConeError> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| {
            std::str::from_utf8(field)
                .map_err(|error| DependencyConeError::Git(format!("non-UTF-8 path: {error}")))
        })
        .collect()
}
