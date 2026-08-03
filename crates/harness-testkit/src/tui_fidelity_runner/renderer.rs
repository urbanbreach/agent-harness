use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

use super::error::RunnerError;
use super::process::{CapturedCheckpoint, ProcessCapture};
use super::types::{ArtifactDigest, BrowserCapabilities, CheckpointReceipt, RendererConfig};
use super::util::sha256_file;
use crate::parity::semantic_frame_from_vt100_screen;
use crate::tui_fidelity::{AdapterKind, CheckpointName};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RendererMetadata {
    browser_capture: String,
    dimensions: RendererDimensions,
    capabilities: BrowserCapabilities,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RendererDimensions {
    cols: u16,
    rows: u16,
    font_family: String,
}

pub(super) fn render(
    adapter: AdapterKind,
    capture: &ProcessCapture,
    config: &RendererConfig,
    evidence_root: &Path,
) -> Result<Vec<CheckpointReceipt>, RunnerError> {
    capture
        .checkpoints
        .iter()
        .map(|checkpoint| render_checkpoint(adapter, checkpoint, config, evidence_root))
        .collect()
}

fn render_checkpoint(
    adapter: AdapterKind,
    checkpoint: &CapturedCheckpoint,
    config: &RendererConfig,
    evidence_root: &Path,
) -> Result<CheckpointReceipt, RunnerError> {
    let root = evidence_root
        .join(adapter_name(adapter))
        .join(checkpoint_name(checkpoint.name));
    fs::create_dir_all(&root).map_err(|error| RunnerError::Io {
        path: root.clone(),
        detail: error.to_string(),
    })?;
    let input_path = root.join("input.ansi");
    fs::write(&input_path, &checkpoint.stream).map_err(|error| RunnerError::Io {
        path: input_path.clone(),
        detail: error.to_string(),
    })?;
    invoke_renderer(config, checkpoint, &input_path, &root)?;
    let metadata = read_metadata(adapter, checkpoint.name, &root.join("metadata.json"))?;
    validate_metadata(checkpoint, config, &metadata)?;
    validate_png(adapter, checkpoint.name, &root.join("terminal.png"))?;

    let mut parser = vt100::Parser::new(checkpoint.viewport.rows, checkpoint.viewport.cols, 0);
    parser.process(&checkpoint.stream);
    let frame = semantic_frame_from_vt100_screen(parser.screen());
    frame
        .write_cells_json(&root.join("cells.json"))
        .map_err(|error| RunnerError::Renderer {
            checkpoint: checkpoint.name,
            detail: error.to_string(),
        })?;
    frame
        .write_cells_txt(&root.join("cells.txt"))
        .map_err(|error| RunnerError::Renderer {
            checkpoint: checkpoint.name,
            detail: error.to_string(),
        })?;

    let artifacts = [
        "terminal.png",
        "terminal.txt",
        "terminal-ansi.txt",
        "cells.json",
        "cells.txt",
        "metadata.json",
    ]
    .iter()
    .map(|name| artifact_digest(&root.join(name)))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(CheckpointReceipt {
        name: checkpoint.name,
        viewport: checkpoint.viewport,
        captured_at_millis: checkpoint.elapsed.as_millis(),
        capabilities: metadata.capabilities,
        artifacts,
    })
}

fn invoke_renderer(
    config: &RendererConfig,
    checkpoint: &CapturedCheckpoint,
    input_path: &Path,
    root: &Path,
) -> Result<(), RunnerError> {
    let mut command = Command::new(&config.node_program);
    command
        .arg(&config.script)
        .args(["--title", "TUI fidelity checkpoint", "--from-file"])
        .arg(input_path)
        .arg("--evidence-dir")
        .arg(root)
        .args(["--cols", &checkpoint.viewport.cols.to_string()])
        .args(["--rows", &checkpoint.viewport.rows.to_string()])
        .arg("--chrome-bin")
        .arg(&config.browser_program)
        .args(["--font-family", &config.font_family]);
    if let Some(node_modules) = &config.node_modules {
        command.env("NODE_PATH", node_modules);
        command.env("TUI_FIDELITY_NODE_MODULES", node_modules);
    }
    let output = command.output().map_err(|error| RunnerError::Renderer {
        checkpoint: checkpoint.name,
        detail: error.to_string(),
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RunnerError::Renderer {
            checkpoint: checkpoint.name,
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

fn read_metadata(
    adapter: AdapterKind,
    checkpoint: CheckpointName,
    path: &Path,
) -> Result<RendererMetadata, RunnerError> {
    let bytes = fs::read(path).map_err(|_| RunnerError::MissingCheckpoint {
        adapter,
        checkpoint,
        path: path.to_path_buf(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| RunnerError::InvalidRendererMetadata {
        checkpoint,
        detail: error.to_string(),
    })
}

fn validate_metadata(
    checkpoint: &CapturedCheckpoint,
    config: &RendererConfig,
    metadata: &RendererMetadata,
) -> Result<(), RunnerError> {
    let valid = metadata.browser_capture == "captured"
        && metadata.dimensions.cols == checkpoint.viewport.cols
        && metadata.dimensions.rows == checkpoint.viewport.rows
        && metadata.dimensions.font_family == config.font_family
        && metadata.capabilities.unicode_version == "11"
        && metadata.capabilities.font_loaded
        && metadata.capabilities.device_pixel_ratio > 0.0
        && !metadata.capabilities.browser.is_empty();
    if valid {
        Ok(())
    } else {
        Err(RunnerError::InvalidRendererMetadata {
            checkpoint: checkpoint.name,
            detail: "required browser/font/Unicode11/DPR capability is absent".to_owned(),
        })
    }
}

fn validate_png(
    adapter: AdapterKind,
    checkpoint: CheckpointName,
    path: &Path,
) -> Result<(), RunnerError> {
    let bytes = fs::read(path).map_err(|_| RunnerError::MissingCheckpoint {
        adapter,
        checkpoint,
        path: path.to_path_buf(),
    })?;
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(())
    } else {
        Err(RunnerError::InvalidRendererMetadata {
            checkpoint,
            detail: "checkpoint PNG signature is invalid".to_owned(),
        })
    }
}

fn artifact_digest(path: &Path) -> Result<ArtifactDigest, RunnerError> {
    Ok(ArtifactDigest {
        path: path.display().to_string(),
        sha256: sha256_file(path)?,
    })
}

pub(super) const fn adapter_name(adapter: AdapterKind) -> &'static str {
    match adapter {
        AdapterKind::Grok => "grok",
        AdapterKind::Harness => "harness",
    }
}

const fn checkpoint_name(checkpoint: CheckpointName) -> &'static str {
    match checkpoint {
        CheckpointName::Rest => "rest",
        CheckpointName::Mid => "mid",
        CheckpointName::Settled => "settled",
    }
}
