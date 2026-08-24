use harness::UnwrapOrAbort;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use harness_core::config::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, public_config_contract,
    PublicConfigAliasScope, PublicConfigKeyStatus,
};
use harness_providers::ProviderErrorCategory;
use harness_tools::discover_skill_catalog;

mod common;

use common::repo_root;

const SHIPPED_AGENT_PROFILES: &str = "default explore general librarian";

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn documented_table_keys(doc: &str, heading: &str) -> BTreeSet<String> {
    let mut section = doc
        .split(&format!("## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("abort"));
    if let Some((current, _rest)) = section.split_once("\n## ") {
        section = current;
    }

    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("| `") {
                return None;
            }
            let after_tick = &trimmed[3..];
            let key = after_tick.split('`').next()?;
            Some(key.to_string())
        })
        .collect()
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn read_doc(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|err| {
        let _ = err;
        panic!("abort");
    })
}

fn markdown_table_rows(doc: &str) -> Vec<Vec<String>> {
    doc.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter(|line| !line.contains("|---"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| row.len() >= 2)
        .collect()
}

fn shipped_builtin_skill_entries() -> Vec<(String, String)> {
    let catalog = discover_skill_catalog(&repo_root()).unwrap_or_abort();
    let mut entries = catalog
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.name.as_str(),
                "git-master" | "review-work" | "frontend-ui-ux" | "harness-qa"
            )
        })
        .map(|entry| (entry.name.clone(), entry.stable_id.clone()))
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries.len(),
        4,
        "expected the four V1 built-in skill candidates in the catalog"
    );
    entries
}

#[test]
fn config_docs_runtime_and_tui_keys_match_generated_schemas() {
    // arrange
    // act
    // assert
    let contract = public_config_contract();
    let runtime_schema = harness_schema_pretty_json().unwrap_or_abort();
    let runtime_schema: serde_json::Value = serde_json::from_str(&runtime_schema).unwrap_or_abort();
    let runtime_keys: BTreeSet<String> = runtime_schema["properties"]
        .as_object()
        .unwrap_or_abort()
        .keys()
        .cloned()
        .collect();
    let contract_runtime_keys = contract
        .runtime_schema_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    let tui_schema = harness_tui_schema_pretty_json().unwrap_or_abort();
    let tui_schema: serde_json::Value = serde_json::from_str(&tui_schema).unwrap_or_abort();
    let tui_keys: BTreeSet<String> = tui_schema["properties"]
        .as_object()
        .unwrap_or_abort()
        .keys()
        .cloned()
        .collect();
    let contract_tui_keys = contract
        .tui_schema_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    let doc_path = repo_root().join("docs/configuration/config.md");
    let doc = std::fs::read_to_string(&doc_path).unwrap_or_abort();

    let documented_runtime_keys = documented_table_keys(&doc, "Runtime top-level keys");
    let documented_tui_keys = documented_table_keys(&doc, "TUI top-level keys");
    let contract_documented_runtime_keys = contract
        .runtime_documented_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let contract_documented_tui_keys = contract
        .tui_documented_top_level_keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();

    assert_eq!(
        runtime_keys, contract_runtime_keys,
        "runtime schema drifted from public config contract"
    );
    assert_eq!(
        tui_keys, contract_tui_keys,
        "tui schema drifted from public config contract"
    );
    assert_eq!(
        documented_runtime_keys, contract_documented_runtime_keys,
        "runtime key table drifted from public config contract"
    );
    assert_eq!(
        documented_tui_keys, contract_documented_tui_keys,
        "tui key table drifted from public config contract"
    );
}

#[test]
fn config_docs_document_variable_substitution_and_config_layering() {
    // arrange
    let root = repo_root();

    // act
    let doc = std::fs::read_to_string(root.join("docs/configuration/config.md")).unwrap_or_abort();

    // assert
    assert!(
        doc.contains("## Variable substitution"),
        "docs/config.md must document variable substitution"
    );
    assert!(
        doc.contains("{env:VAR}"),
        "docs/config.md must document {{env:VAR}} syntax"
    );
    assert!(
        doc.contains("{file:path}"),
        "docs/config.md must document {{file:path}} syntax"
    );
    assert!(
        doc.contains("${VAR:-fallback}"),
        "docs/config.md must document ${{VAR:-fallback}} syntax"
    );
    assert!(
        doc.contains("apiKeyEnv"),
        "docs/config.md must distinguish apiKeyEnv from {{env:VAR}}"
    );

    assert!(
        doc.contains("## Config layering"),
        "docs/config.md must document config layering"
    );
    assert!(
        doc.contains("XDG global config"),
        "docs/config.md must document XDG global config discovery"
    );
    assert!(
        doc.contains("Project local config"),
        "docs/config.md must document project local config discovery"
    );
    assert!(
        doc.contains("Agent markdown files"),
        "docs/config.md must document agent markdown file discovery"
    );
    assert!(
        doc.contains("last-wins"),
        "docs/config.md must document last-wins discovery order"
    );
}

#[test]
fn config_contract_semantic_metadata_matches_docs() {
    // arrange
    let contract = public_config_contract();
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/configuration/config.md")).unwrap_or_abort();

    // act
    let runtime_key = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "runtime")
        .unwrap_or_abort();
    assert_eq!(runtime_key.status, PublicConfigKeyStatus::Canonical);
    assert!(doc.contains("| `runtime` | Runtime knobs"));

    let small_model_alias = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "smallModel")
        .unwrap_or_abort();
    assert_eq!(
        small_model_alias.status,
        PublicConfigKeyStatus::Compatibility
    );
    assert_eq!(small_model_alias.canonical_name, Some("small_model"));
    assert!(doc.contains("compatibility aliases"));

    let server = contract
        .runtime_top_level_keys
        .iter()
        .find(|key| key.name == "server")
        .unwrap_or_abort();
    assert_eq!(server.status, PublicConfigKeyStatus::UnsupportedActive);
    assert!(doc.contains("`server`, `command`, `autoshare`"));

    let bash = contract
        .permission_names
        .iter()
        .find(|permission| permission.name == "bash")
        .unwrap_or_abort();
    assert!(bash.canonical);
    assert!(bash.schema_property);
    assert!(bash.supports_selectors);
    assert!(
        doc.contains(
            "`bash`, `edit`, `question`, `task`, `webfetch`, `websearch`, `codesearch`, `lsp`, plus safety kinds `read`, `external_directory`, and `doom_loop`"
        ) || doc.contains(
            "`bash`, `edit`, `question`, `task`,\n`webfetch`, `websearch`, `codesearch`, and `lsp`"
        ),
        "docs/config.md must list canonical permission names"
    );

    let compaction = contract
        .compaction_knobs
        .iter()
        .find(|knob| knob.canonical_name == "fallback_input_tokens")
        .unwrap_or_abort();
    assert_eq!(compaction.default_value, "32768");
    assert!(compaction.aliases.contains(&"fallbackInputTokens"));
    assert!(doc.contains("| `fallbackInputTokens` / `fallback_input_tokens` | `32768` |"));

    let compaction_aliases = contract
        .runtime_aliases
        .iter()
        .filter(|alias| alias.scope == PublicConfigAliasScope::RuntimeCompaction)
        .map(|alias| (alias.alias, alias.canonical))
        .collect::<BTreeSet<_>>();

    // assert
    assert!(compaction_aliases.contains(&("fallbackInputTokens", "fallback_input_tokens")));
}

#[test]
fn config_docs_capture_v1_skill_contract_and_authoring_guide() {
    // arrange
    let root = repo_root();
    let doc = std::fs::read_to_string(root.join("docs/configuration/config.md")).unwrap_or_abort();
    let starter = std::fs::read_to_string(root.join("docs/configuration/starter-skills.md"))
        .unwrap_or_abort();
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap_or_abort();
    let runtime_schema = harness_schema_pretty_json().unwrap_or_abort();
    let runtime_schema: serde_json::Value =
        // act
        serde_json::from_str(&runtime_schema).unwrap_or_abort();

    // assert
    assert!(runtime_schema["definitions"]["SkillsConfig"]["properties"]
        .as_object()
        .unwrap_or_abort()
        .contains_key("disabled"));
    assert_eq!(
        runtime_schema["properties"]["skills"]["default"]["disabled"],
        serde_json::json!([])
    );

    for expected in [
        "## Skill discovery and V1 skill contract",
        "\"disabled\": [\"skill:project:old-skill\", \"experimental-*\"]",
        "V1 discovery never fetches\nremote skills",
        "Unsupported public fields make that skill `malformed`",
        "`body_loaded: false`",
        "Missing, denied, disabled,\nmalformed, and symlink-unsafe skills fail before activation or child spawn",
        "They never grant runtime tools",
        "External editor, assistant, and agent compatibility roots are adapter work",
        "does not search `.external-editor/skills`,\n`.assistant/skills`, `.agents/skills`",
        "explicitly lists those paths in `skills.project_roots` or `skills.global_roots`",
        "V1 root precedence is deterministic",
        "At each ancestor, Harness-owned roots (`.agent-harness/skills`, then\n   `.harness/skills`) are searched before other non-compatibility project roots",
        "When they are listed, they are imported after Harness-owned and other\nnon-compatibility roots",
        "cannot shadow\n`.agent-harness/skills/foo/SKILL.md`, `.harness/skills/foo/SKILL.md`",
        "duplicate compatibility roots resolve\nin their configured order",
    ] {
        assert!(
            doc.contains(expected),
            "docs/config.md missing V1 skill contract anchor: {expected}"
        );
    }

    for expected in [
        "## V1 frontmatter",
        "## Progressive disclosure and governance",
        "purpose",
        "do not use when",
        "execution policy",
        "final checklist",
        "it never grants tools",
        "Compatibility roots from other editors or assistants",
        "ignored unless an operator explicitly adds them to `skills.project_roots` or\n`skills.global_roots`",
        "compatibility roots are searched\nafter Harness-owned and other non-compatibility project/global roots",
        "project compatibility roots win before global compatibility roots",
    ] {
        assert!(
            starter.contains(expected),
            "docs/starter-skills.md missing skill guide anchor: {expected}"
        );
    }

    for expected in [
        "duplicate names load once at their first occurrence",
        "malformed, or symlink-unsafe skills fail the task call before child spawn",
        "`body_loaded: false`",
    ] {
        assert!(
            readme.contains(expected),
            "README.md missing task/skill anchor: {expected}"
        );
    }
}

#[test]
#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn built_in_skill_docs_and_capability_map_cover_catalog_stable_ids() {
    // arrange
    let config = read_doc("docs/configuration/config.md");
    let starter = read_doc("docs/configuration/starter-skills.md");
    let extension = read_doc("docs/operations/extension-strategy.md");
    let starter_rows = markdown_table_rows(&starter);
    let extension_rows = markdown_table_rows(&extension);

    // act
    let built_in_skill_entries = shipped_builtin_skill_entries();

    // assert
    assert!(
        config.contains("disabled skills are catalog-visible but\ncannot be activated through either `skill` or `task(load_skills = [...])`"),
        "config docs must state disabled built-ins fail through both skill and task loading"
    );
    assert!(
        starter.contains("Disable a built-in with `skills.disabled`"),
        "starter skill docs must name the disablement config key"
    );
    assert!(
        extension.contains("## Core runtime behavior vs disableable built-in capabilities"),
        "extension strategy missing core-vs-disableable capability map"
    );

    for (name, stable_id) in built_in_skill_entries {
        let starter_row = starter_rows
            .iter()
            .find(|row| {
                row.first()
                    .is_some_and(|cell| cell.contains(&format!("`{stable_id}`")))
            })
            .unwrap_or_else(|| panic!("abort"));
        assert!(
            starter_row.get(1).is_some_and(|cell| cell.len() > 20),
            "starter skill row for {stable_id} needs a concrete use-when entry"
        );
        assert!(
            starter_row.get(2).is_some_and(|cell| cell.len() > 20),
            "starter skill row for {stable_id} needs a concrete do-not-use-when entry"
        );

        let extension_row = extension_rows
            .iter()
            .find(|row| {
                row.get(2)
                    .is_some_and(|cell| cell.contains(&format!("`{stable_id}`")))
            })
            .unwrap_or_else(|| {
                panic!("extension capability map missing built-in row for {stable_id}")
            });
        assert!(
            extension_row
                .first()
                .is_some_and(|cell| cell.contains(&format!("`{name}`"))),
            "extension capability map should name the skill `{name}`"
        );
        assert!(
            extension_row
                .get(1)
                .is_some_and(|cell| cell == "disableable built-in capability"),
            "extension capability map should classify {stable_id} as disableable"
        );
        assert!(
            extension_row.get(3).is_some_and(|cell| cell == "loadable"),
            "extension capability map should state {stable_id} default state"
        );
    }
}

#[test]
fn v1_release_docs_cover_permissions_extension_privacy_migration_and_provider_support() {
    // arrange
    let required_docs = [
        "docs/permissions/permissions.md",
        "docs/operations/extension-strategy.md",
        "docs/permissions/privacy-and-local-data.md",
        "docs/operations/migration-notes.md",
        "docs/configuration/provider-support.md",
        "docs/testing/budgets.md",
    ];

    for path in required_docs {
        // act
        let doc = read_doc(path);
        // assert
        assert!(
            doc.lines().count() >= 20,
            "{path} is too thin for V1 release docs"
        );
    }

    let permissions = read_doc("docs/permissions/permissions.md");
    for permission in [
        "bash",
        "edit",
        "question",
        "task",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
    ] {
        assert!(
            permissions.contains(&format!("`{permission}`")),
            "permissions doc missing `{permission}`"
        );
    }
    assert!(permissions.contains("operator approval layer, not a sandbox"));
    assert!(permissions.contains("runtime-enforced vs behavioral"));

    let extension = read_doc("docs/operations/extension-strategy.md");
    for seam in [
        "typed extension manifest",
        "command/hook",
        "final-slice",
        "post-V1",
        "config-backed MCP",
        "markdown skills",
    ] {
        assert!(
            extension.contains(seam),
            "extension strategy missing `{seam}`"
        );
    }

    let privacy = read_doc("docs/permissions/privacy-and-local-data.md");
    for topic in [
        "Data egress",
        "Storage paths",
        "Redaction",
        "No telemetry",
        "redact.rs",
        "support export",
    ] {
        assert!(privacy.contains(topic), "privacy doc missing `{topic}`");
    }

    let migration = read_doc("docs/operations/migration-notes.md");
    for unsupported in [
        "HTTP server",
        "web share",
        "plugin host",
        "autoupdate",
        "enterprise",
        "desktop/mobile/PWA",
        "browser/media automation",
        "OAuth MCP",
        "remote collaboration bots",
        "Ralph/continuation loops",
    ] {
        assert!(
            migration.contains(unsupported),
            "migration notes missing `{unsupported}`"
        );
    }

    let provider = read_doc("docs/configuration/provider-support.md");
    for category in ProviderErrorCategory::ALL {
        let variant = format!("{category:?}");
        assert!(
            provider.contains(&variant),
            "provider support doc missing `{variant}`"
        );
        assert!(
            provider.contains(category.as_str()),
            "provider support doc missing serialized `{}`",
            category.as_str()
        );
    }
    assert!(provider.contains("OpenAI-compatible `auto` mode"));
    assert!(provider.contains(
        "Eligible provider failures may also advance through an explicitly configured `model_profile.fallback` chain"
    ));
}

mod extension_strategy_test {
    use super::*;
    include!("config_docs_reference/extension_strategy_test.rs");
}

mod v1_docs_surface_test {
    use super::*;
    include!("config_docs_reference/v1_docs_surface_test.rs");
}

mod sessions_architecture_test {
    use super::*;
    include!("config_docs_reference/sessions_architecture_test.rs");
}
