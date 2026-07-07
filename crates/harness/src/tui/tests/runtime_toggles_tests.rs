use super::*;
use harness::UnwrapOrAbort;

#[test]
fn runtime_toggles_report_compact_skill_catalog_states() {
    // arrange
    let workspace = tempfile::tempdir().unwrap_or_abort();
    fs::create_dir_all(workspace.path().join(".git")).unwrap_or_abort();
    for (name, body) in [
        ("ready-skill", "READY SKILL BODY SENTINEL"),
        ("disabled-skill", "DISABLED SKILL BODY SENTINEL"),
    ] {
        let skill_dir = workspace.path().join(".agent-harness/skills").join(name);
        fs::create_dir_all(&skill_dir).unwrap_or_abort();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} description\n---\n\n{body}\n"),
        )
        .unwrap_or_abort();
    }
    let config = load_config_from_str(
        r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: { "gpt-5.4-mini": { display_name: "GPT-5.4 Mini" } }
            }
          },
          agents: {
            build: {
              description: "Implementation",
              system_prompt: "Implement carefully.",
              model_ref: "default:gpt-5.4-mini",
              tools: ["skill"]
            }
          },
          default_agent: "build",
          permissions: { defaults: { edit: "allow", shell: "allow", network: "allow" } },
          runtime: { session_dir: ".agent-harness/sessions" },
          integrations: { remote_search: { endpoint: "https://mcp.exa.ai/mcp" } },
          skills: {
            project_roots: [".agent-harness/skills"],
            global_roots: [],
            disabled: ["disabled-skill"]
          }
        }
        "#,
    )
    .unwrap_or_abort();

    let toggles = runtime_toggles_config(Some(&config), workspace.path());
    let ready = toggles
        .entries
        .iter()
        // act
        .find(|entry| {
            // assert
            matches!(&entry.kind, ToggleEntryKind::AgentSkill { agent, skill }
                if agent == "build" && skill == "skill:project:ready-skill")
        })
        .unwrap_or_abort();
    assert_eq!(ready.label, "build: ready-skill");
    assert!(ready.description.contains("loadable skill `ready-skill`"));
    assert!(ready.description.contains("project root"));
    assert!(ready.enabled);

    let disabled = toggles
        .entries
        .iter()
        .find(|entry| {
            matches!(&entry.kind, ToggleEntryKind::AgentSkill { agent, skill }
                if agent == "build" && skill == "skill:project:disabled-skill")
        })
        .unwrap_or_abort();
    assert_eq!(disabled.label, "build: disabled-skill");
    assert!(disabled
        .description
        .contains("disabled skill `disabled-skill`"));
    assert!(disabled.description.contains("disabled by skills.disabled"));
    assert!(!disabled.enabled);

    let rendered = format!("{toggles:?}");
    assert!(!rendered.contains("READY SKILL BODY SENTINEL"));
    assert!(!rendered.contains("DISABLED SKILL BODY SENTINEL"));
}
