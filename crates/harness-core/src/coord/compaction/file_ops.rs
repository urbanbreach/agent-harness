// allow: SIZE_OK — file operations tracking for compaction summarization
use std::collections::HashSet;

use serde_json::Value;

/// File path sets accumulated over a compaction window.
#[derive(Debug, Clone, Default)]
pub struct FileOperations {
    /// Files that were read (only).
    pub read: HashSet<String>,
    /// Files that were written (created or overwritten).
    pub written: HashSet<String>,
    /// Files that were edited (modified in place).
    pub edited: HashSet<String>,
}

impl FileOperations {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A single file operation inferred from one tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOperation {
    pub read: bool,
    pub written: bool,
    pub edited: bool,
    pub path: String,
}

/// Regular expression for extracting file paths from a bash command string.
///
/// Mirrors the pattern from Pi's compaction utils, with a fix for extensionless
/// filenames (`Makefile`, `Dockerfile`) that the original pattern incorrectly
/// placed inside the dot-extension group.
const BASH_PATH_REGEX: &str = r"(?:^|\s)([\w\-/]+\.(?:md|txt|rs|ts|js|json|toml|yaml|yml|py|go|java|kt|swift|c|cpp|h|hpp|cs|rb|php|sh)|Makefile|Dockerfile)";

/// Extract a single file path from tool arguments for the given key.
///
/// Returns `None` if the key is absent or not a string.
fn extract_path_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from)
}

/// Extract all file paths matching supported extensions from a bash command string.
///
/// Uses the same heuristic as Pi: scan the command for paths that end in a known
/// source/config file extension.
fn extract_paths_from_bash_command(command: &str) -> Vec<String> {
    let Some(re) = regex::Regex::new(BASH_PATH_REGEX).ok() else {
        return Vec::new();
    };
    re.find_iter(command)
        .map(|m| m.as_str().trim().to_string())
        .collect()
}

/// Extract a single `FileOperation` from a tool call.
///
/// Returns `None` when the tool does not operate on a file path or no path could
/// be extracted.
///
/// ## Tool mapping (mirrors Pi)
///
/// | tool_id | operation |
/// |---------|-----------|
/// | `read`, `list`, `glob`, `grep` | read |
/// | `edit` | edited + written |
/// | `write` | written |
/// | `bash` | extract paths from command string via regex |
pub fn extract_file_ops_from_tool_call(tool_id: &str, args: &Value) -> Option<FileOperation> {
    match tool_id {
        // Read-only tools — extract `path`, `file`, or `pattern`
        "read" | "list" | "glob" | "grep" => {
            let path = extract_path_arg(args, "path")
                .or_else(|| extract_path_arg(args, "file"))
                .or_else(|| extract_path_arg(args, "pattern"))?;
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path,
            })
        }

        // Edit is both edited and written (touches the file on disk)
        "edit" => {
            let path = extract_path_arg(args, "path")?;
            Some(FileOperation {
                read: false,
                written: true,
                edited: true,
                path,
            })
        }

        // Write creates or overwrites a file
        "write" => {
            let path = extract_path_arg(args, "path")?;
            Some(FileOperation {
                read: false,
                written: true,
                edited: false,
                path,
            })
        }

        // Bash may read or write files referenced in the command string
        "bash" => {
            let command = args.get("command")?.as_str()?.trim();
            let paths = extract_paths_from_bash_command(command);
            // Return the first detected path; if none, bail out so the tool
            // call contributes no file operation.
            let path = paths.into_iter().next()?;
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path,
            })
        }

        _ => None,
    }
}

/// Merge a single `FileOperation` into the accumulated `FileOperations`.
pub fn merge_file_operations(ops: &mut FileOperations, op: FileOperation) {
    if op.read {
        ops.read.insert(op.path.clone());
    }
    if op.written {
        ops.written.insert(op.path.clone());
    }
    if op.edited {
        ops.edited.insert(op.path);
    }
}

/// Compute final `readFiles` and `modifiedFiles` lists from accumulated operations.
///
/// - `readFiles`: files that were read but **not** edited or written (read-only files).
/// - `modifiedFiles`: all files that were edited or written (including files that
///   were both read and modified).
///
/// The result vectors are sorted for deterministic output.
pub fn compute_file_lists(ops: &FileOperations) -> (Vec<String>, Vec<String>) {
    // modified = edited ∪ written
    let modified: HashSet<String> = ops.edited.union(&ops.written).cloned().collect();

    // read_only = read − modified
    let read_only: Vec<String> = ops
        .read
        .iter()
        .filter(|p| !modified.contains(*p))
        .cloned()
        .collect();

    let mut read_files = read_only;
    read_files.sort();

    let mut modified_files: Vec<String> = modified.into_iter().collect();
    modified_files.sort();

    (read_files, modified_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // -------------------------------------------------------------------------
    // Helper
    // -------------------------------------------------------------------------
    fn json_args(pairs: &[(&str, &str)]) -> Value {
        let mut m = serde_json::Map::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), json!(v));
        }
        Value::Object(m)
    }

    // -------------------------------------------------------------------------
    // extract_file_ops_from_tool_call
    // -------------------------------------------------------------------------

    #[test]
    fn extract_file_ops_from_tool_call_read_path() {
        let args = json_args(&[("path", "src/main.rs")]);
        let op = extract_file_ops_from_tool_call("read", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "src/main.rs".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_read_file_alias() {
        let args = json_args(&[("file", "README.md")]);
        let op = extract_file_ops_from_tool_call("read", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "README.md".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_read_pattern() {
        let args = json_args(&[("pattern", "*.rs")]);
        let op = extract_file_ops_from_tool_call("grep", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "*.rs".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_list() {
        let args = json_args(&[("path", "/tmp/workspace")]);
        let op = extract_file_ops_from_tool_call("list", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "/tmp/workspace".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_glob() {
        let args = json_args(&[("path", "**/*.json")]);
        let op = extract_file_ops_from_tool_call("glob", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "**/*.json".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_edit() {
        let args = json_args(&[("path", "Cargo.toml")]);
        let op = extract_file_ops_from_tool_call("edit", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: false,
                written: true,
                edited: true,
                path: "Cargo.toml".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_write() {
        let args = json_args(&[("path", "new_file.txt")]);
        let op = extract_file_ops_from_tool_call("write", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: false,
                written: true,
                edited: false,
                path: "new_file.txt".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_bash_extracts_file_paths() {
        let args = json_args(&[("command", "cat src/main.rs && echo done")]);
        let op = extract_file_ops_from_tool_call("bash", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "src/main.rs".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_bash_extracts_multiple_extensions() {
        // `cargo build` reads Cargo.toml
        let args = json_args(&[("command", "cargo build --manifest-path Cargo.toml")]);
        let op = extract_file_ops_from_tool_call("bash", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "Cargo.toml".into(),
            })
        );

        // `python script.py`
        let args = json_args(&[("command", "python scripts/deploy.py")]);
        let op = extract_file_ops_from_tool_call("bash", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "scripts/deploy.py".into(),
            })
        );

        // Makefile
        let args = json_args(&[("command", "make -f Makefile")]);
        let op = extract_file_ops_from_tool_call("bash", &args);
        assert_eq!(
            op,
            Some(FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "Makefile".into(),
            })
        );
    }

    #[test]
    fn extract_file_ops_from_tool_call_bash_no_path_returns_none() {
        // Command with no detectable file path
        let args = json_args(&[("command", "echo hello world")]);
        let op = extract_file_ops_from_tool_call("bash", &args);
        assert_eq!(op, None);
    }

    #[test]
    fn extract_file_ops_from_tool_call_unknown_tool_returns_none() {
        let args = json_args(&[("path", "something.txt")]);
        let op = extract_file_ops_from_tool_call("webfetch", &args);
        assert_eq!(op, None);
    }

    #[test]
    fn extract_file_ops_from_tool_call_missing_path_returns_none() {
        // read with no path field
        let args = json_args(&[]);
        let op = extract_file_ops_from_tool_call("read", &args);
        assert_eq!(op, None);
    }

    #[test]
    fn extract_file_ops_from_tool_call_empty_path_returns_none() {
        let args = json_args(&[("path", "   ")]);
        let op = extract_file_ops_from_tool_call("read", &args);
        assert_eq!(op, None);
    }

    // -------------------------------------------------------------------------
    // merge_file_operations
    // -------------------------------------------------------------------------

    #[test]
    fn merge_file_operations_read_only() {
        let mut ops = FileOperations::new();
        merge_file_operations(
            &mut ops,
            FileOperation {
                read: true,
                written: false,
                edited: false,
                path: "src/lib.rs".into(),
            },
        );
        assert_eq!(ops.read.len(), 1);
        assert!(ops.written.is_empty());
        assert!(ops.edited.is_empty());
    }

    #[test]
    fn merge_file_operations_edit() {
        let mut ops = FileOperations::new();
        merge_file_operations(
            &mut ops,
            FileOperation {
                read: false,
                written: true,
                edited: true,
                path: "Cargo.toml".into(),
            },
        );
        assert!(ops.read.is_empty());
        assert!(ops.written.contains("Cargo.toml"));
        assert!(ops.edited.contains("Cargo.toml"));
    }

    // -------------------------------------------------------------------------
    // compute_file_lists
    // -------------------------------------------------------------------------

    #[test]
    fn compute_file_lists_read_only_file() {
        let mut ops = FileOperations::new();
        ops.read.insert("src/lib.rs".into());
        ops.read.insert("README.md".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert_eq!(read_files, vec!["README.md", "src/lib.rs"]);
        assert!(modified_files.is_empty());
    }

    #[test]
    fn compute_file_lists_modified_file() {
        let mut ops = FileOperations::new();
        ops.edited.insert("Cargo.toml".into());
        ops.written.insert("new_file.txt".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert!(read_files.is_empty());
        assert_eq!(modified_files, vec!["Cargo.toml", "new_file.txt"]);
    }

    #[test]
    fn compute_file_lists_read_then_edit_goes_to_modified() {
        // File was read AND edited — it belongs in modified_files, not read_files
        let mut ops = FileOperations::new();
        ops.read.insert("src/main.rs".into());
        ops.edited.insert("src/main.rs".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert!(read_files.is_empty());
        assert_eq!(modified_files, vec!["src/main.rs"]);
    }

    #[test]
    fn compute_file_lists_read_then_write_goes_to_modified() {
        // File was read AND written — it belongs in modified_files
        let mut ops = FileOperations::new();
        ops.read.insert("config.json".into());
        ops.written.insert("config.json".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert!(read_files.is_empty());
        assert_eq!(modified_files, vec!["config.json"]);
    }

    #[test]
    fn compute_file_lists_deduplicates() {
        // Same path in edited and written should appear once in modified_files
        let mut ops = FileOperations::new();
        ops.edited.insert("Cargo.toml".into());
        ops.written.insert("Cargo.toml".into());

        let (_, modified_files) = compute_file_lists(&ops);
        assert_eq!(modified_files, vec!["Cargo.toml"]);
    }

    #[test]
    fn compute_file_lists_sorted_output() {
        let mut ops = FileOperations::new();
        ops.edited.insert("z.rs".into());
        ops.edited.insert("a.rs".into());
        ops.edited.insert("m.rs".into());

        let (_, modified_files) = compute_file_lists(&ops);
        assert_eq!(modified_files, vec!["a.rs", "m.rs", "z.rs"]);
    }

    #[test]
    fn compute_file_lists_mixed_operations() {
        let mut ops = FileOperations::new();
        // Read-only
        ops.read.insert("README.md".into());
        ops.read.insert("docs/guide.md".into());
        // Modified
        ops.edited.insert("src/main.rs".into());
        ops.written.insert("new_file.txt".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert_eq!(read_files, vec!["README.md", "docs/guide.md"]);
        assert_eq!(modified_files, vec!["new_file.txt", "src/main.rs"]);
    }

    #[test]
    fn compute_file_lists_all_modified_none_read() {
        let mut ops = FileOperations::new();
        ops.edited.insert("a.rs".into());
        ops.edited.insert("b.rs".into());
        ops.written.insert("c.rs".into());
        // Also "read" the same files — they should still be in modified, not read
        ops.read.insert("a.rs".into());
        ops.read.insert("b.rs".into());

        let (read_files, modified_files) = compute_file_lists(&ops);
        assert!(read_files.is_empty());
        assert_eq!(modified_files, vec!["a.rs", "b.rs", "c.rs"]);
    }
}
