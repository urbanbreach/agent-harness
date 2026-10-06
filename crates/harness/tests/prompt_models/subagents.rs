use super::*;
use harness_providers::MessageRole;

#[tokio::test]
async fn bundled_agents_execute_with_their_roles_models_and_spawn_restrictions(
) -> Result<(), Box<dyn std::error::Error>> {
    for (role, model, eval_permitted) in [
        ("reviewer", "zai-glm-5-3", true),
        ("task", "gpt-6", true),
        ("task", "zai-glm-5-3", true),
        ("scout", "gpt-5.6", true),
        ("task", "gpt-6", false),
        ("security-reviewer", "gpt-6", true),
        ("sonic", "gpt-5.6", true),
    ] {
        let root = tempfile::tempdir()?;
        let user = root.path().join("user/harness/prompts/models");
        let project = root.path().join(".agent-harness/prompts/models");
        fs::create_dir_all(&user)?;
        fs::create_dir_all(&project)?;
        for (directory, preset) in [
            (&user, "gpt-5.6"),
            (&project, "gpt-6"),
            (&project, "glm-5.3"),
        ] {
            fs::write(
                directory.join(format!("{preset}.md")),
                format!("${{% extends \"system.md\" %}}${{% block model_guidance %}}OVERRIDE_{preset}${{% endblock %}}"),
            )?;
        }
        if role != "task" {
            fs::write(root.path().join("task.txt"), "fixture")?;
        }
        fs::write(root.path().join("evidence.txt"), "second evidence file")?;
        fs::write(root.path().join("fixture.json"), json!({
            "provider":{"local":{"type":"openai_compatible","models":{
                "gpt-6":{}, "gpt-5.6":{}, "zai-glm-5-3":{}
            }}},
            "model":"local/gpt-6", "small_model":"local/gpt-5.6",
            "subagents":{"models":{(role):format!("local/{model}")}},
            "agent":{"scout":{"system_prompt":"Wrong generic scout prompt.","mode":"subagent","tools":[]}},
            "permission":{"*":"allow", "eval":if eval_permitted { "allow" } else { "deny" }},
            "eval":{"route_tools":["read"],"sandbox":{"enabled":true}},
            "instructions":"Keep the shared project contract."
        }).to_string())?;
        let mut script = Vec::new();
        let mut children = Vec::new();
        script.push(call("spawn_subagent", json!({"prompt":"Complete the assigned check.","description":role,"subagent_type":role,"background":false})));
        children.push((script.len(), role, model));
        match role {
            "task" | "sonic" => script.push(call(
                "write",
                json!({"path":format!("{role}.txt"),"content":role}),
            )),
            "scout" => {
                script.push(call(
                    "write",
                    json!({"path":"scout-escape.txt","content":"forbidden"}),
                ));
                script.push(call("read", json!({"path":"task.txt"})));
            }
            "reviewer" => {
                script.push(call("spawn_subagent", json!({"prompt":"must be rejected","description":"forbidden worker","subagent_type":"task","background":false})));
                script.push(call("spawn_subagent", json!({"prompt":"Read task.txt","description":"review evidence","background":false})));
                children.push((script.len(), "scout", "gpt-5.6"));
                script.push(call("read", json!({"path":"task.txt"})));
                script.push(done(
                    "{\"summary\":\"Found task\",\"files\":[],\"architecture\":\"fixture\"}",
                ));
                script.push(call(
                    "eval",
                    json!({
                        "language":"js", "isolate":true, "summary":"Check reviewer eval is denied",
                        "code":"print('unexpected reviewer execution');"
                    }),
                ));
            }
            "security-reviewer" => script.push(call(
                "bash",
                json!({"command":"touch security-escape.txt","workdir":root.path()}),
            )),
            _ => return Err("unknown fixture role".into()),
        }
        let uses_eval = eval_permitted && matches!(role, "task" | "sonic");
        if uses_eval {
            // QuickJS exercises the real child tool bridge without requiring Node.
            script.push(call("eval", json!({
                "language":"js", "isolate":true, "summary":"Read independent evidence",
                "code":r#"var results = await Promise.allSettled(['task.txt', 'evidence.txt'].map(path => tool.read({path})));
for (var result of results) display(result.status === 'fulfilled' ? result.value : {error: String(result.reason)});
if (results.some(result => result.status !== 'fulfilled' || result.value.hasError)) throw new Error('batch failed');
print('batch complete');"#
            })));
        } else if role == "task" {
            script.push(call("read", json!({"path":"evidence.txt"})));
        }
        script.push(done("{\"summary\":\"Child complete\"}"));
        script.push(done("All assignments complete."));
        let provider = Arc::new(MockProvider::script(script));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "fixture.json",
                "prompt",
                "--text",
                "Exercise the bundled agents.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .without_env("HOME")
                .with_env(
                    "XDG_CONFIG_HOME",
                    root.path().join("user").to_string_lossy(),
                )
                .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        if matches!(role, "task" | "sonic") {
            assert_eq!(
                fs::read_to_string(root.path().join(format!("{role}.txt")))?,
                role
            );
        }
        assert!(!root.path().join("scout-escape.txt").exists());
        assert!(!root.path().join("security-escape.txt").exists());
        let requests = provider.captured_requests().await;
        for (index, role, model) in children {
            let request = requests
                .get(index)
                .ok_or("child provider request missing")?;
            assert_eq!(request.model_id, model, "wrong model for {role}");
            let system = &request.messages[0].content;
            assert!(
                system.contains("§ Assignment role"),
                "missing role for {role}"
            );
            assert!(system.contains("# 5. Hand-off"));
            assert!(system.contains("Keep the shared project contract."));
            let preset = if model == "zai-glm-5-3" {
                "glm-5.3"
            } else {
                model
            };
            assert!(system.contains(&format!("OVERRIDE_{preset}")), "{system}");
            assert!(!system.contains("Wrong generic scout prompt."));
            let role_contract = match role {
                "scout" => "Investigate the codebase rapidly.",
                "reviewer" => "Find bugs author wants fixed before merge.",
                "security-reviewer" => "Review assigned repository scope only.",
                "sonic" => "Only strictly mechanical updates or data collection.",
                _ => "Worker agent: delegated tasks.",
            };
            assert!(
                system.contains(role_contract),
                "missing {role} instructions"
            );
            let tools = request.tools.as_deref().unwrap_or_default();
            let eval = tools.iter().find(|tool| tool.tool_id == "eval");
            let child_uses_eval = eval_permitted && matches!(role, "task" | "sonic");
            assert_eq!(
                eval.is_some(),
                child_uses_eval,
                "wrong eval access for {role}"
            );
            assert_eq!(
                tools.iter().any(|tool| tool.tool_id == "read"),
                !child_uses_eval
            );
            let dialect = if model == "zai-glm-5-3" {
                "<eval_routing>"
            } else {
                "# Eval routing for GPT"
            };
            if let Some(eval) = eval {
                assert!(
                    system.contains(dialect),
                    "missing child eval dialect for {model}"
                );
                assert!(eval
                    .description
                    .as_deref()
                    .unwrap_or_default()
                    .contains(dialect));
                assert!(system.contains("Follow the model-specific eval routing"));
            } else {
                assert!(system.contains("Eval is unavailable in this child"));
                assert!(!system.contains("# Eval routing"));
                assert!(!system.contains("<eval_routing>"));
            }
            if role == "reviewer" {
                let spawn = tools
                    .iter()
                    .find(|tool| tool.tool_id == "spawn_subagent")
                    .ok_or("reviewer cannot delegate to scout")?;
                assert_eq!(
                    spawn.parameters["properties"]["subagent_type"]["enum"],
                    json!(["scout"])
                );
            }
            if matches!(role, "scout" | "security-reviewer") {
                assert!(!tools.iter().any(|tool| matches!(
                    tool.tool_id.as_str(),
                    "write" | "edit" | "eval" | "bash" | "spawn_subagent"
                )));
            }
        }
        let returned = |needles: &[&str]| {
            requests
                .iter()
                .flat_map(|request| &request.messages)
                .filter(|message| message.role == MessageRole::Tool)
                .any(|message| {
                    needles
                        .iter()
                        .all(|needle| message.content.contains(needle))
                })
        };
        if uses_eval {
            assert!(
                returned(&["second evidence file", "batch complete"]),
                "child eval did not return the batched reads"
            );
        } else if role == "task" {
            assert!(
                returned(&["second evidence file"]),
                "direct read fallback failed"
            );
        }
        if role == "reviewer" {
            assert!(returned(&["tool eval is not enabled for this agent"]));
            assert!(requests
                .iter()
                .flat_map(|request| &request.messages)
                .any(|message| message.content.contains("agent can only spawn: scout")));
        }
        assert!(String::from_utf8(stdout)?.contains("All assignments complete."));
    }
    Ok(())
}
