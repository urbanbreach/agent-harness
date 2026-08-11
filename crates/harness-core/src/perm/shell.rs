// allow: SIZE_OK — shell permission system (command request + allowlist + path validation + builtin rules)
use crate::UnwrapOrAbort;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellCommandRequest {
    pub original: String,
    pub commands: Vec<ScannedShellCommand>,
    pub patterns: Vec<String>,
    pub always_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedShellCommand {
    pub source: String,
    pub executable: String,
    pub tokens: Vec<String>,
    pub arity_tokens: Vec<String>,
    pub pattern: String,
    pub always_pattern: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_patterns: Vec<ShellPathPattern>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellPathPattern {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ShellScanError {
    #[error("failed to parse command string: unclosed quote")]
    UnclosedQuote,
    #[error("failed to parse command string: dangling escape")]
    DanglingEscape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuoteState {
    None,
    Single,
    Double,
}

pub fn scan_shell_command(command: &str) -> Result<ShellCommandRequest, ShellScanError> {
    let commands = match scan_single_python_heredoc(command)? {
        Some(command) => vec![command],
        None => {
            let mut commands = Vec::new();
            for source in split_shell_commands(command)? {
                let tokens = tokenize_shell_command(&source)?;
                if let Some(scanned) = scan_tokens(source, tokens) {
                    commands.push(scanned);
                }
            }
            commands
        }
    };
    let patterns = unique(commands.iter().map(|command| command.pattern.clone()));
    let always_patterns = unique(
        commands
            .iter()
            .map(|command| command.always_pattern.clone()),
    );
    Ok(ShellCommandRequest {
        original: command.to_string(),
        commands,
        patterns,
        always_patterns,
    })
}

fn scan_single_python_heredoc(
    command: &str,
) -> Result<Option<ScannedShellCommand>, ShellScanError> {
    let mut lines = command.lines();
    let Some(header) = lines.next() else {
        return Ok(None);
    };
    let tokens = tokenize_shell_command(header)?;
    if !matches!(
        tokens.first().map(String::as_str),
        Some("python" | "python3")
    ) {
        return Ok(None);
    }
    let Some(redirection_index) = tokens.iter().position(|token| token == "<<") else {
        return Ok(None);
    };
    if tokens.iter().filter(|token| token.as_str() == "<<").count() != 1
        || redirection_index + 2 != tokens.len()
    {
        return Ok(None);
    }
    let Some(delimiter) = tokens.get(redirection_index + 1) else {
        return Ok(None);
    };

    let mut found_delimiter = false;
    for line in lines {
        if found_delimiter {
            if !line.trim().is_empty() {
                return Ok(None);
            }
        } else if line == delimiter {
            found_delimiter = true;
        }
    }
    if !found_delimiter {
        return Ok(None);
    }

    Ok(scan_tokens(command.to_string(), tokens))
}

pub fn direct_shell_command_request(cmd: &str, args: &[String]) -> ShellCommandRequest {
    let mut tokens = Vec::with_capacity(args.len() + 1);
    tokens.push(cmd.to_string());
    tokens.extend(args.iter().cloned());
    let source = tokens.join(" ");
    let commands = scan_tokens(source.clone(), tokens)
        .into_iter()
        .collect::<Vec<_>>();
    let patterns = unique(commands.iter().map(|command| command.pattern.clone()));
    let always_patterns = unique(
        commands
            .iter()
            .map(|command| command.always_pattern.clone()),
    );
    ShellCommandRequest {
        original: source,
        commands,
        patterns,
        always_patterns,
    }
}

pub fn shell_permission_pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut remainder = value;
    let mut parts = pattern.split('*').peekable();
    let mut anchored_start = !pattern.starts_with('*');
    while let Some(part) = parts.next() {
        if part.is_empty() {
            anchored_start = false;
            continue;
        }
        if anchored_start {
            let Some(next) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = next;
            anchored_start = false;
            continue;
        }
        let Some(index) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[index + part.len()..];
        if parts.peek().is_none() && !pattern.ends_with('*') {
            return remainder.is_empty();
        }
    }
    pattern.ends_with('*') || remainder.is_empty()
}

fn split_shell_commands(command: &str) -> Result<Vec<String>, ShellScanError> {
    let mut commands = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut quote = QuoteState::None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quote != QuoteState::Single => {
                current.push(ch);
                escaped = true;
            }
            '\'' if quote == QuoteState::None => {
                quote = QuoteState::Single;
                current.push(ch);
            }
            '\'' if quote == QuoteState::Single => {
                quote = QuoteState::None;
                current.push(ch);
            }
            '"' if quote == QuoteState::None => {
                quote = QuoteState::Double;
                current.push(ch);
            }
            '"' if quote == QuoteState::Double => {
                quote = QuoteState::None;
                current.push(ch);
            }
            '&' if quote == QuoteState::None && chars.peek() == Some(&'&') => {
                chars.next();
                push_command(&mut commands, &mut current);
            }
            '|' if quote == QuoteState::None => {
                if matches!(chars.peek(), Some('|') | Some('&')) {
                    chars.next();
                }
                push_command(&mut commands, &mut current);
            }
            ';' | '\n' if quote == QuoteState::None => push_command(&mut commands, &mut current),
            _ => current.push(ch),
        }
    }
    if escaped {
        return Err(ShellScanError::DanglingEscape);
    }
    if quote != QuoteState::None {
        return Err(ShellScanError::UnclosedQuote);
    }
    push_command(&mut commands, &mut current);
    Ok(commands)
}

fn push_command(commands: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        commands.push(trimmed.to_string());
    }
    current.clear();
}

fn tokenize_shell_command(source: &str) -> Result<Vec<String>, ShellScanError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = source.chars().peekable();
    let mut quote = QuoteState::None;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quote != QuoteState::Single => escaped = true,
            '\'' if quote == QuoteState::None => quote = QuoteState::Single,
            '\'' if quote == QuoteState::Single => quote = QuoteState::None,
            '"' if quote == QuoteState::None => quote = QuoteState::Double,
            '"' if quote == QuoteState::Double => quote = QuoteState::None,
            ch if quote == QuoteState::None && ch.is_whitespace() => {
                push_token(&mut tokens, &mut current)
            }
            '<' | '>' if quote == QuoteState::None => {
                push_redirection_token(ch, &mut chars, &mut tokens, &mut current)
            }
            _ => current.push(ch),
        }
    }
    if escaped {
        return Err(ShellScanError::DanglingEscape);
    }
    if quote != QuoteState::None {
        return Err(ShellScanError::UnclosedQuote);
    }
    push_token(&mut tokens, &mut current);
    Ok(tokens)
}

fn push_redirection_token(
    ch: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    tokens: &mut Vec<String>,
    current: &mut String,
) {
    push_token(tokens, current);
    let mut token = ch.to_string();
    while chars
        .peek()
        .is_some_and(|next| matches!(next, '<' | '>' | '&'))
    {
        if let Some(next) = chars.next() {
            token.push(next);
        }
    }
    tokens.push(token);
}

fn push_token(tokens: &mut Vec<String>, current: &mut String) {
    if !current.is_empty() {
        tokens.push(std::mem::take(current));
    }
}

fn scan_tokens(source: String, tokens: Vec<String>) -> Option<ScannedShellCommand> {
    let executable = tokens.first()?.clone();
    let arity_tokens = shell_arity_tokens(&tokens);
    let always_pattern = format!("{} *", arity_tokens.join(" "));
    Some(ScannedShellCommand {
        pattern: source.clone(),
        path_patterns: shell_path_patterns(&tokens),
        source,
        executable,
        tokens,
        arity_tokens,
        always_pattern,
    })
}

fn shell_arity_tokens(tokens: &[String]) -> Vec<String> {
    if uses_shell_command_mode(tokens) {
        return tokens.to_vec();
    }
    for (prefix, arity) in SHELL_ARITY {
        let prefix_tokens = prefix.split(' ').collect::<Vec<_>>();
        if tokens.len() >= prefix_tokens.len()
            && tokens
                .iter()
                .zip(prefix_tokens.iter())
                .all(|(token, prefix_token)| token == prefix_token)
        {
            return tokens.iter().take(*arity).cloned().collect();
        }
    }
    tokens.iter().take(1).cloned().collect()
}

fn uses_shell_command_mode(tokens: &[String]) -> bool {
    matches!(
        tokens.first().map(String::as_str),
        Some("bash" | "sh" | "zsh" | "fish")
    ) && tokens.iter().skip(1).any(|token| {
        token == "-c"
            || (token.starts_with('-') && !token.starts_with("--") && token[1..].contains('c'))
    })
}

const SHELL_ARITY: &[(&str, usize)] = &[
    ("python3 -c", 2),
    ("cargo test", 2),
    ("cargo run", 3),
    ("npm run", 3),
    ("git status", 2),
    ("git diff", 2),
    ("git log", 2),
    ("git show", 2),
    ("find", 1),
    ("rg", 1),
    ("python3", 2),
    ("printf", 1),
    ("sed", 1),
    ("awk", 1),
    ("head", 1),
    ("tail", 1),
    ("cargo", 2),
    ("npm", 2),
    ("git", 2),
];

fn shell_path_patterns(tokens: &[String]) -> Vec<ShellPathPattern> {
    let mut paths = Vec::new();
    let mut next_is_redirection = false;
    let executable = tokens.first().map(String::as_str).unwrap_or_default();
    for (index, token) in tokens.iter().enumerate().skip(1) {
        if is_redirection_token(token) {
            next_is_redirection = true;
            continue;
        }
        if token.starts_with('-') {
            if let Some((_, value)) = token.split_once('=') {
                if is_path_like(value) {
                    paths.push(ShellPathPattern {
                        path: value.to_string(),
                        kind: "option".to_string(),
                    });
                }
            }
            next_is_redirection = false;
            continue;
        }
        if next_is_redirection {
            paths.push(ShellPathPattern {
                path: token.clone(),
                kind: "redirection".to_string(),
            });
            next_is_redirection = false;
        } else if is_path_like(token) {
            paths.push(ShellPathPattern {
                path: token.clone(),
                kind: "argument".to_string(),
            });
        } else if tracks_bare_path_arguments(executable)
            && !is_grep_pattern_argument(executable, tokens, index)
        {
            paths.push(ShellPathPattern {
                path: token.clone(),
                kind: "argument".to_string(),
            });
        }
    }
    paths
}

fn tracks_bare_path_arguments(executable: &str) -> bool {
    matches!(
        executable,
        "cat"
            | "bash"
            | "cp"
            | "find"
            | "fish"
            | "grep"
            | "head"
            | "ls"
            | "mv"
            | "node"
            | "perl"
            | "python"
            | "python3"
            | "rg"
            | "rm"
            | "ruby"
            | "sed"
            | "sh"
            | "tail"
            | "touch"
            | "zsh"
    )
}

fn is_grep_pattern_argument(executable: &str, tokens: &[String], index: usize) -> bool {
    if !matches!(executable, "grep" | "rg") {
        return false;
    }

    tokens
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, token)| !token.starts_with('-') && !is_redirection_token(token))
        .next()
        .is_some_and(|(pattern_index, _)| pattern_index == index)
}

fn is_redirection_token(token: &str) -> bool {
    token.contains('<') || token.contains('>')
}

fn is_path_like(token: &str) -> bool {
    token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token == "."
        || token == ".."
        || token.contains('/')
}

fn unique(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{scan_shell_command, shell_permission_pattern_matches};
    use crate::UnwrapOrAbort;

    #[test]
    fn scan_shell_command_extracts_pipeline_patterns() {
        // arrange
        // act
        // assert
        let request =
            scan_shell_command("git status --short | rg '^ M' && cargo test -p harness-core")
                .unwrap_or_abort();

        assert_eq!(
            request.patterns,
            vec![
                "git status --short".to_string(),
                "rg '^ M'".to_string(),
                "cargo test -p harness-core".to_string(),
            ]
        );
        assert_eq!(
            request.always_patterns,
            vec![
                "git status *".to_string(),
                "rg *".to_string(),
                "cargo test *".to_string()
            ]
        );
    }

    #[test]
    fn scan_shell_command_extracts_redirection_path() {
        // arrange
        // act
        // assert
        let request = scan_shell_command("printf hi > artifacts/out.txt").unwrap_or_abort();

        assert_eq!(
            request.commands[0].path_patterns[0].path,
            "artifacts/out.txt"
        );
        assert_eq!(request.commands[0].path_patterns[0].kind, "redirection");
    }

    #[test]
    fn scan_shell_command_extracts_path_from_option_value() {
        // arrange
        let command = "chromium --screenshot=/tmp/harness-index.png index.html";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert_eq!(
            request.commands[0].path_patterns[0].path,
            "/tmp/harness-index.png"
        );
        assert_eq!(request.commands[0].path_patterns[0].kind, "option");
    }

    #[test]
    fn scan_shell_command_extracts_bare_file_operand_paths() {
        // arrange
        let command = "grep needle leak && cat other && bash script.sh && node app.js";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert_eq!(request.commands[0].path_patterns[0].path, "leak");
        assert_eq!(request.commands[0].path_patterns[0].kind, "argument");
        assert_eq!(request.commands[1].path_patterns[0].path, "other");
        assert_eq!(request.commands[1].path_patterns[0].kind, "argument");
        assert_eq!(request.commands[2].path_patterns[0].path, "script.sh");
        assert_eq!(request.commands[2].path_patterns[0].kind, "argument");
        assert_eq!(request.commands[3].path_patterns[0].path, "app.js");
        assert_eq!(request.commands[3].path_patterns[0].kind, "argument");
    }

    #[test]
    fn scan_shell_command_extracts_touch_rm_patterns() {
        // arrange
        // act
        // assert
        let request =
            scan_shell_command("touch src/new.rs; rm -f target/tmp.txt").unwrap_or_abort();

        assert_eq!(
            request.always_patterns,
            vec!["touch *".to_string(), "rm *".to_string()]
        );
    }

    #[test]
    fn scan_shell_command_extracts_python3_c_always_pattern() {
        // arrange
        // act
        // assert
        let request = scan_shell_command("python3 -c 'print(1)'").unwrap_or_abort();

        assert_eq!(request.commands[0].arity_tokens, vec!["python3", "-c"]);
        assert_eq!(request.always_patterns, vec!["python3 -c *".to_string()]);
    }

    #[test]
    fn scan_shell_command_excludes_heredoc_body_from_permission_patterns() {
        // arrange
        let command = "python3 - <<'PY'\nprint('ok')\nPY";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert_eq!(request.commands.len(), 1);
        assert_eq!(request.commands[0].executable, "python3");
        assert_eq!(request.always_patterns, vec!["python3 - *".to_string()]);
    }

    #[test]
    fn scan_shell_command_does_not_mistake_quoted_shift_for_heredoc() {
        // arrange
        let command = "python3 -c 'print(1 << 2)'\nrm -rf /tmp/target";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert_eq!(request.commands.len(), 2);
        assert_eq!(request.commands[1].executable, "rm");
    }

    #[test]
    fn scan_shell_command_does_not_mistake_here_string_for_heredoc() {
        // arrange
        let command = "cat <<< harmless\nrm -rf /tmp/target";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert_eq!(request.commands.len(), 2);
        assert_eq!(request.commands[1].executable, "rm");
    }

    #[test]
    fn scan_shell_command_keeps_trailing_command_after_heredoc_visible() {
        // arrange
        let command = "python3 - <<'PY'\nprint('ok')\nPY\nrm -rf /tmp/target";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert!(request
            .commands
            .iter()
            .any(|command| command.executable == "rm"));
    }

    #[test]
    fn scan_shell_command_keeps_same_line_heredoc_pipeline_visible() {
        // arrange
        let command = "python3 - <<'PY' | sh\nprint('echo bypass')\nPY";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert!(request
            .commands
            .iter()
            .any(|command| command.executable == "sh"));
    }

    #[test]
    fn scan_shell_command_keeps_same_line_heredoc_conditionals_visible() {
        // arrange
        let command = "python3 - <<'PY' && env || source secrets\nprint('ok')\nPY";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert!(request
            .commands
            .iter()
            .any(|command| command.executable == "env"));
        assert!(request
            .commands
            .iter()
            .any(|command| command.executable == "source"));
    }

    #[test]
    fn scan_shell_command_does_not_collapse_multiple_heredocs() {
        // arrange
        let command = "python3 - <<'FIRST' <<'SECOND'\nprint('one')\nFIRST\nprint('two')\nSECOND";

        // act
        let request = scan_shell_command(command).unwrap_or_abort();

        // assert
        assert!(request.commands.len() > 1);
    }

    #[test]
    fn shell_always_pattern_uses_cargo_test_prefix() {
        // arrange
        // act
        // assert
        let request = scan_shell_command("cargo test -p harness-core --lib").unwrap_or_abort();

        assert_eq!(request.commands[0].arity_tokens, vec!["cargo", "test"]);
        assert_eq!(request.always_patterns, vec!["cargo test *".to_string()]);
    }

    #[test]
    fn shell_permission_pattern_matches_return_correct_bool() {
        // arrange
        // act
        // assert
        assert!(shell_permission_pattern_matches(
            "cargo test *",
            "cargo test *"
        ));
        assert!(shell_permission_pattern_matches("cargo *", "cargo test *"));
        assert!(shell_permission_pattern_matches("* test *", "cargo test *"));
        assert!(!shell_permission_pattern_matches(
            "git status *",
            "git diff *"
        ));
    }
}
