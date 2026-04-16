use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::Duration;

use harness_core::config::{registered_lsp_config, LspConfig, LspServerConfig};
use harness_core::tool::ToolError;
use serde::Serialize;
use serde_json::{json, Value};
use walkdir::{DirEntry, WalkDir};

const DEFAULT_LSP_BOOT_DELAY_MS: u64 = 150;
const DEFAULT_LSP_RETRY_ATTEMPTS: usize = 8;

const SUPPORTED_LSP_OPERATION_NAMES: &[&str] = &[
    "goToDefinition",
    "findReferences",
    "hover",
    "documentSymbol",
    "workspaceSymbol",
    "goToImplementation",
    "prepareCallHierarchy",
    "incomingCalls",
    "outgoingCalls",
    "fileDiagnostics",
    "workspaceDiagnostics",
];
const POSITION_LSP_OPERATION_NAMES: &[&str] = &[
    "goToDefinition",
    "findReferences",
    "hover",
    "goToImplementation",
    "prepareCallHierarchy",
    "incomingCalls",
    "outgoingCalls",
];
const FILE_LSP_OPERATION_NAMES: &[&str] =
    &["documentSymbol", "fileDiagnostics", "workspaceDiagnostics"];
const QUERY_LSP_OPERATION_NAMES: &[&str] = &["workspaceSymbol"];
const WORKSPACE_DIAGNOSTICS_SKIPPED_DIR_NAMES: &[&str] = &[".git", "target", "node_modules"];

const RUST_ROOT_MARKERS: &[&str] = &["Cargo.toml", "rust-project.json"];
const TYPESCRIPT_ROOT_MARKERS: &[&str] = &[
    "tsconfig.json",
    "jsconfig.json",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
];
const PYTHON_ROOT_MARKERS: &[&str] = &[
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "requirements.txt",
    "requirements-dev.txt",
    "Pipfile",
    "poetry.lock",
    "uv.lock",
    "uv.toml",
];
const GO_ROOT_MARKERS: &[&str] = &["go.work", "go.mod"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LspOperation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    FileDiagnostics,
    WorkspaceDiagnostics,
}

impl LspOperation {
    pub(crate) fn parse(value: &str) -> Result<Self, ToolError> {
        match value {
            "goToDefinition" => Ok(Self::GoToDefinition),
            "findReferences" => Ok(Self::FindReferences),
            "hover" => Ok(Self::Hover),
            "documentSymbol" => Ok(Self::DocumentSymbol),
            "workspaceSymbol" => Ok(Self::WorkspaceSymbol),
            "goToImplementation" => Ok(Self::GoToImplementation),
            "prepareCallHierarchy" => Ok(Self::PrepareCallHierarchy),
            "incomingCalls" => Ok(Self::IncomingCalls),
            "outgoingCalls" => Ok(Self::OutgoingCalls),
            "fileDiagnostics" => Ok(Self::FileDiagnostics),
            "workspaceDiagnostics" => Ok(Self::WorkspaceDiagnostics),
            "prepareRename" | "renameSymbol" => Err(ToolError::InvalidArguments(format!(
                "unsupported lsp operation: {value}; use lsp.rename for the explicit write-capable rename flow; supported operations: {}",
                SUPPORTED_LSP_OPERATION_NAMES.join(", ")
            ))),
            _ => Err(ToolError::InvalidArguments(format!(
                "unsupported lsp operation: {value}; supported operations: {}",
                SUPPORTED_LSP_OPERATION_NAMES.join(", ")
            ))),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::GoToDefinition => "goToDefinition",
            Self::FindReferences => "findReferences",
            Self::Hover => "hover",
            Self::DocumentSymbol => "documentSymbol",
            Self::WorkspaceSymbol => "workspaceSymbol",
            Self::GoToImplementation => "goToImplementation",
            Self::PrepareCallHierarchy => "prepareCallHierarchy",
            Self::IncomingCalls => "incomingCalls",
            Self::OutgoingCalls => "outgoingCalls",
            Self::FileDiagnostics => "fileDiagnostics",
            Self::WorkspaceDiagnostics => "workspaceDiagnostics",
        }
    }

    pub(crate) fn input_kind(self) -> LspOperationInputKind {
        match self {
            Self::GoToDefinition
            | Self::FindReferences
            | Self::Hover
            | Self::GoToImplementation
            | Self::PrepareCallHierarchy
            | Self::IncomingCalls
            | Self::OutgoingCalls => LspOperationInputKind::Position,
            Self::DocumentSymbol | Self::FileDiagnostics | Self::WorkspaceDiagnostics => {
                LspOperationInputKind::File
            }
            Self::WorkspaceSymbol => LspOperationInputKind::Query,
        }
    }

    pub(crate) fn supported_names_for(kind: LspOperationInputKind) -> &'static [&'static str] {
        match kind {
            LspOperationInputKind::Position => POSITION_LSP_OPERATION_NAMES,
            LspOperationInputKind::File => FILE_LSP_OPERATION_NAMES,
            LspOperationInputKind::Query => QUERY_LSP_OPERATION_NAMES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LspOperationInputKind {
    Position,
    File,
    Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LspPosition {
    line: u32,
    character: u32,
}

impl LspPosition {
    pub(crate) fn from_one_based(line: i32, character: i32) -> Result<Self, ToolError> {
        if line < 1 || character < 1 {
            return Err(ToolError::InvalidArguments(
                "line and character must be >= 1".to_string(),
            ));
        }

        Ok(Self {
            line: (line as u32) - 1,
            character: (character as u32) - 1,
        })
    }

    fn line(self) -> u32 {
        self.line
    }

    fn character(self) -> u32 {
        self.character
    }
}

pub(crate) struct LspOperationRequest<'a> {
    pub(crate) operation: LspOperation,
    pub(crate) input: LspOperationInput<'a>,
    pub(crate) workspace_root: &'a Path,
}

pub(crate) struct LspRenameRequest<'a> {
    pub(crate) file_path: &'a Path,
    pub(crate) position: LspPosition,
    pub(crate) workspace_root: &'a Path,
    pub(crate) new_name: &'a str,
}

pub(crate) enum LspOperationInput<'a> {
    Position {
        file_path: &'a Path,
        position: LspPosition,
    },
    File {
        file_path: &'a Path,
    },
    Query {
        file_path: &'a Path,
        query: &'a str,
    },
}

impl LspOperationInput<'_> {
    fn file_path(&self) -> &Path {
        match self {
            Self::Position { file_path, .. }
            | Self::File { file_path }
            | Self::Query { file_path, .. } => file_path,
        }
    }

    fn position(&self) -> Option<LspPosition> {
        match self {
            Self::Position { position, .. } => Some(*position),
            Self::File { .. } | Self::Query { .. } => None,
        }
    }

    fn query(&self) -> Option<&str> {
        match self {
            Self::Query { query, .. } => Some(*query),
            Self::Position { .. } | Self::File { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LspServerMetadata {
    pub(crate) name: String,
    pub(crate) command: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LspDiagnosticReport {
    pub(crate) file_path: String,
    pub(crate) diagnostics: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LspOperationResponse {
    pub(crate) server: LspServerMetadata,
    pub(crate) result: Value,
    pub(crate) diagnostics: Vec<LspDiagnosticReport>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LspRenameResponse {
    pub(crate) server: LspServerMetadata,
    pub(crate) prepare_result: Value,
    pub(crate) workspace_edit: Value,
    pub(crate) diagnostics: Vec<LspDiagnosticReport>,
}

pub(crate) fn execute_lsp_operation(
    request: &LspOperationRequest<'_>,
) -> Result<LspOperationResponse, ToolError> {
    let file_path = request
        .input
        .file_path()
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve file path: {err}")))?;
    let cfg = registered_lsp_config();
    let spec = server_for_path(&file_path, &cfg)?;
    let root = project_root(&file_path, request.workspace_root, spec.root_markers);
    let mut session = LspSession::start(&spec, &root)?;
    let server = LspServerMetadata {
        name: spec.name.clone(),
        command: spec.command.clone(),
    };

    match request.operation {
        LspOperation::FileDiagnostics => {
            session.open_file(&file_path, spec.name.as_str())?;
            refresh_diagnostics_after_open(
                &mut session,
                "textDocument/diagnostic",
                json!({
                    "textDocument": { "uri": path_to_uri(&file_path) },
                }),
            )?;
            let diagnostics = vec![session.diagnostics_for(&file_path)];
            return Ok(LspOperationResponse {
                server,
                result: file_diagnostics_result(&file_path, &diagnostics),
                diagnostics,
            });
        }
        LspOperation::WorkspaceDiagnostics => {
            let opened_files = open_workspace_files_for_diagnostics(&mut session, &root, &spec)?;
            refresh_diagnostics_after_open(
                &mut session,
                "workspace/diagnostic",
                json!({
                    "previousResultIds": [],
                }),
            )?;
            let diagnostics = session.diagnostics();
            return Ok(LspOperationResponse {
                server,
                result: workspace_diagnostics_result(&root, opened_files, &diagnostics),
                diagnostics,
            });
        }
        _ => session.open_file(&file_path, spec.name.as_str())?,
    }

    let result = match request.operation {
        LspOperation::GoToDefinition => request_with_retry(
            &mut session,
            "textDocument/definition",
            position_request_params(request, &file_path)?,
        ),
        LspOperation::FindReferences => {
            let position = position_request_params(request, &file_path)?;
            request_with_retry(
                &mut session,
                "textDocument/references",
                json!({
                    "textDocument": position["textDocument"].clone(),
                    "position": position["position"].clone(),
                    "context": { "includeDeclaration": true },
                }),
            )
        }
        LspOperation::Hover => request_with_retry(
            &mut session,
            "textDocument/hover",
            position_request_params(request, &file_path)?,
        ),
        LspOperation::DocumentSymbol => request_with_retry(
            &mut session,
            "textDocument/documentSymbol",
            json!({
                "textDocument": { "uri": path_to_uri(&file_path) },
            }),
        ),
        LspOperation::WorkspaceSymbol => request_with_retry(
            &mut session,
            "workspace/symbol",
            json!({
                "query": request.input.query().ok_or_else(|| ToolError::InvalidArguments(
                    "workspaceSymbol requires a query".to_string(),
                ))?,
            }),
        ),
        LspOperation::GoToImplementation => request_with_retry(
            &mut session,
            "textDocument/implementation",
            position_request_params(request, &file_path)?,
        ),
        LspOperation::PrepareCallHierarchy => request_with_retry(
            &mut session,
            "textDocument/prepareCallHierarchy",
            position_request_params(request, &file_path)?,
        ),
        LspOperation::IncomingCalls => {
            let prepared = request_with_retry(
                &mut session,
                "textDocument/prepareCallHierarchy",
                position_request_params(request, &file_path)?,
            )?;
            let Some(item) = prepared.as_array().and_then(|items| items.first()).cloned() else {
                return Ok(LspOperationResponse {
                    server: server.clone(),
                    result: Value::Array(Vec::new()),
                    diagnostics: session.diagnostics(),
                });
            };
            request_with_retry(
                &mut session,
                "callHierarchy/incomingCalls",
                json!({ "item": item }),
            )
        }
        LspOperation::OutgoingCalls => {
            let prepared = request_with_retry(
                &mut session,
                "textDocument/prepareCallHierarchy",
                position_request_params(request, &file_path)?,
            )?;
            let Some(item) = prepared.as_array().and_then(|items| items.first()).cloned() else {
                return Ok(LspOperationResponse {
                    server: server.clone(),
                    result: Value::Array(Vec::new()),
                    diagnostics: session.diagnostics(),
                });
            };
            request_with_retry(
                &mut session,
                "callHierarchy/outgoingCalls",
                json!({ "item": item }),
            )
        }
        LspOperation::FileDiagnostics | LspOperation::WorkspaceDiagnostics => {
            unreachable!("diagnostics-first operations return before navigation dispatch")
        }
    }?;

    Ok(LspOperationResponse {
        server,
        result,
        diagnostics: session.diagnostics(),
    })
}

fn position_request_params(
    request: &LspOperationRequest<'_>,
    file_path: &Path,
) -> Result<Value, ToolError> {
    let position = request.input.position().ok_or_else(|| {
        ToolError::InvalidArguments(format!(
            "{} requires line and character",
            request.operation.as_str()
        ))
    })?;
    Ok(json!({
        "textDocument": { "uri": path_to_uri(file_path) },
        "position": {
            "line": position.line(),
            "character": position.character(),
        },
    }))
}

fn file_diagnostics_result(file_path: &Path, diagnostics: &[LspDiagnosticReport]) -> Value {
    json!({
        "scope": "file",
        "filePath": file_path.display().to_string(),
        "reports": diagnostics,
        "diagnosticCount": diagnostic_count(diagnostics),
    })
}

fn workspace_diagnostics_result(
    workspace_root: &Path,
    files_scanned: usize,
    diagnostics: &[LspDiagnosticReport],
) -> Value {
    json!({
        "scope": "workspace",
        "workspaceRoot": workspace_root.display().to_string(),
        "filesScanned": files_scanned,
        "reports": diagnostics,
        "diagnosticCount": diagnostic_count(diagnostics),
    })
}

fn diagnostic_count(reports: &[LspDiagnosticReport]) -> usize {
    reports.iter().map(|report| report.diagnostics.len()).sum()
}

fn refresh_diagnostics_after_open(
    session: &mut LspSession,
    method: &str,
    params: Value,
) -> Result<(), ToolError> {
    match request_with_retry(session, method, params) {
        Ok(_) => Ok(()),
        Err(ToolError::Execution(message)) if is_unsupported_diagnostic_request(&message) => Ok(()),
        Err(err) => Err(err),
    }
}

fn is_unsupported_diagnostic_request(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("method not found")
        || normalized.contains("not implemented")
        || normalized.contains("-32601")
}

fn open_workspace_files_for_diagnostics(
    session: &mut LspSession,
    root: &Path,
    spec: &LspServerSpec,
) -> Result<usize, ToolError> {
    let mut files = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_workspace_diagnostics_entry(entry))
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| matches_lsp_extension(path, &spec.extensions))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    files.sort();
    for path in &files {
        session.open_file(path, spec.name.as_str())?;
    }
    Ok(files.len())
}

pub(crate) fn execute_lsp_rename(
    request: &LspRenameRequest<'_>,
) -> Result<LspRenameResponse, ToolError> {
    let StartedLspSession {
        file_path,
        spec,
        mut session,
    } = start_lsp_session(request.file_path, request.workspace_root)?;

    let position = json!({
        "textDocument": { "uri": path_to_uri(&file_path) },
        "position": {
            "line": request.position.line(),
            "character": request.position.character(),
        },
    });

    let prepare_result =
        request_with_retry(&mut session, "textDocument/prepareRename", position.clone())?;
    if prepare_result.is_null() {
        return Err(ToolError::Execution(
            "language server reported rename is unavailable at the requested position".to_string(),
        ));
    }

    let workspace_edit = request_with_retry(
        &mut session,
        "textDocument/rename",
        json!({
            "textDocument": position["textDocument"].clone(),
            "position": position["position"].clone(),
            "newName": request.new_name,
        }),
    )?;

    Ok(LspRenameResponse {
        server: LspServerMetadata {
            name: spec.name,
            command: spec.command,
        },
        prepare_result,
        workspace_edit,
        diagnostics: session.diagnostics(),
    })
}

struct StartedLspSession {
    file_path: PathBuf,
    spec: LspServerSpec,
    session: LspSession,
}

fn start_lsp_session(
    file_path: &Path,
    workspace_root: &Path,
) -> Result<StartedLspSession, ToolError> {
    let file_path = file_path
        .canonicalize()
        .map_err(|err| ToolError::Execution(format!("failed to resolve file path: {err}")))?;
    let cfg = registered_lsp_config();
    let spec = server_for_path(&file_path, &cfg)?;
    let root = project_root(&file_path, workspace_root, spec.root_markers);
    let mut session = LspSession::start(&spec, &root)?;
    session.open_file(&file_path, spec.name.as_str())?;
    Ok(StartedLspSession {
        file_path,
        spec,
        session,
    })
}

fn should_skip_workspace_diagnostics_entry(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    entry.file_type().is_dir()
        && entry
            .file_name()
            .to_str()
            .is_some_and(|name| WORKSPACE_DIAGNOSTICS_SKIPPED_DIR_NAMES.contains(&name))
}

fn matches_lsp_extension(path: &Path, supported_extensions: &[String]) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };
    let normalized = format!(".{}", extension.to_ascii_lowercase());
    supported_extensions
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(&normalized))
}

#[derive(Debug, Clone)]
struct LspServerSpec {
    name: String,
    disabled: bool,
    command: Vec<String>,
    extensions: Vec<String>,
    root_markers: &'static [&'static str],
    env: BTreeMap<String, String>,
    initialization: Option<Value>,
}

impl LspServerSpec {
    fn builtin(
        name: &str,
        command: &[&str],
        extensions: &[&str],
        root_markers: &'static [&'static str],
    ) -> Self {
        Self {
            name: name.to_string(),
            disabled: false,
            command: command.iter().map(|token| (*token).to_string()).collect(),
            extensions: extensions
                .iter()
                .map(|extension| (*extension).to_string())
                .collect(),
            root_markers,
            env: BTreeMap::new(),
            initialization: None,
        }
    }

    fn rust() -> Self {
        Self::builtin("rust", &["rust-analyzer"], &[".rs"], RUST_ROOT_MARKERS)
    }

    fn typescript() -> Self {
        Self::builtin(
            "typescript",
            &["typescript-language-server", "--stdio"],
            &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts"],
            TYPESCRIPT_ROOT_MARKERS,
        )
    }

    fn python() -> Self {
        Self::builtin(
            "python",
            &["pyright-langserver", "--stdio"],
            &[".py", ".pyi"],
            PYTHON_ROOT_MARKERS,
        )
    }

    fn go() -> Self {
        Self::builtin("go", &["gopls"], &[".go"], GO_ROOT_MARKERS)
    }

    fn json() -> Self {
        Self::builtin(
            "json",
            &["vscode-json-language-server", "--stdio"],
            &[".json", ".jsonc"],
            &[],
        )
    }

    fn yaml() -> Self {
        Self::builtin(
            "yaml",
            &["yaml-language-server", "--stdio"],
            &[".yaml", ".yml"],
            &[],
        )
    }

    fn custom(name: &str, cfg: &LspServerConfig) -> Result<Self, ToolError> {
        let command = cfg.command.clone().ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "lsp.servers.`{name}` must provide `command` for custom local servers"
            ))
        })?;
        let extensions = cfg.extensions.clone().ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "lsp.servers.`{name}` must provide `extensions` for custom local servers"
            ))
        })?;
        let spec = Self {
            name: name.to_string(),
            disabled: cfg.disabled,
            command,
            extensions,
            root_markers: &[],
            env: cfg.env.clone(),
            initialization: cfg.initialization.clone(),
        };
        spec.validate_runtime()?;
        Ok(spec)
    }

    fn apply_override(&mut self, cfg: &LspServerConfig) {
        self.disabled = cfg.disabled;
        if let Some(command) = &cfg.command {
            self.command = command.clone();
        }
        if let Some(extensions) = &cfg.extensions {
            self.extensions = extensions.clone();
        }
        if !cfg.env.is_empty() {
            self.env = cfg.env.clone();
        }
        if let Some(initialization) = &cfg.initialization {
            self.initialization = Some(initialization.clone());
        }
    }

    fn validate_runtime(&self) -> Result<(), ToolError> {
        if self.command.is_empty() {
            return Err(ToolError::InvalidArguments(format!(
                "configured lsp server `{}` has no command",
                self.name
            )));
        }
        if self.extensions.is_empty() {
            return Err(ToolError::InvalidArguments(format!(
                "configured lsp server `{}` has no extensions",
                self.name
            )));
        }
        Ok(())
    }
}

fn default_server_specs() -> BTreeMap<String, LspServerSpec> {
    BTreeMap::from([
        ("go".to_string(), LspServerSpec::go()),
        ("json".to_string(), LspServerSpec::json()),
        ("python".to_string(), LspServerSpec::python()),
        ("rust".to_string(), LspServerSpec::rust()),
        ("typescript".to_string(), LspServerSpec::typescript()),
        ("yaml".to_string(), LspServerSpec::yaml()),
    ])
}

fn resolved_server_specs(cfg: &LspConfig) -> Result<BTreeMap<String, LspServerSpec>, ToolError> {
    let mut specs = default_server_specs();
    for (name, server) in &cfg.servers {
        if let Some(spec) = specs.get_mut(name) {
            spec.apply_override(server);
            spec.validate_runtime()?;
            continue;
        }
        specs.insert(name.clone(), LspServerSpec::custom(name, server)?);
    }
    Ok(specs)
}

fn server_for_path(path: &Path, cfg: &LspConfig) -> Result<LspServerSpec, ToolError> {
    if cfg.disabled {
        return Err(ToolError::InvalidArguments(
            "lsp is disabled by config".to_string(),
        ));
    }

    let extension = normalized_extension(path).ok_or_else(|| unsupported_language_error(None))?;
    let specs = resolved_server_specs(cfg)?;

    if let Some(spec) = specs.values().find(|spec| {
        !spec.disabled
            && spec
                .extensions
                .iter()
                .any(|candidate| candidate == &extension)
    }) {
        return Ok(spec.clone());
    }

    let disabled = specs
        .values()
        .filter(|spec| {
            spec.disabled
                && spec
                    .extensions
                    .iter()
                    .any(|candidate| candidate == &extension)
        })
        .map(|spec| spec.name.as_str())
        .collect::<Vec<_>>();
    if !disabled.is_empty() {
        return Err(disabled_server_error(&extension, &disabled));
    }

    Err(unsupported_language_error_with_specs(
        Some(&extension),
        specs.values(),
    ))
}

fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|extension| format!(".{}", extension.to_ascii_lowercase()))
}

fn unsupported_language_error(extension: Option<&str>) -> ToolError {
    let specs = default_server_specs();
    unsupported_language_error_with_specs(extension, specs.values())
}

fn unsupported_language_error_with_specs<'a>(
    extension: Option<&str>,
    specs: impl IntoIterator<Item = &'a LspServerSpec>,
) -> ToolError {
    let mut supported = specs
        .into_iter()
        .filter(|spec| !spec.disabled)
        .flat_map(|spec| spec.extensions.iter().cloned())
        .collect::<Vec<_>>();
    supported.sort();
    supported.dedup();

    ToolError::InvalidArguments(format!(
        "unsupported lsp language extension: {}; supported extensions: {}",
        extension.unwrap_or("<none>"),
        if supported.is_empty() {
            "<none>".to_string()
        } else {
            supported.join(", ")
        }
    ))
}

fn disabled_server_error(extension: &str, servers: &[&str]) -> ToolError {
    let names = servers
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let verb = if servers.len() == 1 { "is" } else { "are" };
    ToolError::InvalidArguments(format!(
        "configured lsp server {names} {verb} disabled for extension {extension}"
    ))
}

fn project_root(file_path: &Path, workspace_root: &Path, markers: &[&str]) -> PathBuf {
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let mut current = file_path.parent();
    while let Some(dir) = current {
        if dir.starts_with(&workspace_root)
            && markers.iter().any(|marker| dir.join(marker).exists())
        {
            return dir.to_path_buf();
        }
        if dir == workspace_root {
            break;
        }
        current = dir.parent();
    }
    workspace_root
}

fn path_to_uri(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    reqwest::Url::from_file_path(&canonical)
        .expect("valid file url")
        .to_string()
}

fn uri_to_workspace_path(uri: &str, root: &Path) -> Option<PathBuf> {
    let url = reqwest::Url::parse(uri).ok()?;
    let path = url.to_file_path().ok()?;
    path.starts_with(root).then_some(path)
}

fn language_id(path: &Path, server_name: &str) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("rs") => "rust".to_string(),
        Some("ts") => "typescript".to_string(),
        Some("tsx") => "typescriptreact".to_string(),
        Some("py") | Some("pyi") => "python".to_string(),
        Some("go") => "go".to_string(),
        Some("json") | Some("jsonc") => "json".to_string(),
        Some("yaml") | Some("yml") => "yaml".to_string(),
        Some("js") => "javascript".to_string(),
        Some("jsx") => "javascriptreact".to_string(),
        Some("mjs") | Some("cjs") | Some("mts") | Some("cts") => "javascript".to_string(),
        Some(other) => other.to_string(),
        None => server_name.to_string(),
    }
}

struct LspSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    root: PathBuf,
    diagnostics: BTreeMap<String, Vec<Value>>,
}

impl LspSession {
    fn start(spec: &LspServerSpec, root: &Path) -> Result<Self, ToolError> {
        let mut command = Command::new(&spec.command[0]);
        command
            .args(spec.command.iter().skip(1))
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if !spec.env.is_empty() {
            command.envs(spec.env.iter());
        }

        let mut child = command.spawn().map_err(|err| {
            ToolError::Execution(format!("failed to start language server: {err}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ToolError::Execution("language server stdin unavailable".to_string()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ToolError::Execution("language server stdout unavailable".to_string())
        })?;

        let mut session = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            root: root.to_path_buf(),
            diagnostics: BTreeMap::new(),
        };

        let mut params = json!({
            "processId": std::process::id(),
            "rootUri": path_to_uri(root),
            "workspaceFolders": [{
                "name": "workspace",
                "uri": path_to_uri(root),
            }],
            "capabilities": {
                "window": { "workDoneProgress": true },
                "workspace": {
                    "configuration": true,
                    "workspaceFolders": true,
                    "workspaceEdit": {
                        "documentChanges": true,
                        "resourceOperations": ["create", "rename", "delete"],
                    },
                },
                "textDocument": {
                    "publishDiagnostics": {
                        "relatedInformation": true,
                    },
                    "rename": {
                        "prepareSupport": true,
                    },
                    "synchronization": {
                        "didOpen": true,
                        "didChange": true,
                    }
                }
            }
        });
        if let Some(initialization) = &spec.initialization {
            params["initializationOptions"] = initialization.clone();
        }

        let initialize_id = session.next_request_id();
        let initialize_result = session.request_raw(initialize_id, "initialize", params)?;
        if initialize_result.is_null() {
            return Err(ToolError::Execution(
                "language server failed to initialize".to_string(),
            ));
        }

        session.notify("initialized", json!({}))?;
        thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
        Ok(session)
    }

    fn open_file(&mut self, file_path: &Path, server_name: &str) -> Result<(), ToolError> {
        let text = fs::read_to_string(file_path)
            .map_err(|err| ToolError::Execution(format!("failed to read source file: {err}")))?;
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": path_to_uri(file_path),
                    "languageId": language_id(file_path, server_name),
                    "version": 0,
                    "text": text,
                }
            }),
        )?;
        thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, ToolError> {
        let id = self.next_request_id();
        self.request_raw(id, method, params)
    }

    fn request_raw(&mut self, id: u64, method: &str, params: Value) -> Result<Value, ToolError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        loop {
            let message = self.read_message()?;
            if let Some(response_id) = message.get("id").and_then(Value::as_u64) {
                if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                    self.respond_to_server_request(response_id, server_method, &message)?;
                    continue;
                }
                if response_id != id {
                    continue;
                }
                if let Some(error) = message.get("error") {
                    return Err(ToolError::Execution(format!(
                        "language server request failed: {}",
                        error_message(error)
                    )));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }

            if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                self.handle_server_notification(server_method, &message);
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), ToolError> {
        self.write_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn next_request_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_message(&mut self, message: Value) -> Result<(), ToolError> {
        let body = serde_json::to_vec(&message)
            .map_err(|err| ToolError::Execution(format!("failed to encode lsp request: {err}")))?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())
            .map_err(|err| ToolError::Execution(format!("failed to write lsp header: {err}")))?;
        self.stdin
            .write_all(&body)
            .map_err(|err| ToolError::Execution(format!("failed to write lsp body: {err}")))?;
        self.stdin
            .flush()
            .map_err(|err| ToolError::Execution(format!("failed to flush lsp request: {err}")))
    }

    fn read_message(&mut self) -> Result<Value, ToolError> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .map_err(|err| ToolError::Execution(format!("failed to read lsp header: {err}")))?;
            if read == 0 {
                return Err(ToolError::Execution(
                    "language server closed the connection".to_string(),
                ));
            }
            if line == "\r\n" {
                break;
            }
            let lowercase = line.to_ascii_lowercase();
            if let Some(value) = lowercase.strip_prefix("content-length:") {
                let parsed = value.trim().parse::<usize>().map_err(|err| {
                    ToolError::Execution(format!("invalid lsp content length: {err}"))
                })?;
                content_length = Some(parsed);
            }
        }

        let length = content_length.ok_or_else(|| {
            ToolError::Execution("language server response missing content length".to_string())
        })?;
        let mut body = vec![0_u8; length];
        self.stdout
            .read_exact(&mut body)
            .map_err(|err| ToolError::Execution(format!("failed to read lsp body: {err}")))?;
        serde_json::from_slice(&body)
            .map_err(|err| ToolError::Execution(format!("failed to decode lsp message: {err}")))
    }

    fn respond_to_server_request(
        &mut self,
        id: u64,
        method: &str,
        _message: &Value,
    ) -> Result<(), ToolError> {
        let result = match method {
            "window/workDoneProgress/create"
            | "client/registerCapability"
            | "client/unregisterCapability" => Value::Null,
            "workspace/configuration" => json!([{}]),
            "workspace/workspaceFolders" => json!([{
                "name": "workspace",
                "uri": path_to_uri(&self.root),
            }]),
            _ => Value::Null,
        };
        self.write_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    fn handle_server_notification(&mut self, method: &str, message: &Value) {
        if method != "textDocument/publishDiagnostics" {
            return;
        }

        let Some(params) = message.get("params") else {
            return;
        };
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return;
        };
        let Some(path) = uri_to_workspace_path(uri, &self.root) else {
            return;
        };
        let diagnostics = params
            .get("diagnostics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        self.diagnostics
            .insert(path.display().to_string(), diagnostics);
    }

    fn diagnostics(&self) -> Vec<LspDiagnosticReport> {
        self.diagnostics
            .iter()
            .map(|(file_path, diagnostics)| LspDiagnosticReport {
                file_path: file_path.clone(),
                diagnostics: diagnostics.clone(),
            })
            .collect()
    }

    fn diagnostics_for(&self, file_path: &Path) -> LspDiagnosticReport {
        let canonical = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf())
            .display()
            .to_string();
        LspDiagnosticReport {
            file_path: canonical.clone(),
            diagnostics: self
                .diagnostics
                .get(&canonical)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

impl Drop for LspSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn error_message(value: &Value) -> String {
    value
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn request_with_retry(
    session: &mut LspSession,
    method: &str,
    params: Value,
) -> Result<Value, ToolError> {
    for attempt in 0..DEFAULT_LSP_RETRY_ATTEMPTS {
        match session.request(method, params.clone()) {
            Ok(value)
                if !lsp_value_is_empty(&value) || attempt + 1 == DEFAULT_LSP_RETRY_ATTEMPTS =>
            {
                return Ok(value)
            }
            Ok(_) => {
                thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
            }
            Err(ToolError::Execution(message)) if message.contains("content modified") => {
                thread::sleep(Duration::from_millis(DEFAULT_LSP_BOOT_DELAY_MS));
            }
            Err(err) => return Err(err),
        }
    }

    unreachable!("lsp retry loop must return before exhaustion")
}

fn lsp_value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        path_to_uri, server_for_path, LspOperation, LspPosition, SUPPORTED_LSP_OPERATION_NAMES,
    };
    use harness_core::config::LspConfig;
    use harness_core::tool::ToolError;
    use std::fs;

    #[test]
    fn path_to_uri_percent_encodes_spaces() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("space file.rs");
        fs::write(&path, "fn demo() {}\n").expect("write source file");
        let uri = path_to_uri(&path);
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("space%20file.rs"));
    }

    #[test]
    fn lsp_position_translates_one_based_to_zero_based() {
        let position = LspPosition::from_one_based(3, 9).expect("valid one-based position");
        assert_eq!(position.line(), 2);
        assert_eq!(position.character(), 8);
    }

    #[test]
    fn lsp_position_rejects_non_positive_coordinates() {
        let err = LspPosition::from_one_based(0, 1).expect_err("line must be >= 1");
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message == "line and character must be >= 1")
        );
    }

    #[test]
    fn lsp_operation_parse_rejects_unsupported_values_with_stable_message() {
        let err = LspOperation::parse("renameSymbol").expect_err("operation should fail");
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message == format!(
                "unsupported lsp operation: renameSymbol; use lsp.rename for the explicit write-capable rename flow; supported operations: {}",
                SUPPORTED_LSP_OPERATION_NAMES.join(", ")
            ))
        );
    }

    #[test]
    fn server_for_path_rejects_unsupported_extension_with_stable_message() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let path = tempdir.path().join("fixture.lua");
        fs::write(&path, "print('hello')\n").expect("write fixture");
        let err = match server_for_path(&path, &LspConfig::default()) {
            Ok(_) => panic!("lua should be unsupported"),
            Err(err) => err,
        };
        assert!(
            matches!(err, ToolError::InvalidArguments(message) if message.contains("unsupported lsp language extension: .lua"))
        );
    }

    #[test]
    fn lsp_operation_supported_names_match_roundtrip_strings() {
        for operation in SUPPORTED_LSP_OPERATION_NAMES {
            let parsed = LspOperation::parse(operation).expect("operation should parse");
            assert_eq!(parsed.as_str(), *operation);
        }
    }
}
