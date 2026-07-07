use harness::UnwrapOrAbort;
#[test]
fn first_run_provider_auth_docs_do_not_assume_loopback_only() {
    // arrange
    let readme = read_doc("README.md");
    let config = read_doc("docs/config.md");

    // act
    let readme_anchors = [
        "real provider first run",
        "openai-codex",
        r#"authProvider: "codex""#,
        "OPENAI_API_KEY",
        "Codex OAuth-backed request\npath",
        "doctor does not prove live provider authentication or transport health",
    ];
    let config_anchors = [
        "### First-run provider authentication",
        "openai-codex",
        r#"authProvider: "codex""#,
        "Codex OAuth by default",
        "OPENAI_API_KEY",
        "doctor checks that the named environment variable is present",
        "doctor does not prove live provider authentication or transport health",
    ];

    // assert
    for expected in readme_anchors {
        assert!(
            readme.contains(expected),
            "README.md missing first-run provider/auth anchor: {expected}"
        );
    }
    for expected in config_anchors {
        assert!(
            config.contains(expected),
            "docs/config.md missing first-run provider/auth anchor: {expected}"
        );
    }
}

#[test]
fn model_prompt_tuning_stance_is_documented_for_v1() {
    // arrange
    let config = read_doc("docs/config.md");

    // act
    let expected_anchors = [
        "## V1 model prompt tuning stance",
        "Provider-family prompt selection is routed through the explicit model-resolution\nseam",
        "`harness_core::model_resolution`",
        "`crates/harness/src/dynamic_prompt.rs`",
        "rather than scattered raw `model_id.contains(...)` checks",
        "golden prompt tests",
    ];

    // assert
    for expected in expected_anchors {
        assert!(
            config.contains(expected),
            "docs/config.md missing V1 model prompt tuning stance anchor: {expected}"
        );
    }
}

#[test]
fn reference_prompt_patterns_map_to_harness_seams() {
    // arrange
    let architecture = read_doc("docs/architecture.md");
    let mut section = architecture
        .split("### Prompt reference seam map\n")
        .nth(1)
        .unwrap_or_abort();
    if let Some((current, _rest)) = section.split_once("\n### ") {
        section = current;
    }
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    // act
    let rows = markdown_table_rows(section);

    // assert
    assert!(
        section.contains("user-observable Harness behavior"),
        "seam map must state reference behavior is copied as product behavior"
    );
    assert!(
        section.contains("not by copying source architecture"),
        "seam map must reject copying source architecture"
    );

    for (pattern, seam_anchor, status_anchor) in [
        (
            "Intent-gate before tool use",
            "dynamic_prompt.rs",
            "Shipped",
        ),
        (
            "Structured delegation reminder",
            "delegation_reminder",
            "WS9",
        ),
        (
            "Category-specific routing and prompt appends",
            "agent_catalog",
            "profiles",
        ),
        (
            "Markdown-defined skills with progressive disclosure",
            "skill_catalog",
            "built-in skill",
        ),
        (
            "Disableable built-in capabilities",
            "SkillCatalogStatus::Disabled",
            "descriptor-only metadata",
        ),
        (
            "Command/hook lifecycle maps",
            "extension-strategy.md",
            "unsupported/post-V1",
        ),
    ] {
        let row = rows
            .iter()
            .find(|row| row.first().is_some_and(|cell| cell == pattern))
            .unwrap_or_else(|| panic!("seam map missing pattern `{pattern}`"));
        assert!(
            row.get(1).is_some_and(|cell| cell.contains(seam_anchor)),
            "seam map row `{pattern}` missing seam anchor `{seam_anchor}`"
        );
        assert!(
            row.get(2).is_some_and(|cell| cell.contains(status_anchor)),
            "seam map row `{pattern}` missing status anchor `{status_anchor}`"
        );
    }
}

#[test]
fn built_in_capability_order_and_state_policy_are_documented_and_guarded() {
    // arrange
    let extension = read_doc("docs/extension-strategy.md");
    let capability_section = extension
        .split("## Core runtime behavior vs disableable built-in capabilities\n")
        .nth(1)
        .unwrap_or_abort()
        .split("\n## Built-in capability order and state policy")
        .next()
        .unwrap_or_abort();
    let rows = markdown_table_rows(capability_section)
        .into_iter()
        .filter(|row| row.first().is_none_or(|cell| cell != "Surface"))
        .collect::<Vec<_>>();

    // act
    let expected_core_order = [
        "Coordinator event append, scheduling, permissions, lifecycle",
        "Native tool registry",
        "Agent profile prompts",
    ];
    let built_in_skill_entries = shipped_builtin_skill_entries();

    // assert
    assert_eq!(
        rows.len(),
        expected_core_order.len() + built_in_skill_entries.len(),
        "capability map should list core rows followed by the shipped built-in skills"
    );
    for (index, expected) in expected_core_order.iter().enumerate() {
        assert_eq!(
            rows[index].first().map(String::as_str),
            Some(*expected),
            "core capability order drifted at row {index}"
        );
    }

    for ((name, stable_id), row) in built_in_skill_entries.iter().zip(rows.iter().skip(3)) {
        assert!(
            row.first()
                .is_some_and(|cell| cell.contains(&format!("`{name}`"))),
            "built-in skill capability rows should be sorted by stable id/name; expected {name}"
        );
        assert!(
            row.get(2)
                .is_some_and(|cell| cell.contains(&format!("`{stable_id}`"))),
            "built-in capability row for {name} should include stable id {stable_id}"
        );
    }

    for anchor in [
        "Order is intentional where it affects runtime behavior",
        "permission checks own authority before native tool registration",
        "native tool registration owns tool ids before agent prompt assembly",
        "compaction consumes replay-derived event/tool context",
        "skill activation still respects the operator-requested `load_skills` order",
        "V1 disableable built-in skills write no JSONL or artifact state by themselves",
        "schema_version",
        "migration policy",
        "replay behavior",
        "event logs in `docs/architecture.md` and `docs/sessions-and-replay.md`",
        "simulation artifacts in `docs/testing.md`",
        "perf/PTY artifacts in `docs/budgets.md` and `docs/testing.md`",
    ] {
        assert!(
            extension.contains(anchor),
            "extension strategy missing built-in order/state anchor: {anchor}"
        );
    }
}

#[test]
fn thin_v1_docs_cover_their_source_surfaces() {
    // arrange
    let agents = read_doc("docs/agents-and-subagents.md");
    let sessions = read_doc("docs/sessions-and-replay.md");
    let native = read_doc("docs/native-tool-catalog.md");
    let troubleshooting = read_doc("docs/troubleshooting.md");

    // act
    let profiles = V1_PROMPT_PROFILES
        .split_whitespace()
        .chain(["title", "summary", "compaction"])
        .collect::<Vec<_>>();

    // assert
    for profile in profiles {
        assert!(
            agents.contains(&format!("`{profile}`")),
            "agents doc missing `{profile}`"
        );
    }
    for field in [
        "context",
        "goal",
        "downstream use",
        "request",
        "required tools",
        "must-do",
        "must-not-do",
    ] {
        assert!(
            agents.contains(field),
            "agents doc missing structured delegation field `{field}`"
        );
    }
    for command in [
        "list", "inspect", "replay", "continue", "export", "tree", "fork", "clone",
    ] {
        assert!(
            sessions.contains(command),
            "sessions doc missing command `{command}`"
        );
    }
    for lineage in [
        "summary",
        "artifact",
        "fork",
        "clone",
        "source cutoff",
        "meaningful title",
    ] {
        assert!(
            sessions.contains(lineage),
            "sessions doc missing lineage topic `{lineage}`"
        );
    }
    for topic in [
        "timeout",
        "output cap",
        "blocked command",
        "ast_grep_search",
        "ast_grep_replace",
        "Defaults to dry-run",
    ] {
        assert!(
            native.contains(topic),
            "native tool catalog missing `{topic}`"
        );
    }
    for topic in [
        "Missing credentials",
        "Invalid credentials",
        "rate limits",
        "Base URL",
        "Missing MCP",
        "resume",
        "terminal rendering",
        "permission",
    ] {
        assert!(
            troubleshooting.contains(topic),
            "troubleshooting doc missing `{topic}`"
        );
    }
}
