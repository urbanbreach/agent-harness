use crate::files::edit::{check_read, commit, existing_contents};
use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) struct PatchTool;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(rename = "patchText", alias = "patch_text", alias = "patch")]
    text: String,
}
struct Patch {
    path: PathBuf,
    change: Change,
}
enum Change {
    Add(String),
    Delete,
    Update(Vec<Hunk>),
}
#[derive(Default)]
struct Hunk {
    hint: Option<String>,
    old: Vec<String>,
    new: Vec<String>,
    eof: bool,
}
fn invalid(message: &str) -> ToolError {
    ToolError::InvalidArguments(message.into())
}
fn args(value: Value) -> Result<Args, ToolError> {
    serde_json::from_value(value).map_err(|e| invalid(&e.to_string()))
}
#[async_trait::async_trait]
impl Tool for PatchTool {
    fn id(&self) -> &str {
        "apply_patch"
    }
    fn description(&self) -> &str {
        "Apply a *** Begin Patch document with Add, Update, and Delete File sections. Read existing files first. Moves are rejected. Files apply sequentially; errors report completed changes."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::EditFs
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn filesystem_paths(&self, value: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(parse(&args(value.clone())?.text)?
            .into_iter()
            .map(|p| p.path)
            .collect())
    }
    fn permission_requests(&self, value: &Value) -> Vec<(String, String)> {
        self.filesystem_paths(value)
            .unwrap_or_default()
            .into_iter()
            .map(|p| ("edit".into(), p.to_string_lossy().into_owned()))
            .collect()
    }
    async fn call(&self, ctx: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let patches = parse(&args(value)?.text)?;
        tokio::task::spawn_blocking(move || apply(&ctx, patches))
            .await
            .map_err(|_| ToolError::Execution("patch operation stopped unexpectedly".into()))?
    }
}
fn parse(text: &str) -> Result<Vec<Patch>, ToolError> {
    let lines: Vec<_> = text.lines().collect();
    if lines.first() != Some(&"*** Begin Patch") || lines.last() != Some(&"*** End Patch") {
        return Err(invalid(
            "patch must start with *** Begin Patch and end with *** End Patch",
        ));
    }
    let mut patches = Vec::new();
    let mut index = 1;
    while index + 1 < lines.len() {
        let (kind, name) = ["Add", "Update", "Delete"]
            .into_iter()
            .find_map(|kind| {
                lines[index]
                    .strip_prefix(&format!("*** {kind} File: "))
                    .map(|name| (kind, name))
            })
            .ok_or_else(|| {
                invalid("expected Add, Update, or Delete File; moves are not supported")
            })?;
        if name.trim().is_empty() || patches.len() == 256 {
            return Err(invalid(
                "patch path is empty or the patch exceeds 256 files",
            ));
        }
        index += 1;
        let change = match kind {
            "Add" => {
                let mut text = String::new();
                while index + 1 < lines.len() && !lines[index].starts_with("*** ") {
                    text.push_str(
                        lines[index]
                            .strip_prefix('+')
                            .ok_or_else(|| invalid("added lines must start with +"))?,
                    );
                    text.push('\n');
                    index += 1;
                }
                Change::Add(text)
            }
            "Delete" => Change::Delete,
            _ => {
                let mut hunks = Vec::new();
                while index + 1 < lines.len() && !lines[index].starts_with("*** ") {
                    hunks.push(parse_hunk(&lines, &mut index)?);
                }
                if hunks.is_empty() {
                    return Err(invalid("updated files require at least one hunk"));
                }
                Change::Update(hunks)
            }
        };
        patches.push(Patch {
            path: name.into(),
            change,
        });
    }
    if patches.is_empty() {
        return Err(invalid("patch contains no file changes"));
    }
    Ok(patches)
}
fn apply(ctx: &ToolContext, patches: Vec<Patch>) -> Result<ToolResult, ToolError> {
    let mut files = Vec::new();
    let mut artifacts = Vec::new();
    let mut display = String::new();
    let mut failure = None;
    for patch in patches {
        let result = (|| {
            let path = ctx.resolve_workspace_path(&patch.path)?;
            ctx.tool_state.edit(&path, |previous| {
                let before = existing_contents(&path)?;
                let after = match patch.change {
                    Change::Add(text) if before.is_none() => Some(text),
                    Change::Add(_) => return Err(invalid("Add File target already exists")),
                    Change::Delete | Change::Update(_) if before.is_none() => {
                        return Err(invalid("patch target does not exist"))
                    }
                    Change::Delete => {
                        check_read(before.as_deref().unwrap_or(""), previous)?;
                        None
                    }
                    Change::Update(hunks) => {
                        let text = before.as_deref().unwrap_or("");
                        check_read(text, previous)?;
                        Some(update(text, hunks)?)
                    }
                };
                commit(ctx, &path, &patch.path, before.as_deref(), after)
            })
        })();
        match result {
            Ok(result) => {
                display.push_str(&result.display_text);
                display.push('\n');
                files.push(result.structured_json.unwrap_or(Value::Null));
                artifacts.extend(result.artifacts);
            }
            Err(error) => {
                failure = Some(error.to_string());
                break;
            }
        }
    }
    if let Some(error) = &failure {
        display.push_str(&format!(
            "Patch stopped: {error}. Completed changes above remain applied."
        ));
    }
    Ok(ToolResult::structured_with_artifacts(
        display,
        json!({"files":files, "is_error":failure.is_some(), "error":failure}),
        artifacts,
    ))
}
fn update(source: &str, hunks: Vec<Hunk>) -> Result<String, ToolError> {
    let source = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .replace("\r\n", "\n");
    let mut changes = Vec::new();
    let mut cursor = 0;
    for hunk in hunks {
        if let Some(hint) = hunk.hint {
            let offset = source[cursor..]
                .split_inclusive('\n')
                .scan(cursor, |offset, line| {
                    let start = *offset;
                    *offset += line.len();
                    Some((start, line.trim()))
                })
                .find(|(_, line)| *line == hint)
                .map(|(offset, _)| offset)
                .ok_or_else(|| invalid("hunk header context was not found"))?;
            cursor = offset;
        }
        let old = hunk.old.join("\n");
        let mut matches = source[cursor..]
            .match_indices(&old)
            .map(|(index, _)| cursor + index)
            .filter(|&start| {
                let end = start + old.len();
                (start == 0 || source[..start].ends_with('\n'))
                    && (end == source.len() || source[end..].starts_with('\n'))
                    && (!hunk.eof || end == source.len() || &source[end..] == "\n")
            });
        let start = if old.is_empty() {
            source.len()
        } else {
            let start = matches
                .next()
                .ok_or_else(|| invalid("patch context was not found; read the current file"))?;
            if matches.next().is_some() {
                return Err(invalid(
                    "patch context is ambiguous; include more surrounding lines",
                ));
            }
            start
        };
        let mut end = start + old.len();
        let newline = source[end..].starts_with('\n');
        if newline {
            end += 1;
        }
        let mut text = hunk.new.join("\n");
        if !hunk.new.is_empty() {
            if old.is_empty() && start > 0 && !source.ends_with('\n') {
                text.insert(0, '\n');
            }
            if newline || end < source.len() || old.is_empty() {
                text.push('\n');
            }
        }
        changes.push((start, end, text));
        cursor = end;
    }
    let mut text = String::with_capacity(source.len());
    cursor = 0;
    for (start, end, replacement) in changes {
        text.push_str(&source[cursor..start]);
        text.push_str(&replacement);
        cursor = end;
    }
    text.push_str(&source[cursor..]);
    Ok(text)
}

fn parse_hunk(lines: &[&str], index: &mut usize) -> Result<Hunk, ToolError> {
    let hint = lines[*index]
        .strip_prefix("@@")
        .ok_or_else(|| invalid("update hunks must start with @@"))?
        .trim();
    let mut hunk = Hunk {
        hint: (!hint.is_empty() && !hint.starts_with('-')).then(|| hint.to_owned()),
        ..Default::default()
    };
    *index += 1;
    while *index + 1 < lines.len()
        && !lines[*index].starts_with("@@")
        && !lines[*index].starts_with("*** ")
    {
        let line = lines[*index];
        match line.as_bytes().first() {
            Some(b' ') => {
                hunk.old.push(line[1..].into());
                hunk.new.push(line[1..].into());
            }
            Some(b'-') => hunk.old.push(line[1..].into()),
            Some(b'+') => hunk.new.push(line[1..].into()),
            _ => return Err(invalid("hunk lines must start with space, +, or -")),
        }
        *index += 1;
    }
    if lines.get(*index) == Some(&"*** End of File") {
        hunk.eof = true;
        *index += 1;
    }
    if hunk.old.is_empty() && hunk.new.is_empty() {
        return Err(invalid("patch hunk is empty"));
    }
    Ok(hunk)
}
