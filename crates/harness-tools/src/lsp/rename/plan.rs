use super::*;
use crate::files::edit::{commit_with_metadata, existing_contents};
use std::{fs, path::Path};

#[derive(Deserialize)]
#[serde(untagged)]
enum Change {
    Text {
        #[serde(rename = "textDocument")]
        document: Document,
        edits: Vec<text::Edit>,
    },
    Resource(Resource),
}
#[derive(Deserialize)]
struct Document {
    uri: String,
    version: Option<i32>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    #[serde(default)]
    overwrite: bool,
    #[serde(default)]
    ignore_if_exists: bool,
    #[serde(default)]
    ignore_if_not_exists: bool,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Resource {
    Create {
        uri: String,
        #[serde(default)]
        options: Options,
    },
    Rename {
        #[serde(rename = "oldUri")]
        from: String,
        #[serde(rename = "newUri")]
        to: String,
        #[serde(default)]
        options: Options,
    },
    Delete {
        uri: String,
        #[serde(default)]
        options: Options,
    },
}
impl Change {
    fn uris(&self) -> Vec<&str> {
        match self {
            Self::Text { document, .. } => vec![&document.uri],
            Self::Resource(Resource::Create { uri, .. } | Resource::Delete { uri, .. }) => {
                vec![uri]
            }
            Self::Resource(Resource::Rename { from, to, .. }) => vec![from, to],
        }
    }
}
fn changes(value: &Value) -> Result<Vec<Change>, ToolError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let result = if let Some(changes) = value.get("documentChanges") {
        serde_json::from_value(changes.clone())
            .map_err(|e| failure(&format!("invalid rename changes: {e}")))?
    } else if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        changes
            .iter()
            .map(|(uri, edits)| {
                Ok(Change::Text {
                    document: Document {
                        uri: uri.clone(),
                        version: None,
                    },
                    edits: serde_json::from_value(edits.clone())
                        .map_err(|_| failure("invalid rename text edits"))?,
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?
    } else if value.as_object().is_some_and(|v| v.is_empty()) {
        Vec::new()
    } else {
        return Err(failure("rename response has no workspace changes"));
    };
    if result.len() > 256 {
        return Err(failure("rename exceeds 256 operations"));
    }
    Ok(result)
}
fn path(uri: &str) -> Result<PathBuf, ToolError> {
    let url = reqwest::Url::parse(uri).map_err(|_| failure("invalid rename URI"))?;
    if url.query().is_some() || url.fragment().is_some() {
        return Err(failure("rename URI contains query or fragment"));
    }
    url.to_file_path()
        .map_err(|()| failure("rename requires local file URIs"))
}
struct File {
    before: Option<String>,
    after: Option<String>,
    permissions: Option<fs::Permissions>,
    before_permissions: Option<fs::Permissions>,
}
fn get<'a>(
    files: &'a mut BTreeMap<PathBuf, File>,
    paths: &BTreeMap<String, PathBuf>,
    uri: &str,
) -> Result<&'a mut File, ToolError> {
    paths
        .get(uri)
        .and_then(|path| files.get_mut(path))
        .ok_or_else(|| failure("rename target is unavailable"))
}
fn occupied(file: &File, options: &Options) -> Result<bool, ToolError> {
    if file.after.is_none() || options.overwrite {
        Ok(false)
    } else if options.ignore_if_exists {
        Ok(true)
    } else {
        Err(failure("rename target already exists"))
    }
}
fn simulate(
    files: &mut BTreeMap<PathBuf, File>,
    paths: &BTreeMap<String, PathBuf>,
    changes: Vec<Change>,
    response: &query::Response,
) -> Result<(), ToolError> {
    for change in changes {
        match change {
            Change::Text { document, edits } => {
                if document
                    .version
                    .is_some_and(|version| response.versions.get(&document.uri) != Some(&version))
                {
                    return Err(failure("rename document version is stale or unknown"));
                }
                let file = get(files, paths, &document.uri)?;
                let source = file
                    .after
                    .as_deref()
                    .ok_or_else(|| failure("rename edits a missing file"))?;
                file.after = Some(text::apply(source, edits)?);
            }
            Change::Resource(Resource::Create { uri, options }) => {
                let file = get(files, paths, &uri)?;
                if !occupied(file, &options)? {
                    file.after = Some(String::new());
                }
            }
            Change::Resource(Resource::Delete { uri, options }) => {
                let file = get(files, paths, &uri)?;
                if file.after.is_none() && !options.ignore_if_not_exists {
                    return Err(failure("rename deletes a missing file"));
                }
                file.after = None;
            }
            Change::Resource(Resource::Rename { from, to, options }) => {
                if paths.get(&from) == paths.get(&to) {
                    continue;
                }
                if get(files, paths, &from)?.after.is_none() {
                    return Err(failure("rename source is missing"));
                }
                if occupied(get(files, paths, &to)?, &options)? {
                    continue;
                }
                let source = get(files, paths, &from)?;
                let text = source.after.take();
                let permissions = source.permissions.clone();
                let target = get(files, paths, &to)?;
                target.after = text;
                target.permissions = permissions;
            }
        }
        if files
            .values()
            .map(|file| file.after.as_ref().map_or(0, String::len))
            .sum::<usize>()
            > 32 * 1024 * 1024
        {
            return Err(failure("rename contents exceed 32 MiB"));
        }
    }
    Ok(())
}
pub(super) fn execute(
    ctx: &ToolContext,
    primary: &Path,
    response: &query::Response,
    apply: bool,
) -> Result<ToolResult, ToolError> {
    let changes = changes(&response.data["result"]["workspaceEdit"])?;
    let mut paths = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    for uri in changes.iter().flat_map(Change::uris) {
        let input = path(uri)?;
        let resolved = ctx.resolve_workspace_path(&input)?;
        if !resolved.starts_with(&ctx.workspace_root) {
            return Err(failure("rename target is outside the workspace"));
        }
        paths.insert(uri.to_owned(), resolved.clone());
        inputs.insert(input, resolved);
    }
    if inputs.len() > 256 {
        return Err(failure("rename exceeds 256 files"));
    }
    let executor = tokio::runtime::Handle::current();
    let reads = inputs
        .keys()
        .filter(|p| p.as_path() != primary)
        .cloned()
        .collect();
    executor
        .block_on(ctx.coordinator.authorize_tool_paths(
            ctx.tool_call_id.to_string(),
            reads,
            &["read", "lsp"],
        ))
        .map_err(|e| failure(&e.to_string()))?;
    let mut files = BTreeMap::new();
    let mut size = 0;
    for (input, resolved) in &inputs {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if ctx.resolve_workspace_path(input)? != *resolved {
            return Err(failure("rename target changed during approval"));
        }
        let before = existing_contents(resolved)?;
        size += before.as_ref().map_or(0, String::len);
        if size > 32 * 1024 * 1024 {
            return Err(failure("rename sources exceed 32 MiB"));
        }
        if resolved == primary
            && before.as_ref().map(|text| blake3::hash(text.as_bytes()))
                != Some(response.primary_digest)
        {
            return Err(failure(
                "rename source changed while the server was computing edits",
            ));
        }
        let permissions = if before.is_some() {
            Some(fs::metadata(resolved)?.permissions())
        } else {
            None
        };
        files.insert(
            resolved.clone(),
            File {
                after: before.clone(),
                before,
                before_permissions: permissions.clone(),
                permissions,
            },
        );
    }
    simulate(&mut files, &paths, changes, response)?;
    if apply {
        executor
            .block_on(ctx.coordinator.authorize_tool_edit_paths(
                ctx.tool_call_id.to_string(),
                inputs.keys().cloned().collect(),
            ))
            .map_err(|e| failure(&e.to_string()))?;
        for (input, resolved) in &inputs {
            if ctx.resolve_workspace_path(input)? != *resolved
                || existing_contents(resolved)? != files[resolved].before
            {
                return Err(failure("rename target changed before applying edits"));
            }
        }
    }
    let mut artifacts = Vec::new();
    let mut details = Vec::new();
    let mut diff = String::new();
    let mut changed: Vec<_> = files
        .into_iter()
        .filter(|(_, file)| {
            file.before != file.after || file.before_permissions != file.permissions
        })
        .collect();
    // Publish retained files before removing sources; each commit has its own undo receipt.
    changed.sort_by_key(|(_, file)| file.after.is_none());
    for (path, file) in changed {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        let display = path.strip_prefix(&ctx.workspace_root).unwrap_or(&path);
        if apply {
            let result = ctx.tool_state.edit(&path, |_| {
                commit_with_metadata(
                    ctx,
                    &path,
                    display,
                    file.before.as_deref(),
                    file.after,
                    file.permissions,
                    false,
                )
            })?;
            details.push(json!({"path":display,"result":result.structured_json,"message":result.display_text}));
            artifacts.extend(result.artifacts);
        } else {
            let patch = similar::TextDiff::configure()
                .timeout(std::time::Duration::from_millis(200))
                .diff_lines(
                    file.before.as_deref().unwrap_or(""),
                    file.after.as_deref().unwrap_or(""),
                )
                .unified_diff()
                .header(
                    &format!("a/{}", display.display()),
                    &format!("b/{}", display.display()),
                )
                .to_string();
            if diff.len() + patch.len() > 1024 * 1024 {
                return Err(failure("rename preview exceeds 1 MiB"));
            }
            diff.push_str(&patch);
            details.push(json!({"path":display}));
        }
    }
    if !apply && !diff.is_empty() {
        artifacts.push(
            executor
                .block_on(
                    ctx.coordinator
                        .retain_tool_diff(ctx.tool_call_id.to_string(), diff),
                )
                .map_err(|e| failure(&e.to_string()))?,
        );
    }
    Ok(ToolResult::structured_with_artifacts(
        format!(
            "{} rename across {} files.",
            if apply { "Applied" } else { "Previewed" },
            details.len()
        ),
        json!({"operation":"renameSymbol","applied":apply && !details.is_empty(),"files":details}),
        artifacts,
    ))
}
