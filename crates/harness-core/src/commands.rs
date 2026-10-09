//! Markdown prompt commands, discovered from injected project and user roots.
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: Option<String>,
    pub template: String,
}

#[derive(Debug, Clone, Default)]
pub struct CommandDiscovery {
    pub commands: Vec<PromptCommand>,
    pub warnings: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Frontmatter {
    description: String,
    #[serde(rename = "argument-hint")]
    argument_hint: Option<String>,
}

/// Project commands (nearest first), then user commands, then compiled templates.
/// Reserved built-in names and aliases are skipped at every tier.
pub fn discover(cwd: &Path, home: Option<&Path>, reserved: &[&str]) -> CommandDiscovery {
    let mut result = CommandDiscovery::default();
    let mut commands = BTreeMap::new();
    for root in crate::config::search_roots(cwd).into_iter().rev() {
        load_directory(
            &root.join(".harness/commands"),
            reserved,
            &mut commands,
            &mut result.warnings,
        );
    }
    if let Some(home) = home {
        load_directory(
            &home.join("commands"),
            reserved,
            &mut commands,
            &mut result.warnings,
        );
    }
    insert_command(
        "init",
        include_str!("../commands/init.md"),
        "bundled init",
        reserved,
        &mut commands,
        &mut result.warnings,
    );
    result.commands = commands.into_values().collect();
    result
}

fn load_directory(
    directory: &Path,
    reserved: &[&str],
    commands: &mut BTreeMap<String, PromptCommand>,
    warnings: &mut Vec<String>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            warnings.push(format!(
                "Cannot read commands {}: {error}",
                directory.display()
            ));
            return;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => warnings.push(format!(
                "Cannot read command entry in {}: {error}",
                directory.display()
            )),
        }
    }
    paths.sort();
    for path in paths {
        if path.extension().is_none_or(|extension| extension != "md") || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            warnings.push(format!(
                "Skipping command with invalid name: {}",
                path.display()
            ));
            continue;
        };
        match fs::read_to_string(&path) {
            Ok(content) => insert_command(
                name,
                &content,
                &path.display().to_string(),
                reserved,
                commands,
                warnings,
            ),
            Err(error) => warnings.push(format!("Cannot read command {}: {error}", path.display())),
        }
    }
}

fn insert_command(
    name: &str,
    content: &str,
    source: &str,
    reserved: &[&str],
    commands: &mut BTreeMap<String, PromptCommand>,
    warnings: &mut Vec<String>,
) {
    if !valid_name(name) {
        warnings.push(format!("Skipping command with invalid name: {source}"));
        return;
    }
    if reserved.contains(&name) {
        warnings.push(format!(
            "Skipping /{name} from {source}: reserved by a built-in command"
        ));
        return;
    }
    if commands.contains_key(name) {
        return;
    }
    match parse(name, content) {
        Ok(command) => {
            commands.insert(name.to_owned(), command);
        }
        Err(error) => warnings.push(format!("Skipping command {source}: {error}")),
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

fn parse(name: &str, content: &str) -> Result<PromptCommand, String> {
    let mut metadata = Frontmatter::default();
    let mut body = content;
    let mut lines = content.split_inclusive('\n');
    if lines.next().is_some_and(|line| line.trim_end() == "---") {
        let start = content.find('\n').map_or(content.len(), |at| at + 1);
        let mut end = start;
        let mut closing = None;
        for line in lines {
            if line.trim_end() == "---" {
                closing = Some((end, end + line.len()));
                break;
            }
            end += line.len();
        }
        let (end, body_start) = closing.ok_or("unterminated YAML frontmatter")?;
        metadata =
            serde_yaml_ng::from_str(&content[start..end]).map_err(|error| error.to_string())?;
        body = &content[body_start..];
    }
    Ok(PromptCommand {
        name: name.to_owned(),
        description: metadata.description,
        argument_hint: metadata.argument_hint,
        template: body.trim().to_owned(),
    })
}

/// Returns an expansion only when the input starts with a known slash command.
pub fn expand_input(commands: &[PromptCommand], input: &str) -> Option<String> {
    let expression = input.trim().strip_prefix('/')?;
    let (name, arguments) = expression
        .split_once(char::is_whitespace)
        .unwrap_or((expression, ""));
    commands
        .iter()
        .find(|command| command.name == name)
        .map(|command| command.expand(arguments))
}

impl PromptCommand {
    pub fn expand(&self, arguments: &str) -> String {
        let arguments = arguments.trim();
        let mut positional = None;
        let mut output = String::with_capacity(self.template.len() + arguments.len());
        let mut remaining = self.template.as_str();
        let mut placeholder = false;
        while let Some(at) = remaining.find('$') {
            output.push_str(&remaining[..at]);
            remaining = &remaining[at..];
            let (value, consumed) = if remaining.starts_with("$ARGUMENTS") {
                (arguments, 10)
            } else if remaining.starts_with("$@") {
                (arguments, 2)
            } else if let Some(digit @ b'1'..=b'9') = remaining.as_bytes().get(1) {
                let positional = positional.get_or_insert_with(|| positional_arguments(arguments));
                (
                    positional
                        .get(usize::from(*digit - b'1'))
                        .map_or("", String::as_str),
                    2,
                )
            } else {
                output.push('$');
                remaining = &remaining[1..];
                continue;
            };
            placeholder = true;
            output.push_str(value);
            remaining = &remaining[consumed..];
        }
        output.push_str(remaining);
        if !placeholder && !arguments.is_empty() {
            output.push_str("\n\n");
            output.push_str(arguments);
        }
        output
    }
}

fn positional_arguments(arguments: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut value = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = arguments.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' if quote != Some('\'') => {
                if let Some(next) = chars.next() {
                    value.push(next);
                } else {
                    value.push(character);
                }
                started = true;
            }
            '\'' | '"' if quote == Some(character) => {
                quote = None;
            }
            '\'' | '"' if quote.is_none() => {
                quote = Some(character);
                started = true;
            }
            character if character.is_whitespace() && quote.is_none() => {
                if started {
                    values.push(std::mem::take(&mut value));
                    started = false;
                }
            }
            character => {
                value.push(character);
                started = true;
            }
        }
    }
    if started {
        values.push(value);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_commands_discover_and_expand() -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let project = temporary.path().join("project");
        let cwd = project.join("nested");
        let home = temporary.path().join("home");
        for directory in [
            project.join(".git"),
            project.join(".harness/commands"),
            cwd.join(".harness/commands"),
            home.join("commands"),
        ] {
            fs::create_dir_all(directory)?;
        }
        for (root, name, content) in [
            (&project, "shared", "ancestor"),
            (&cwd, "shared", "nearest"),
            (&project, "ancestor", "ancestor only"),
            (&cwd, "BAD", "invalid"),
            (&cwd, "login", "reserved"),
            (&cwd, "sign-in", "reserved alias"),
            (&cwd, "init", "project init"),
            (&cwd, "0-valid_name", "valid"),
        ] {
            fs::write(
                root.join(".harness/commands").join(format!("{name}.md")),
                content,
            )?;
        }
        fs::write(home.join("commands/shared.md"), "home")?;
        fs::write(
            home.join("commands/user.md"),
            "---\ndescription: User command\nargument-hint: '<name>'\n---\nHello $1",
        )?;
        fs::create_dir_all(cwd.join(".harness/commands/deep"))?;
        fs::write(cwd.join(".harness/commands/deep/hidden.md"), "hidden")?;
        fs::create_dir_all(temporary.path().join(".harness/commands"))?;
        fs::write(
            temporary.path().join(".harness/commands/outside.md"),
            "outside",
        )?;
        let found = discover(&cwd, Some(&home), &["login", "sign-in"]);
        for (name, expected) in [
            ("shared", "nearest"),
            ("ancestor", "ancestor only"),
            ("init", "project init"),
            ("0-valid_name", "valid"),
            ("user", "Hello $1"),
        ] {
            assert_eq!(
                found
                    .commands
                    .iter()
                    .find(|command| command.name == name)
                    .map(|command| command.template.as_str()),
                Some(expected)
            );
        }
        for name in ["BAD", "login", "sign-in", "hidden", "outside"] {
            assert!(!found.commands.iter().any(|command| command.name == name));
        }
        assert_eq!(found.warnings.len(), 3);
        let user = found
            .commands
            .iter()
            .find(|command| command.name == "user")
            .ok_or("missing user command")?;
        assert_eq!(
            (&*user.description, user.argument_hint.as_deref()),
            ("User command", Some("<name>"))
        );
        assert!(discover(temporary.path(), None, &[])
            .commands
            .iter()
            .any(|command| command.name == "init"));
        for (template, args, expected) in [
            (
                "All: $ARGUMENTS / $@",
                "  one two  ",
                "All: one two / one two",
            ),
            (
                "$1 + $2",
                "'one two' \"three four\"",
                "one two + three four",
            ),
            ("$1/$2/$9", "one", "one//"),
            ("Plain", " words ", "Plain\n\nwords"),
            ("Plain", "", "Plain"),
            ("$1/$2", "\"\" end", "/end"),
            ("$1", "'$2'", "$2"),
        ] {
            let command = PromptCommand {
                name: "hi".into(),
                description: String::new(),
                argument_hint: None,
                template: template.into(),
            };
            assert_eq!(
                expand_input(&[command], &format!("/hi {args}")),
                Some(expected.to_owned())
            );
        }
        Ok(())
    }
}
