use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::process::{CapturedCheckpoint, ProcessCapture};
use super::renderer_command::{self, RendererInvocation};
use super::types::{BrowserCapabilities, CheckpointReceipt, RendererConfig, RunnerTiming};
use crate::parity::semantic_frame_from_vt100_screen;
use crate::tui_fidelity::{AdapterKind, CheckpointName};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RendererMetadata {
    browser_capture: String,
    dimensions: RendererDimensions,
    capabilities: BrowserCapabilities,
    renderer_binding: RendererBinding,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RendererDimensions {
    cols: u16,
    rows: u16,
    font_family: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RendererBinding {
    node: String,
    xterm: String,
    unicode11: String,
    node_pty: String,
    pngjs: String,
    puppeteer_core: String,
}

impl RendererBinding {
    fn is_pinned(&self) -> bool {
        self.node.starts_with('v')
            && self.xterm == "6.0.0"
            && self.unicode11 == "0.9.0"
            && self.node_pty == "1.1.0"
            && self.pngjs == "7.0.0"
            && self.puppeteer_core == "24.43.1"
    }
}

pub(super) struct RenderContext<'a> {
    pub config: &'a RendererConfig,
    pub timing: RunnerTiming,
    pub evidence_root: &'a Path,
    pub tracker: &'a mut CleanupTracker,
}

pub(super) fn render(
    adapter: AdapterKind,
    capture: &ProcessCapture,
    context: &mut RenderContext<'_>,
) -> Result<Vec<CheckpointReceipt>, RunnerError> {
    let mut receipts = Vec::with_capacity(capture.checkpoints.len());
    for checkpoint in &capture.checkpoints {
        receipts.push(render_checkpoint(adapter, checkpoint, context)?);
    }
    Ok(receipts)
}

fn render_checkpoint(
    adapter: AdapterKind,
    checkpoint: &CapturedCheckpoint,
    context: &mut RenderContext<'_>,
) -> Result<CheckpointReceipt, RunnerError> {
    let root = context
        .evidence_root
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
    renderer_command::invoke(
        RendererInvocation {
            config: context.config,
            timing: context.timing,
            checkpoint,
            input_path: &input_path,
            root: &root,
        },
        context.tracker,
    )?;
    let metadata = read_metadata(adapter, checkpoint.name, &root.join("metadata.json"))?;
    validate_metadata(checkpoint, context.config, &metadata)?;
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
    .map(|name| renderer_command::artifact_digest(&root.join(name), context.tracker))
    .collect::<Result<Vec<_>, _>>()?;
    Ok(CheckpointReceipt {
        name: checkpoint.name,
        viewport: checkpoint.viewport,
        captured_at_millis: checkpoint.elapsed.as_millis(),
        capabilities: metadata.capabilities,
        artifacts,
    })
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
        && !metadata.capabilities.browser.is_empty()
        && metadata.renderer_binding.is_pinned();
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

#[cfg(test)]
mod tests {
    use super::RendererBinding;

    #[test]
    fn renderer_binding_rejects_dependency_version_drift() {
        // arrange
        let pinned = RendererBinding {
            node: "v24.0.0".to_owned(),
            xterm: "6.0.0".to_owned(),
            unicode11: "0.9.0".to_owned(),
            node_pty: "1.1.0".to_owned(),
            pngjs: "7.0.0".to_owned(),
            puppeteer_core: "24.43.1".to_owned(),
        };
        let mut drifted = RendererBinding {
            node: pinned.node.clone(),
            xterm: pinned.xterm.clone(),
            unicode11: pinned.unicode11.clone(),
            node_pty: pinned.node_pty.clone(),
            pngjs: pinned.pngjs.clone(),
            puppeteer_core: pinned.puppeteer_core.clone(),
        };
        drifted.xterm = "6.0.1".to_owned();

        // act
        let verdicts = (pinned.is_pinned(), drifted.is_pinned());

        // assert
        assert_eq!(verdicts, (true, false));
    }
}
