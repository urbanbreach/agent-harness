use harness::{run, CliDeps, CliIo};
use harness_core::{event::EventV1, store::read_events};
use serde_json::json;
use std::{
    fs,
    io::{Cursor, Write},
    sync::Arc,
};

#[path = "cli/http_prompt.rs"]
mod http_prompt;

#[test]
fn openai_login_routes_oauth_to_codex_and_keeps_api_keys_separate(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::auth::{CredentialStore, ProviderId, StoredCredential};

    for method in ["browser", "device", "api-key"] {
        let root = tempfile::tempdir()?;
        let data = root.path().join("data");
        let config = root.path().join("fixture.json");
        fs::write(
            &config,
            r#"{"provider":{"openai":{"type":"openai_compatible"}}}"#,
        )?;
        let store = CredentialStore::new(data.clone());
        store.save(&StoredCredential::oauth(
            ProviderId::codex(),
            "old-access-token",
            "old-refresh-token",
            None,
            "2026-09-28T00:00:00Z",
        ))?;
        let deps = CliDeps::real()
            .with_current_dir(root.path().into())
            .with_env("HARNESS_HOME", data.to_str().ok_or("data path")?)
            .without_env("HARNESS_CONFIG")
            .without_env("HARNESS_CONFIG_CONTENT");
        let mut args = vec![
            "login".into(),
            "openai".into(),
            "--method".into(),
            method.into(),
        ];
        if method == "api-key" {
            args.push("--api-key-stdin".into());
        } else {
            args.extend(["--mock-token".into(), "new-access-token".into()]);
        }
        let output =
            harness::execute_auth_backend_args(&args, Some(config), None, "new-api-key", &deps);
        assert_eq!(output.code, 0, "{}", output.stderr);
        let codex = store
            .load(&ProviderId::codex())?
            .ok_or("Codex credential missing")?;
        let openai = store.load(&ProviderId::parse("openai").ok_or("OpenAI provider")?)?;
        if method == "api-key" {
            assert_eq!(codex.access_token.as_deref(), Some("old-access-token"));
            assert_eq!(
                openai.ok_or("API key missing")?.api_key.as_deref(),
                Some("new-api-key")
            );
        } else {
            assert_eq!(codex.access_token.as_deref(), Some("new-access-token"));
            assert!(
                openai.is_none(),
                "ChatGPT login must replace the Codex credential"
            );
        }
    }
    Ok(())
}

#[test]
fn invalid_prompt_setup_fails_before_creating_a_session() -> Result<(), Box<dyn std::error::Error>>
{
    for kind in ["environment", "input", "run_input"] {
        let root = tempfile::tempdir()?;
        let text = if kind != "environment" {
            let mut text = vec![b'\n'; 1024 * 1024 + 1];
            text[0] = b'x';
            text
        } else {
            b"hello".to_vec()
        };
        let (mut input, mut stdout, mut stderr) = (Cursor::new(text), Vec::new(), Vec::new());
        let mut deps = CliDeps::real()
            .with_current_dir(root.path().into())
            .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy());
        if kind == "environment" {
            deps = deps.with_env("HARNESS_REMOTE_SEARCH_TIMEOUT_SECS", "invalid");
        }
        let result = run(
            [
                "harness",
                if kind == "run_input" { "run" } else { "prompt" },
                "--mock",
                "--stdin",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps,
        );
        assert_eq!(result.code, 1);
        assert!(
            String::from_utf8(stderr)?.contains(if kind == "environment" {
                "HARNESS_REMOTE_SEARCH_TIMEOUT_SECS"
            } else {
                "1 MiB"
            })
        );
        assert_eq!(fs::read_dir(root.path())?.count(), 0);
    }
    Ok(())
}

#[tokio::test]
async fn failure_to_print_the_run_directory_finishes_the_started_session(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let root = tempfile::tempdir()?;
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Broken);
    let result = run(
        [
            "harness",
            "prompt",
            "--mock",
            "--text",
            "hello",
            "--print-run-dir",
        ],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real()
            .with_current_dir(root.path().into())
            .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy()),
    );
    assert_eq!(result.code, 1);
    let run_dir = fs::read_dir(
        harness_core::storage_paths::ProjectPaths::new(&root.path().join("data"), root.path())?
            .sessions_dir(),
    )?
    .next()
    .ok_or("session missing")??
    .path();
    let events = read_events(&run_dir.join("events.jsonl"))?;
    assert!(
        matches!(
            events.last().map(|e| &e.payload),
            Some(EventV1::RunFailed(_))
        ),
        "setup failure must not leave an active session"
    );
    Ok(())
}

#[test]
fn interrupted_prompt_cancels_active_work_and_closes_the_session(
) -> Result<(), Box<dyn std::error::Error>> {
    struct Interrupt(tokio_util::sync::CancellationToken);
    #[async_trait::async_trait]
    impl harness_providers::Provider for Interrupt {
        async fn stream_completion(
            &self,
            _: harness_providers::CompletionRequest,
        ) -> harness_providers::ProviderEventStream {
            self.0.cancel();
            Box::pin(tokio_stream::pending())
        }
    }
    let root = tempfile::tempdir()?;
    let config = root.path().join("runtime.json");
    fs::write(
        &config,
        json!({"runtime":{"prompt":{"wait_timeout_ms":100}}}).to_string(),
    )?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let (mut input, mut output, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        [
            "harness",
            "--config",
            config.to_str().ok_or("config path")?,
            "prompt",
            "--mock",
            "--text",
            "hello",
        ],
        &mut CliIo::new(&mut input, &mut output, &mut errors),
        CliDeps::real()
            .with_current_dir(root.path().into())
            .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy())
            .with_provider_override(Arc::new(Interrupt(cancel.clone())))
            .with_cancellation(cancel),
    );
    assert_eq!(result.code, 1);
    assert!(String::from_utf8(errors)?.contains("prompt interrupted"));
    let run_dir = fs::read_dir(
        harness_core::storage_paths::ProjectPaths::new(&root.path().join("data"), root.path())?
            .sessions_dir(),
    )?
    .next()
    .ok_or("session missing")??
    .path();
    let events = read_events(&run_dir.join("events.jsonl"))?;
    assert!(events
        .iter()
        .any(|e| matches!(e.payload, EventV1::TaskCancelled(_))));
    assert!(matches!(
        events.last().map(|e| &e.payload),
        Some(EventV1::RunFailed(_))
    ));
    Ok(())
}
