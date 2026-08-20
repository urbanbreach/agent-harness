use std::fmt;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

const OWNER_TEST: &str = "tui_dependency_audit_test";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DependencyDisposition {
    Retained,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectDependency {
    pub crate_name: String,
    pub requirement: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineGapMapping {
    pub gap: String,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyJustification {
    pub crate_name: String,
    pub version: String,
    pub retained_or_added: DependencyDisposition,
    pub capability_gap: String,
    pub license: String,
    pub offline_status: String,
    pub supply_chain_posture: String,
    pub size_estimate: String,
    pub owner_test: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditReport {
    pub direct_dependencies: Vec<DirectDependency>,
    pub justifications: Vec<DependencyJustification>,
    pub baseline_gap_mappings: Vec<BaselineGapMapping>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuditError {
    CargoMetadataFailed { mode: String, stderr: String },
    MetadataParse { mode: String, message: String },
    MissingPackage { package: String },
    MissingResolvedDependency { dependency: String },
    MissingJustification { dependency: String },
    MissingLicense { dependency: String },
}

impl fmt::Display for AuditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CargoMetadataFailed { mode, stderr } => {
                write!(formatter, "cargo metadata ({mode}) failed: {stderr}")
            }
            Self::MetadataParse { mode, message } => {
                write!(formatter, "cargo metadata ({mode}) was invalid: {message}")
            }
            Self::MissingPackage { package } => write!(formatter, "missing package {package}"),
            Self::MissingResolvedDependency { dependency } => {
                write!(formatter, "missing resolved dependency {dependency}")
            }
            Self::MissingJustification { dependency } => {
                write!(formatter, "missing justification for {dependency}")
            }
            Self::MissingLicense { dependency } => {
                write!(formatter, "missing license for {dependency}")
            }
        }
    }
}

impl std::error::Error for AuditError {}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    dependencies: Vec<MetadataDependency>,
}

#[derive(Debug, Deserialize)]
struct MetadataDependency {
    name: String,
    req: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Debug, Deserialize)]
struct ResolveNode {
    id: String,
    deps: Vec<ResolveDependency>,
}

#[derive(Debug, Deserialize)]
struct ResolveDependency {
    name: String,
    pkg: String,
}

pub fn audit_workspace() -> Result<AuditReport, AuditError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest_metadata = run_metadata(&root, true)?;
    let resolved_metadata = run_metadata(&root, false)?;
    let tui = manifest_metadata
        .packages
        .iter()
        .find(|package| package.name == "harness-tui")
        .ok_or_else(|| AuditError::MissingPackage {
            package: "harness-tui".to_owned(),
        })?;
    let node = resolved_metadata
        .resolve
        .as_ref()
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == tui.id))
        .ok_or_else(|| AuditError::MissingPackage {
            package: "harness-tui resolve node".to_owned(),
        })?;

    let mut direct_dependencies = Vec::with_capacity(tui.dependencies.len());
    let mut justifications = Vec::with_capacity(tui.dependencies.len());
    for dependency in &tui.dependencies {
        let kind = dependency.kind.as_deref().unwrap_or("normal").to_owned();
        let resolved = node
            .deps
            .iter()
            .find(|resolved| {
                resolved.name == dependency.name
                    || resolved.name == dependency.name.replace('-', "_")
            })
            .ok_or_else(|| AuditError::MissingResolvedDependency {
                dependency: dependency.name.clone(),
            })?;
        let package = resolved_metadata
            .packages
            .iter()
            .find(|package| package.id == resolved.pkg)
            .ok_or_else(|| AuditError::MissingPackage {
                package: resolved.pkg.clone(),
            })?;
        let (capability_gap, size_estimate) =
            rationale(&dependency.name).ok_or_else(|| AuditError::MissingJustification {
                dependency: dependency.name.clone(),
            })?;
        let license = package
            .license
            .clone()
            .ok_or_else(|| AuditError::MissingLicense {
                dependency: dependency.name.clone(),
            })?;
        direct_dependencies.push(DirectDependency {
            crate_name: dependency.name.clone(),
            requirement: dependency.req.clone(),
            kind,
        });
        justifications.push(DependencyJustification {
            crate_name: dependency.name.clone(),
            version: package.version.clone(),
            retained_or_added: DependencyDisposition::Retained,
            capability_gap: capability_gap.to_owned(),
            license,
            offline_status: "metadata and lock verified offline".to_owned(),
            supply_chain_posture: supply_chain(package),
            size_estimate: size_estimate.to_owned(),
            owner_test: OWNER_TEST.to_owned(),
        });
    }
    Ok(AuditReport {
        direct_dependencies,
        justifications,
        baseline_gap_mappings: gap_mappings(),
    })
}

fn run_metadata(root: &Path, no_deps: bool) -> Result<Metadata, AuditError> {
    let mode = if no_deps { "--no-deps" } else { "resolved" };
    let mut command = Command::new("cargo");
    command.args(["metadata", "--format-version", "1", "--locked", "--offline"]);
    if no_deps {
        command.arg("--no-deps");
    }
    let output =
        command
            .current_dir(root)
            .output()
            .map_err(|error| AuditError::CargoMetadataFailed {
                mode: mode.to_owned(),
                stderr: error.to_string(),
            })?;
    if !output.status.success() {
        return Err(AuditError::CargoMetadataFailed {
            mode: mode.to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|error| AuditError::MetadataParse {
        mode: mode.to_owned(),
        message: error.to_string(),
    })
}

fn supply_chain(package: &Package) -> String {
    if package.id.contains("registry+") {
        "crates.io registry source, Cargo.lock checksum, no git dependency".to_owned()
    } else {
        "workspace path source, owned and reviewed in this repository".to_owned()
    }
}

fn rationale(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "anyhow" => ("lifecycle error propagation", "small"),
        "crossterm" => ("interaction/lifecycle/viewport events", "medium"),
        "crossbeam-channel" => ("runtime input, live-update, and frame delivery", "small"),
        "fuzzy-matcher" => ("surface command/dashboard navigation", "small"),
        "harness-core" => ("lifecycle state/render transitions", "workspace"),
        "imara-diff" => ("visual-behavior diff visualization", "medium"),
        "ratatui" => ("viewport/surface composition", "medium"),
        "serde" => ("lifecycle/surface state serialization", "small"),
        "serde_json" => ("visual-behavior evidence/report shape", "small"),
        "syntect" => ("surface code/media styling", "large"),
        "unicode-width" => ("viewport CJK width/glyph placement", "small"),
        "font8x8" => ("surface font/media fixtures", "small"),
        "fontdue" => ("surface font/media fixtures", "medium"),
        "harness-providers" => ("lifecycle question/completion fixtures", "workspace"),
        "harness-testkit" => ("visual-behavior owner/baseline evidence", "workspace"),
        "image" => ("visual-behavior pixel evidence", "medium"),
        "insta" => ("surface/visual-behavior snapshots", "medium"),
        "portable-pty" => ("viewport terminal lifecycle capture", "medium"),
        "sha2" => ("visual-behavior artifact integrity", "small"),
        "tempfile" => ("lifecycle cleanup/evidence isolation", "small"),
        "thiserror" => ("typed runtime and presentation errors", "small"),
        "vt100" => ("interaction/viewport terminal emulation", "medium"),
        _ => return None,
    })
}

fn gap_mappings() -> Vec<BaselineGapMapping> {
    [
        (
            "viewport-scale",
            "large/max layout, resize, and settling behavior",
        ),
        ("interaction", "mouse capture, decoding, and hit routing"),
        (
            "surface",
            "media/dashboard/modal composition, clipping, and layering",
        ),
        (
            "lifecycle",
            "question/cancel/recover/complete state transitions",
        ),
        (
            "visual-behavior",
            "geometry/glyph/style/cursor parity comparison",
        ),
    ]
    .into_iter()
    .map(|(gap, capability)| BaselineGapMapping {
        gap: gap.to_owned(),
        capability: capability.to_owned(),
    })
    .collect()
}
