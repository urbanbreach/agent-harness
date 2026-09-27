use harness::{run, CliDeps, CliIo};
use harness_providers::{mock::MockProvider, Provider, ProviderErrorCategory, ProviderStreamEvent};
use std::{fs, io::Cursor, sync::Arc};

#[tokio::test]
async fn model_fallback_changes_prompt_family_and_keeps_explicit_instructions(
) -> Result<(), Box<dyn std::error::Error>> {
    for explicit in [None, Some("Keep this configured prompt.")] {
        let root = tempfile::tempdir()?;
        let prompts = root.path().join(".agent-harness/prompt-families");
        fs::create_dir_all(&prompts)?;
        fs::write(prompts.join("default.md"), "Primary family.")?;
        fs::write(prompts.join("anthropic.md"), "Fallback family.")?;
        fs::write(root.path().join("fixture.json"), serde_json::json!({
            "provider":{"local":{"type":"openai_compatible","models":{
                "base":{}, "backup":{"metadata":{"family":"claude"}}
            }}},
            "model":"quality", "model_profile":{"quality":{"model":"local/base","fallback":[{"model":"local/backup"}]}},
            "agent":{"default":{"tools":[],"system_prompt":explicit}},
            "instructions":"Keep project instructions.","runtime":{"provider_retry":{"max_retries":0}}
        }).to_string())?;
        let provider = Arc::new(MockProvider::script([
            vec![ProviderStreamEvent::categorized_error(
                "try the fallback",
                ProviderErrorCategory::TransportFailure,
            )],
            vec![
                ProviderStreamEvent::TextDelta("Finished.".into()),
                ProviderStreamEvent::Done { usage: None },
            ],
            vec![ProviderStreamEvent::Done { usage: None }],
        ]));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "fixture.json",
                "prompt",
                "--text",
                "Continue.",
                "--session-id",
                "family-test",
                "--rules",
                "Keep the command rule.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let result = run(
            [
                "harness",
                "--config",
                "fixture.json",
                "prompt",
                "--text",
                "Continue again.",
                "--resume",
                "family-test",
                "--rules",
                "Keep the command rule.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 3);
        for (request, model, family) in [
            (&requests[0], "base", "Primary family."),
            (&requests[1], "backup", "Fallback family."),
            (&requests[2], "backup", "Fallback family."),
        ] {
            assert_eq!(request.model_id, model);
            assert!(
                request.messages[0]
                    .content
                    .starts_with(explicit.unwrap_or(family)),
                "{}",
                request.messages[0].content
            );
            assert!(request.messages[0]
                .content
                .contains("Keep project instructions."));
            assert_eq!(
                request.messages[0]
                    .content
                    .matches("Keep the command rule.")
                    .count(),
                1
            );
        }
        assert_eq!(requests[0].messages[1..], requests[1].messages[1..]);
    }
    Ok(())
}
