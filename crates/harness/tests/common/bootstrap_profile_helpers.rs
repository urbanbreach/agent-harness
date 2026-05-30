fn snapshot_model_target() -> ResolvedModelTarget {
    ResolvedModelTarget {
        model_ref: "default/gpt-5.4-mini".to_string(),
        provider: "default".to_string(),
        model: "gpt-5.4-mini".to_string(),
        variant: Some("high".to_string()),
        reasoning_effort: Some("high".to_string()),
        text_verbosity: None,
        reasoning_summary: None,
    }
}

fn snapshot_workspace_environment() -> WorkspaceEnvironment {
    WorkspaceEnvironment {
        working_directory: PathBuf::from("/workspace/agent-harness"),
        workspace_root: PathBuf::from("/workspace/agent-harness"),
        is_git_repository: true,
        git_branch: Some("dev".to_string()),
    }
}

fn assert_snapshot_text(path: &Path, actual: &str) {
    let actual = snapshot_text(actual);
    assert!(
        !actual.trim().is_empty(),
        "prompt snapshot {} must not be empty",
        path.display()
    );

    if prompt_snapshot_update_enabled() {
        fs::create_dir_all(path.parent().expect("snapshot parent"))
            .expect("create prompt snapshot dir");
        fs::write(path, actual)
            .unwrap_or_else(|err| panic!("write prompt snapshot {}: {err}", path.display()));
        return;
    }

    let expected = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!(
            "read prompt snapshot {}; set {UPDATE_PROMPT_SNAPSHOTS_ENV}=1 to regenerate: {err}",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "prompt snapshot drifted: {}",
        path.display()
    );
}

fn assert_snapshot_dir_contains_exact_files(snapshot_dir: &Path, expected_files: &[String]) {
    let mut expected_files = expected_files.to_vec();
    expected_files.sort();
    let mut actual_files = if snapshot_dir.exists() {
        fs::read_dir(snapshot_dir)
            .unwrap_or_else(|err| panic!("read snapshot dir {}: {err}", snapshot_dir.display()))
            .map(|entry| {
                entry
                    .expect("snapshot dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .filter(|file_name| file_name.ends_with(".txt"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    actual_files.sort();
    assert_eq!(
        actual_files,
        expected_files,
        "snapshot dir {} must contain exactly the expected prompt snapshots",
        snapshot_dir.display()
    );
}

fn snapshot_text(actual: &str) -> String {
    let mut text = actual.trim_end().to_string();
    text.push('\n');
    text
}

fn prompt_snapshot_update_enabled() -> bool {
    std::env::var_os(UPDATE_PROMPT_SNAPSHOTS_ENV).is_some_and(|value| value == "1")
}

fn normalize_composed_prompt_snapshot(prompt: &str) -> String {
    let prompt = prompt
        .lines()
        .map(|line| {
            if line.starts_with("Instructions from: ") && line.ends_with("AGENTS.md") {
                "Instructions from: /workspace/agent-harness/AGENTS.md"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let Some(start) = prompt.find("Here is some useful information about the environment") else {
        return prompt.to_string();
    };
    let Some(end_offset) = prompt[start..].find("</env>") else {
        return prompt.to_string();
    };
    let end = start + end_offset + "</env>".len();
    let fixed_env = "Here is some useful information about the environment you are running in:\n<env>\n  Working directory: /workspace/agent-harness\n  Workspace root folder: /workspace/agent-harness\n  Is directory a git repo: yes\n  Git branch: dev\n  Platform: linux\n  Today's date: Fri May 29 2026\n</env>";
    let mut normalized = String::with_capacity(prompt.len());
    normalized.push_str(&prompt[..start]);
    normalized.push_str(fixed_env);
    normalized.push_str(&prompt[end..]);
    normalized
}

fn shipped_v1_prompt_asset_snapshot(repo_root: &Path) -> serde_json::Value {
    let mut profiles = serde_json::Map::new();
    for profile in V1_PROMPT_PROFILES.split_whitespace() {
        let asset_path = repo_root
            .join(".agent-harness")
            .join("agents")
            .join(format!("{profile}.md"));
        let markdown = fs::read_to_string(&asset_path)
            .unwrap_or_else(|err| panic!("read prompt asset {}: {err}", asset_path.display()));
        let body = prompt_body_from_markdown(&markdown);
        let digest12 = blake3::hash(body.as_bytes())
            .to_hex()
            .chars()
            .take(12)
            .collect::<String>();
        let sections = body
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .map(str::to_string)
            .collect::<Vec<_>>();
        profiles.insert(
            profile.to_string(),
            serde_json::json!({
                "digest12": digest12,
                "line_count": body.lines().count(),
                "sections": sections,
            }),
        );
    }

    serde_json::json!({
        "schema_version": "v1-prompt-assets-snapshot-v1",
        "profiles": profiles,
    })
}

fn prompt_body_from_markdown(markdown: &str) -> String {
    let mut lines = markdown.lines();
    if lines.next() == Some("---") {
        for line in &mut lines {
            if line == "---" {
                break;
            }
        }
        return lines.collect::<Vec<_>>().join("\n").trim().to_string();
    }
    markdown.trim().to_string()
}

fn shipped_profile_body(repo_root: &Path, profile: &str) -> String {
    let asset_path = repo_root
        .join(".agent-harness")
        .join("agents")
        .join(format!("{profile}.md"));
    let markdown = fs::read_to_string(&asset_path)
        .unwrap_or_else(|err| panic!("read prompt asset {}: {err}", asset_path.display()));
    prompt_body_from_markdown(&markdown)
}

fn coordinator_denies_tool_for_profile(
    coordinator_config: &harness_core::coord::CoordinatorConfig,
    profile_name: &str,
    tool_id: &str,
) -> bool {
    let profile = &coordinator_config.agent_profiles[profile_name];
    if !profile.toolset.iter().any(|tool| tool == tool_id) {
        return true;
    }

    let Some(tool) = coordinator_config.tool_registry.get(tool_id) else {
        return true;
    };
    let Some(kind) = permission_kind_for_tool_call(tool_id, tool.capability()) else {
        return false;
    };

    !matches!(
        coordinator_config
            .permission_policy
            .evaluate(Some(profile_name), kind),
        PolicyDecision::Allow
    )
}

fn task_description_for_profile(registry: &ToolRegistry, profile: &AgentProfile) -> String {
    build_provider_tool_defs(profile, registry)
        .expect("tool defs")
        .into_iter()
        .find(|tool| tool.tool_id == "task")
        .expect("task tool")
        .description
        .expect("task description")
}

fn prompt_section(body: &str, heading: &str) -> String {
    let marker = format!("## {heading}\n");
    let section = body
        .split(&marker)
        .nth(1)
        .unwrap_or_else(|| panic!("missing prompt section {heading}"));
    section
        .split("\n## ")
        .next()
        .unwrap_or(section)
        .trim()
        .to_string()
}
