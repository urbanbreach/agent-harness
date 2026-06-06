use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use super::text::ParsedTextEdit;
use super::{plural, RenameResourceOperationKind};
use crate::lsp_support::{format_diagnostics, LspRenameResponse};

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct RenamePreview {
    file_count: usize,
    text_edit_count: usize,
    files: Vec<RenameFilePreview>,
    resource_operations: Vec<RenameResourceOperationPreview>,
    annotations: Vec<RenameAnnotationPreview>,
}

impl RenamePreview {
    pub(super) fn from_accumulators(
        preview_files: BTreeMap<String, PreviewFileAccumulator>,
        resource_operations: Vec<RenameResourceOperationPreview>,
        workspace_edit: &Value,
    ) -> Self {
        let files = preview_files
            .into_iter()
            .map(|(path, accumulator)| RenameFilePreview {
                path,
                edit_count: accumulator.edit_count,
                annotation_ids: accumulator.annotation_ids.into_iter().collect(),
            })
            .collect::<Vec<_>>();
        let text_edit_count = files.iter().map(|file| file.edit_count).sum();

        Self {
            file_count: files.len(),
            text_edit_count,
            files,
            resource_operations,
            annotations: RenameAnnotationPreview::from_workspace_edit(workspace_edit),
        }
    }

    fn append_summary_lines(&self, lines: &mut Vec<String>, apply: bool) {
        if self.file_count == 0 && self.resource_operations.is_empty() {
            lines.push("No workspace changes were returned.".to_string());
            return;
        }

        lines.push(format!(
            "{} text edit{} across {} file{}",
            self.text_edit_count,
            plural(self.text_edit_count),
            self.file_count,
            plural(self.file_count),
        ));
        for file in &self.files {
            lines.push(file.display_line());
        }
        for operation in &self.resource_operations {
            lines.push(operation.display_line());
        }
        if !apply {
            lines.push("Re-run with `apply: true` to execute these edits.".to_string());
        }
    }

    pub(super) fn display_text(
        &self,
        new_name: &str,
        apply: bool,
        symbol_preview: Option<&str>,
        response: &LspRenameResponse,
    ) -> String {
        let current = symbol_preview.unwrap_or("<symbol>");
        let mut lines = vec![if apply {
            format!("Applied LSP rename `{current}` → `{new_name}`")
        } else {
            format!("Prepared LSP rename preview `{current}` → `{new_name}`")
        }];
        lines.push(format!("Server: {}", response.server.name));

        self.append_summary_lines(&mut lines, apply);

        let diagnostics = format_diagnostics(&response.diagnostics);
        if !diagnostics.is_empty() {
            lines.push(String::new());
            lines.push("Diagnostics:".to_string());
            lines.push(diagnostics);
        }

        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize)]
struct RenameFilePreview {
    path: String,
    edit_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotation_ids: Vec<String>,
}

impl RenameFilePreview {
    fn display_line(&self) -> String {
        let annotation_suffix = if self.annotation_ids.is_empty() {
            String::new()
        } else {
            format!(" · annotations: {}", self.annotation_ids.join(", "))
        };
        format!(
            "- {} ({} edit{}){}",
            self.path,
            self.edit_count,
            plural(self.edit_count),
            annotation_suffix
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct RenameResourceOperationPreview {
    kind: RenameResourceOperationKind,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    annotation_id: Option<String>,
}

impl RenameResourceOperationPreview {
    pub(super) fn from_change(
        kind: RenameResourceOperationKind,
        path: &str,
        to_path: Option<&str>,
        change: &Value,
    ) -> Self {
        Self {
            kind,
            path: path.to_string(),
            to_path: to_path.map(str::to_string),
            annotation_id: change
                .get("annotationId")
                .and_then(Value::as_str)
                .map(str::to_string),
        }
    }

    fn display_line(&self) -> String {
        let detail = match &self.to_path {
            Some(to_path) => format!("{} → {}", self.path, to_path),
            None => self.path.clone(),
        };
        format!("- {} {}", self.kind.as_str(), detail)
    }
}

#[derive(Debug, Clone, Serialize)]
struct RenameAnnotationPreview {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    needs_confirmation: bool,
}

impl RenameAnnotationPreview {
    fn from_workspace_edit(workspace_edit: &Value) -> Vec<Self> {
        let Some(annotations) = workspace_edit
            .get("changeAnnotations")
            .and_then(Value::as_object)
        else {
            return Vec::new();
        };

        annotations
            .iter()
            .map(|(id, annotation)| Self::from_change_annotation(id, annotation))
            .collect()
    }

    fn from_change_annotation(id: &str, annotation: &Value) -> Self {
        Self {
            id: id.to_string(),
            label: annotation
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_string),
            description: annotation
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            needs_confirmation: annotation
                .get("needsConfirmation")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }
}

#[derive(Default)]
pub(super) struct PreviewFileAccumulator {
    edit_count: usize,
    annotation_ids: BTreeSet<String>,
}

impl PreviewFileAccumulator {
    pub(super) fn record_text_edits(&mut self, parsed_edits: &[ParsedTextEdit]) {
        self.edit_count += parsed_edits.len();
        self.annotation_ids.extend(
            parsed_edits
                .iter()
                .filter_map(ParsedTextEdit::annotation_id)
                .map(str::to_string),
        );
    }
}
