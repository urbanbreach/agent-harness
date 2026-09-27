mod launch;
use harness_core::{
    config::{ShellAllowlist, ShellAllowlistMode},
    tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub(crate) struct BashTool {
    allowlist: ShellAllowlist,
    policy: Result<harness_core::sandbox::SandboxPolicy, &'static str>,
}
pub fn register_shell_tool(
    registry: &mut harness_core::tool::ToolRegistry,
    allowlist: ShellAllowlist,
    lookup: &dyn Fn(&str) -> Option<String>,
) {
    registry.register(std::sync::Arc::new(BashTool::new(allowlist, lookup)));
}
impl BashTool {
    fn new(allowlist: ShellAllowlist, lookup: &dyn Fn(&str) -> Option<String>) -> Self {
        let policy = lookup("HARNESS_OS_SANDBOX_POLICY").map_or(
            Ok(harness_core::sandbox::SandboxPolicy::Off),
            |value| {
                harness_core::sandbox::SandboxPolicy::parse(&value).ok_or(
                    "unknown OS sandbox policy; expected off, workspace_write, read_only or strict",
                )
            },
        );
        Self { allowlist, policy }
    }
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    command: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, alias = "cwd")]
    workdir: Option<PathBuf>,
    #[serde(default, alias = "timeout_ms")]
    timeout: Option<u64>,
}
struct Invocation {
    source: String,
    words: Vec<String>,
}
enum Step {
    Command(Invocation),
    Redirect(PathBuf),
}
fn args(value: Value) -> Result<Args, ToolError> {
    serde_json::from_value(value).map_err(|e| ToolError::InvalidArguments(e.to_string()))
}
#[async_trait::async_trait]
impl Tool for BashTool {
    fn id(&self) -> &str {
        "bash"
    }
    fn description(&self) -> &str {
        "Run build, test, and version-control commands with a timeout. Pipes and command lists are supported; expansion, background execution, and shell definitions are rejected."
    }
    fn parameters_json_schema(&self) -> Value {
        schemars::schema_for!(Args).to_value()
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::Shell
    }
    fn permission_requests(&self, value: &Value) -> Vec<(String, String)> {
        let command = value.get("command").and_then(Value::as_str).unwrap_or("");
        let mut requirements = vec![("bash".into(), command.into())];
        if let Ok(commands) = scan(command, &self.allowlist) {
            requirements.extend(commands.into_iter().filter_map(|step| match step {
                Step::Command(c) => Some(("bash".into(), c.source)),
                Step::Redirect(_) => None,
            }));
        }
        requirements
    }
    fn permission_always_patterns(&self, value: &Value) -> Vec<String> {
        let Some(command) = value.get("command").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Ok(commands) = scan(command, &self.allowlist) else {
            return Vec::new();
        };
        let mut patterns = Vec::new();
        for step in commands {
            let Step::Command(command) = step else {
                continue;
            };
            let words = command.words;
            let arity = match words.first().map(String::as_str) {
                Some("cargo" | "npm") if words.get(1).is_some_and(|word| word == "run") => 3,
                Some("cargo" | "npm" | "git" | "python3") => 2,
                _ => 1,
            };
            let prefix = words
                .iter()
                .take(arity)
                .map(String::as_str)
                .collect::<Vec<_>>();
            if prefix
                .iter()
                .any(|word| word.contains(char::is_whitespace) || word.contains('*'))
            {
                return Vec::new();
            }
            patterns.push(format!("{} *", prefix.join(" ")));
        }
        patterns.sort();
        patterns.dedup();
        patterns
    }
    fn filesystem_paths(&self, value: &Value) -> Result<Vec<PathBuf>, ToolError> {
        let args = args(value.clone())?;
        let commands = scan(&args.command, &self.allowlist)?;
        let mut cwd = args.workdir.unwrap_or_else(|| ".".into());
        let mut paths = vec![cwd.clone()];
        for step in commands {
            let command = match step {
                Step::Redirect(path) => {
                    paths.push(cwd.join(path));
                    continue;
                }
                Step::Command(command) => command,
            };
            if command.words.first().is_some_and(|w| w == "cd") {
                let dir = command.words.get(1).ok_or_else(|| {
                    ToolError::InvalidArguments("cd requires an explicit path".into())
                })?;
                cwd = cwd.join(dir);
                paths.push(cwd.clone());
                continue;
            }
            for word in command.words {
                let value = word
                    .split_once('=')
                    .map_or(word.as_str(), |(_, value)| value);
                let value = value.strip_prefix("-C").unwrap_or(value);
                if value.starts_with('/') || value.starts_with("./") || value.starts_with("../") {
                    paths.push(cwd.join(value));
                }
            }
        }
        Ok(paths)
    }
    async fn call(&self, context: ToolContext, value: Value) -> Result<ToolResult, ToolError> {
        let args = args(value)?;
        scan(&args.command, &self.allowlist)?;
        let timeout = args.timeout.unwrap_or(120_000);
        if !(1..=3_600_000).contains(&timeout) {
            return Err(ToolError::InvalidArguments(
                "timeout must be between 1 and 3600000 milliseconds".into(),
            ));
        }
        let cwd =
            context.resolve_workspace_path(args.workdir.as_deref().unwrap_or(Path::new(".")))?;
        if !cwd.is_dir() {
            return Err(ToolError::InvalidArguments(
                "workdir must be a directory".into(),
            ));
        }
        if !self.allowlist.cwd_roots.is_empty()
            && !self.allowlist.cwd_roots.iter().any(|root| {
                context
                    .resolve_workspace_path(Path::new(root))
                    .is_ok_and(|root| cwd.starts_with(root))
            })
        {
            return Err(ToolError::Execution(
                "workdir is outside the configured command roots".into(),
            ));
        }
        let policy = self
            .policy
            .map_err(|error| ToolError::Execution(error.into()))?;
        let (mut command, scratch) = launch::prepare(&context, policy)?;
        command
            .args(["--noprofile", "--norc", "-c", &args.command])
            .current_dir(cwd);
        launch::environment(&mut command, scratch.as_ref().map(tempfile::TempDir::path));
        let output = crate::process::run(
            command,
            Duration::from_millis(timeout),
            &context.cancellation,
        )
        .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let display = format!(
            "{stdout}{stderr}\nExit code: {}{}",
            output
                .status
                .code()
                .map_or_else(|| "signal".into(), |c| c.to_string()),
            if output.truncated {
                " (output truncated)"
            } else {
                ""
            }
        );
        Ok(ToolResult::structured(
            display,
            json!({"exit_code":output.status.code(), "is_error":!output.status.success(), "stdout":stdout, "stderr":stderr, "truncated":output.truncated, "os_sandbox_policy":policy.as_str()}),
        ))
    }
}

fn scan(source: &str, policy: &ShellAllowlist) -> Result<Vec<Step>, ToolError> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|_| ToolError::Execution("Bash parser is unavailable".into()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| ToolError::InvalidArguments("invalid Bash command".into()))?;
    if tree.root_node().has_error() {
        return Err(ToolError::InvalidArguments("invalid Bash syntax".into()));
    }
    let mut nodes = vec![tree.root_node()];
    let mut commands = Vec::new();
    while let Some(node) = nodes.pop() {
        if matches!(
            node.kind(),
            "&" | "command_substitution"
                | "process_substitution"
                | "expansion"
                | "simple_expansion"
                | "arithmetic_expansion"
                | "variable_assignment"
                | "function_definition"
                | "for_statement"
                | "c_style_for_statement"
                | "while_statement"
                | "if_statement"
                | "case_statement"
                | "subshell"
                | "declaration_command"
                | "unset_command"
        ) {
            return Err(ToolError::InvalidArguments(format!(
                "unsupported Bash construct: {}",
                node.kind()
            )));
        }
        if node.kind() == "command" {
            let text = node
                .utf8_text(source.as_bytes())
                .map_err(|_| ToolError::InvalidArguments("invalid command encoding".into()))?;
            let mut cursor = node.walk();
            let words = node
                .child_by_field_name("name")
                .into_iter()
                .chain(node.children_by_field_name("argument", &mut cursor))
                .map(|node| literal(node, source))
                .collect::<Result<Vec<_>, _>>()?;
            let executable = words
                .first()
                .ok_or_else(|| ToolError::InvalidArguments("empty command".into()))?;
            let name = Path::new(executable)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(executable);
            let discovery =
                name == "command" && words.len() >= 3 && matches!(words[1].as_str(), "-v" | "-V");
            if [
                "source", ".", "eval", "exec", "builtin", "env", "printenv", "set", "export", "sh",
                "bash", "zsh", "fish", "dash", "ksh",
            ]
            .contains(&name)
                || (name == "command" && !discovery)
                || executable.contains(['*', '?', '[', '{', '~'])
            {
                return Err(ToolError::Execution(format!(
                    "{name} is not allowed in bash"
                )));
            }
            if policy.mode == ShellAllowlistMode::LegacyExecutables
                && ((executable != "cd" && !policy.executables.contains(executable))
                    || (matches!(name, "python" | "python3" | "node" | "ruby" | "perl")
                        && words
                            .iter()
                            .skip(1)
                            .any(|word| matches!(word.as_str(), "-c" | "-e" | "--eval" | "-"))))
            {
                return Err(ToolError::Execution(format!(
                    "{executable} is not in the executable allowlist"
                )));
            }
            commands.push(Step::Command(Invocation {
                source: text.into(),
                words,
            }));
        }
        if node.kind() == "file_redirect" {
            let mut cursor = node.walk();
            let descriptor = node
                .children(&mut cursor)
                .any(|child| matches!(child.kind(), ">&" | "<&"));
            let mut cursor = node.walk();
            for destination in node.children_by_field_name("destination", &mut cursor) {
                let path = literal(destination, source)?;
                if path != "/dev/null"
                    && !(descriptor && (path == "-" || path.parse::<u32>().is_ok()))
                {
                    commands.push(Step::Redirect(path.into()));
                }
            }
        }
        let mut cursor = node.walk();
        nodes.extend(
            node.children(&mut cursor)
                .collect::<Vec<_>>()
                .into_iter()
                .rev(),
        );
    }
    if !commands.iter().any(|step| matches!(step, Step::Command(_))) {
        return Err(ToolError::InvalidArguments(
            "command must not be empty".into(),
        ));
    }
    Ok(commands)
}

fn literal(node: tree_sitter::Node<'_>, source: &str) -> Result<String, ToolError> {
    let text = node
        .utf8_text(source.as_bytes())
        .map_err(|_| ToolError::InvalidArguments("invalid command encoding".into()))?;
    let mut words = shell_words::split(text)
        .map_err(|_| ToolError::InvalidArguments("invalid command quoting".into()))?;
    if words.len() != 1 {
        return Err(ToolError::InvalidArguments(
            "expected one literal shell word".into(),
        ));
    }
    Ok(words.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_policy_checks_commands_without_interpreting_quoted_data() -> Result<(), ToolError> {
        let policy = ShellAllowlist::default();
        for source in [
            "printf '%s' '$(literal) &'; echo ok | cat",
            "python3 - <<'PY'\nprint('$HOME')\nPY",
            "command -v cargo",
            "echo hi > ./report 2>&1",
            "printf *.rs > /dev/null",
        ] {
            assert!(scan(source, &policy).is_ok(), "rejected: {source}");
        }
        for source in [
            "echo $HOME",
            "echo $(pwd)",
            "echo `pwd`",
            "cat <(echo x)",
            "echo hi &",
            "env",
            "FOO=bar cargo build",
            "bash -c 'echo hi'",
            "command cargo build",
            "eval echo hi",
        ] {
            assert!(scan(source, &policy).is_err(), "accepted: {source}");
        }
        let tool = BashTool::new(policy, &|_| None);
        let paths =
            tool.filesystem_paths(&json!({"command":"cd sub && printf hi > ../../outside.txt"}))?;
        assert!(paths.contains(&PathBuf::from("./sub/../../outside.txt")));
        assert!(!tool
            .filesystem_paths(&json!({"command":"echo ok >/dev/null 2>&1"}))?
            .contains(&PathBuf::from("/dev/null")));
        let legacy = ShellAllowlist {
            mode: ShellAllowlistMode::LegacyExecutables,
            executables: vec!["python3".into(), "cargo".into()],
            ..Default::default()
        };
        assert!(scan("cargo check", &legacy).is_ok());
        assert!(scan("python3 -c 'print(1)'", &legacy).is_err());
        assert!(scan("echo ok", &legacy).is_err());
        Ok(())
    }
}
