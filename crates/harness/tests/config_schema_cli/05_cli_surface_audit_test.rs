fn run_help(args: &[&str]) -> String {
    let output = harness_command()
        .args(args.iter().copied())
        .output()
        .unwrap_or_else(|err| panic!("run harness {args:?}: {err}"));
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help output is utf-8")
}

fn command_rows(help: &str) -> Vec<(String, String)> {
    let Some(commands_section) = help.split("Commands:\n").nth(1) else {
        return Vec::new();
    };
    commands_section
        .lines()
        .take_while(|line| !line.trim_start().starts_with("Options:"))
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                return None;
            }
            let mut parts = trimmed.splitn(2, char::is_whitespace);
            let name = parts.next()?.to_string();
            let description = parts.next().unwrap_or_default().trim().to_string();
            Some((name, description))
        })
        .collect()
}

fn assert_help_has_complete_command_descriptions(args: &[&str]) {
    // arrange
    let help = run_help(args);
    let rows = command_rows(&help);

    // act/assert
    assert!(!rows.is_empty(), "harness {args:?} help has no command rows");
    for (name, description) in rows {
        assert!(
            !description.is_empty(),
            "harness {args:?} command `{name}` has an empty help description:\n{help}"
        );
        let lower = description.to_ascii_lowercase();
        assert!(
            !lower.contains("todo") && !lower.contains("tbd") && !lower.contains("placeholder"),
            "harness {args:?} command `{name}` has placeholder help: {description}"
        );
    }
}

fn extract_harness_command_paths(readme: &str) -> std::collections::BTreeSet<Vec<String>> {
    let root_commands = [
        "tui", "run", "doctor", "models", "prompt", "replay", "sessions", "schema", "config",
    ];
    let nested_commands = [
        "generate",
        "generated",
        "probe",
        "list",
        "inspect",
        "reopen",
        "replay",
        "continue",
        "export",
        "tree",
        "fork",
        "clone",
        "validate",
    ];
    let global_options_with_values = ["--config", "--session-dir", "-p"];
    let mut paths = std::collections::BTreeSet::new();

    for line in readme.lines() {
        let mut rest = line;
        while let Some(index) = rest.find("harness") {
            rest = &rest[index + "harness".len()..];
            let Some(next) = rest.chars().next() else {
                break;
            };
            if !next.is_whitespace() {
                continue;
            }

            let mut path = Vec::new();
            let mut skip_next = false;
            for raw in rest.split_whitespace() {
                let token = raw.trim_matches(|ch: char| {
                    matches!(ch, '`' | ',' | '.' | ';' | ':' | ')' | '(' | '\\')
                });
                if token.is_empty() || token == "--" {
                    continue;
                }
                if skip_next {
                    skip_next = false;
                    continue;
                }
                if global_options_with_values.contains(&token) {
                    skip_next = true;
                    continue;
                }
                if token.starts_with('-') {
                    continue;
                }
                if path.is_empty() {
                    if root_commands.contains(&token) {
                        path.push(token.to_string());
                    }
                    break;
                }
            }

            if let Some(first) = path.first().cloned() {
                let after_first = rest
                    .split_whitespace()
                    .skip_while(|raw| {
                        raw.trim_matches(|ch: char| {
                            matches!(ch, '`' | ',' | '.' | ';' | ':' | ')' | '(' | '\\')
                        }) != first
                    })
                    .nth(1)
                    .map(|raw| {
                        raw.trim_matches(|ch: char| {
                            matches!(ch, '`' | ',' | '.' | ';' | ':' | ')' | '(' | '\\')
                        })
                    });
                if let Some(second) = after_first {
                    if nested_commands.contains(&second) {
                        path.push(second.to_string());
                    }
                }
            }

            paths.insert(path);
        }
    }

    paths
}

#[test]
fn cli_help_lists_non_placeholder_command_descriptions() {
    // arrange
    let command_surfaces = [
        &["--help"] as &[&str],
        &["config", "--help"],
        &["sessions", "--help"],
        &["models", "--help"],
    ];

    // act
    let checked = command_surfaces.len();

    // assert
    for surface in command_surfaces {
        assert_help_has_complete_command_descriptions(surface);
    }
    assert_eq!(checked, 4);
}

#[test]
fn readme_command_audit_resolves_to_real_subcommands() {
    // arrange
    let readme = fs::read_to_string(repo_root().join("README.md")).expect("read README.md");

    // act
    let documented = extract_harness_command_paths(&readme);

    // assert
    let required = [
        vec!["config", "validate"],
        vec!["doctor"],
        vec!["prompt"],
        vec!["sessions", "export"],
        vec!["sessions", "inspect"],
        vec!["sessions", "tree"],
        vec!["sessions", "fork"],
        vec!["sessions", "clone"],
    ];
    for path in required {
        let path = path.into_iter().map(str::to_string).collect::<Vec<_>>();
        assert!(
            documented.contains(&path),
            "README.md no longer documents required harness command `{}`",
            path.join(" ")
        );
    }

    for path in documented {
        let mut args = path.iter().map(String::as_str).collect::<Vec<_>>();
        args.push("--help");
        run_help(&args);
    }
}
