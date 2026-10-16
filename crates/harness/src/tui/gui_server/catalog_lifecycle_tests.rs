use super::super::super::{runtime_ext, wire};
use super::*;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{atomic::AtomicU64, Mutex},
};

fn config(extra: bool) -> String {
    format!(
        r#"{{
        provider: {{ local: {{ type: "openai_compatible",
            options: {{ baseURL: "http://127.0.0.1:8317/v1", apiKey: "lifecycle-test-key" }},
            models: {{ example: {{ name: "Example" }} {} }}
        }}, secondary: {{ type: "openai_compatible",
            options: {{ baseURL: "http://127.0.0.1:8318/v1", apiKey: "secondary-lifecycle-test-key" }},
            models: {{ example: {{ name: "Example" }} }}
        }} }},
        model: "local/example",
        agent: {{ default: {{ model: "local/example" }}, alternate: {{ model: "local/example" }} }}
    }}"#,
        if extra {
            ", added: { name: \"Added\" }"
        } else {
            ""
        }
    )
}

fn server(root: &Path, home: &Path, path: &Path) -> Server {
    let (output, _) = tokio::sync::mpsc::unbounded_channel();
    Server {
        config_path: Some(path.to_path_buf()),
        session_dir: Some(home.join("sessions")),
        config_context: harness::CliDeps::real()
            .with_current_dir(root.to_path_buf())
            .with_env("HARNESS_HOME", home.to_string_lossy())
            .config_load_context(),
        mock: false,
        mock_script: None,
        settings: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        sessions: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        output,
        auth_attempt: Arc::new(Mutex::new(0)),
        auth_sequence: AtomicU64::new(0),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_in_refresh_makes_new_models_selectable_in_existing_session(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let path = root.path().join("harness.jsonc");
    std::fs::write(&path, config(false))?;
    let server = server(root.path(), &home, &path);
    let created = server.create(&json!({"cwd":root.path()}), false).await?;
    let id = created["sessionId"].as_str().ok_or("missing session")?;
    let selection = json!({"profile":"default","provider":"local","model":"added"});
    assert!(wire::selection(
        &selection,
        &server
            .sessions
            .lock()
            .await
            .get(id)
            .ok_or("session missing")?
            .metadata
    )
    .is_err());
    std::fs::write(&path, config(true))?;
    refresh_sessions(&server.sessions, Some(path), &home).await?;
    let selected = wire::selection(
        &selection,
        &server
            .sessions
            .lock()
            .await
            .get(id)
            .ok_or("session missing")?
            .metadata,
    )?;
    assert_eq!(selected.model(), Some("added"));
    let selected = wire::selection(
        &json!({"profile":"default","provider":"secondary","model":"example"}),
        &server
            .sessions
            .lock()
            .await
            .get(id)
            .ok_or("session missing")?
            .metadata,
    )?;
    assert_eq!(selected.provider(), "secondary");
    server.close(id).await?;
    Ok(())
}

#[tokio::test]
async fn profile_override_removes_disabled_profile_from_cached_catalog(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let path = root.path().join("harness.jsonc");
    std::fs::write(&path, config(false))?;
    let server = server(root.path(), &home, &path);
    let settings = server.settings(root.path()).await?;
    assert!(settings
        .launch_metadata
        .switchable_profiles()
        .iter()
        .any(|profile| profile == "alternate"));
    runtime_ext::dispatch(
        &server,
        "toggles.set",
        &json!({
            "cwd": root.path(), "id": "profile:alternate", "enabled": false
        }),
    )
    .await?;
    let settings = server.settings(root.path()).await?;
    assert!(!settings
        .launch_metadata
        .switchable_profiles()
        .iter()
        .any(|profile| profile == "alternate"));
    assert!(wire::selection(
        &json!({"profile":"alternate","provider":"local","model":"example"}),
        &settings.launch_metadata
    )
    .is_err());
    Ok(())
}

#[tokio::test]
async fn explicit_catalog_refresh_evicts_previously_cached_workspace_settings(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let path = root.path().join("harness.jsonc");
    std::fs::write(&path, config(false))?;
    let mut server = server(root.path(), &home, &path);
    server.mock = true;
    let _ = server.settings(root.path()).await?;
    assert!(server.settings.lock().await.contains_key(root.path()));
    super::super::super::inspection::dispatch(
        &server,
        "diagnostics.refreshModelCatalog",
        &json!({"cwd":root.path()}),
    )
    .await?;
    assert!(server.settings.lock().await.is_empty());
    let _ = server.settings(root.path()).await?;
    assert!(server.settings.lock().await.contains_key(root.path()));
    Ok(())
}
