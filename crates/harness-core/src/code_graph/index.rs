use super::*;
use regex::Regex;
use std::{collections::HashSet, fs, io::Read, path::Component};
const SCHEMA: &str = "harness-code-graph-index-v1";
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SYMBOLS: usize = 32_000;
const MAX_EDGES: usize = 64_000;

pub fn build_persistent_graph_index(
    root: &Path,
    runtime_dir: &Path,
) -> Result<(PathBuf, SimpleGraphIndex), CodeGraphError> {
    let path = runtime_dir.join(GRAPH_INDEX_FILE);
    validate_path(&path)?;
    if !root.is_dir() {
        return Err(CodeGraphError::Invalid("workspace is not a directory"));
    }
    let files = source_files(root)?;
    let definitions = Regex::new(r"^\s*(?:(?:pub(?:\([^)]*\))?|async|const|static|unsafe|export|default)\s+)*(?:fn|struct|enum|trait|type|class|def|interface|function|func)\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)").map_err(|_| CodeGraphError::Invalid("invalid symbol matcher"))?;
    let mut index = SimpleGraphIndex {
        schema: SCHEMA.into(),
        symbols: Vec::new(),
        edges: Vec::new(),
    };
    let mut digests = Vec::with_capacity(files.len());
    for file in &files {
        let text = read_text(file, 1024 * 1024)?;
        digests.push(blake3::hash(text.as_bytes()));
        let relative = relative_path(root, file)?;
        for (row, line) in text.lines().enumerate() {
            let Some(capture) = definitions.captures(line) else {
                continue;
            };
            index.symbols.push(GraphSymbolHit {
                symbol: capture[1].into(),
                path: relative.clone(),
                line: row_number(row)?,
                kind: GraphQueryKind::SymbolDef,
            });
            if index.symbols.len() > MAX_SYMBOLS {
                return Err(CodeGraphError::Invalid("index exceeds 32000 symbols"));
            }
        }
    }
    let known: HashSet<_> = index.symbols.iter().map(|h| h.symbol.as_str()).collect();
    for (file, digest) in files.iter().zip(digests) {
        let text = read_text(file, 1024 * 1024)?;
        if blake3::hash(text.as_bytes()) != digest {
            return Err(CodeGraphError::Invalid("source changed during index build"));
        }
        collect_edges(
            &text,
            &relative_path(root, file)?,
            &definitions,
            &known,
            &mut index.edges,
        )?;
    }
    validate_index(&index)?;
    let bytes =
        serde_json::to_vec(&index).map_err(|_| CodeGraphError::Invalid("cannot encode index"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(CodeGraphError::Invalid("index exceeds 16 MiB"));
    }
    let parent = path
        .parent()
        .ok_or(CodeGraphError::Invalid("index path has no parent"))?;
    crate::store::create_private_dir(parent)?;
    crate::store::write_private_atomic(&path, &bytes)?;
    Ok((path, index))
}
pub fn load_simple_graph_index(
    runtime_dir: &Path,
) -> Result<Option<SimpleGraphIndex>, CodeGraphError> {
    let path = runtime_dir.join(GRAPH_INDEX_FILE);
    validate_path(&path)?;
    let text = match read_text(&path, MAX_BYTES) {
        Ok(text) => text,
        Err(CodeGraphError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let index: SimpleGraphIndex =
        serde_json::from_str(&text).map_err(|_| CodeGraphError::Invalid("invalid index JSON"))?;
    validate_index(&index)?;
    Ok(Some(index))
}
fn validate_index(index: &SimpleGraphIndex) -> Result<(), CodeGraphError> {
    if index.schema != SCHEMA || index.symbols.len() > MAX_SYMBOLS || index.edges.len() > MAX_EDGES
    {
        return Err(CodeGraphError::Invalid("unsupported index schema or size"));
    }
    let invalid = index.symbols.iter().any(|h| {
        h.kind != GraphQueryKind::SymbolDef || !valid_hit(&h.path, h.line) || !valid_name(&h.symbol)
    }) || index
        .edges
        .iter()
        .any(|e| !valid_hit(&e.path, e.line) || !valid_name(&e.caller) || !valid_name(&e.callee));
    if invalid {
        return Err(CodeGraphError::Invalid("invalid index path or symbol"));
    }
    Ok(())
}
fn source_files(root: &Path) -> Result<Vec<PathBuf>, CodeGraphError> {
    let walker = ignore::WalkBuilder::new(root)
        .follow_links(false)
        .require_git(false)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|e| {
            e.depth() == 0
                || !matches!(
                    e.file_name().to_str(),
                    Some("target" | "node_modules" | "vendor" | "dist" | "build")
                )
        })
        .build();
    let mut files = Vec::new();
    for entry in walker {
        let entry = entry.map_err(|_| CodeGraphError::Invalid("cannot enumerate workspace"))?;
        if entry.file_type().is_some_and(|t| t.is_file())
            && matches!(
                entry.path().extension().and_then(|e| e.to_str()),
                Some("rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go")
            )
        {
            files.push(entry.into_path());
        }
        if files.len() > 4096 {
            return Err(CodeGraphError::Invalid("index exceeds 4096 source files"));
        }
    }
    Ok(files)
}
fn read_text(path: &Path, limit: u64) -> Result<String, CodeGraphError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(CodeGraphError::Invalid(
            "source or index is not a regular file within its size limit",
        ));
    }
    let mut text = String::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        return Err(CodeGraphError::Invalid(
            "source or index exceeds its size limit",
        ));
    }
    Ok(text)
}
fn validate_path(path: &Path) -> Result<(), CodeGraphError> {
    for part in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
        match fs::symlink_metadata(part) {
            Ok(m) if m.is_symlink() => {
                return Err(CodeGraphError::Invalid("index paths cannot be symlinks"))
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}
fn relative_path(root: &Path, path: &Path) -> Result<String, CodeGraphError> {
    path.strip_prefix(root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|p| {
            if cfg!(windows) {
                p.replace('\\', "/")
            } else {
                p.to_owned()
            }
        })
        .ok_or(CodeGraphError::Invalid(
            "source path is not UTF-8 or is outside workspace",
        ))
}
fn valid_hit(path: &str, line: u32) -> bool {
    line > 0
        && !path.is_empty()
        && !path.chars().any(char::is_control)
        && (!cfg!(windows) || !path.contains('\\'))
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn row_number(row: usize) -> Result<u32, CodeGraphError> {
    u32::try_from(row + 1).map_err(|_| CodeGraphError::Invalid("source has too many lines"))
}
fn collect_edges(
    text: &str,
    path: &str,
    definitions: &Regex,
    known: &HashSet<&str>,
    edges: &mut Vec<GraphEdge>,
) -> Result<(), CodeGraphError> {
    // ponytail: this is a line index, not semantic resolution; use language parsers for precise scopes and aliases.
    let mut caller = None;
    for (row, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with("//") || line.starts_with('#') || line.starts_with("/*") {
            continue;
        }
        if let Some(capture) = definitions.captures(line) {
            caller = Some(capture[1].to_owned());
        }
        let Some(caller) = &caller else {
            continue;
        };
        for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            if word == caller || !known.contains(word) {
                continue;
            }
            let kind = if line.contains(&format!("{word}(")) || line.contains(&format!("{word}::"))
            {
                GraphEdgeKind::Call
            } else {
                GraphEdgeKind::Reference
            };
            edges.push(GraphEdge {
                caller: caller.clone(),
                callee: word.into(),
                path: path.into(),
                line: row_number(row)?,
                kind,
            });
            if edges.len() > MAX_EDGES {
                return Err(CodeGraphError::Invalid("index exceeds 64000 edges"));
            }
        }
    }
    Ok(())
}
