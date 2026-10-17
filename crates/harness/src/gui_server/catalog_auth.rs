use std::sync::{atomic::Ordering, Arc};

use harness_tui::{LiveUpdate, OperatorNoticeLevel};
use serde_json::{json, Value};

use super::super::{required as string, Server, Session};
use crate::live::auth_backend::{
    refreshed_settings_after_auth, run_tui_auth_backend_streaming_with_deps,
    send_tui_auth_backend_input, spawn_tui_auth_backend_task, TuiAuthBackendContext,
};

pub(super) async fn auth_start(server: &Server, params: &Value) -> Result<Value, String> {
    let args: Vec<String> = serde_json::from_value(
        params
            .get("args")
            .cloned()
            .ok_or("invalid_params: args is required")?,
    )
    .map_err(|_| "invalid_params: args must be a string array")?;
    let stdin = match params.get("stdin") {
        None => None,
        Some(Value::String(value)) => Some(value.clone()),
        _ => return Err("invalid_params: stdin must be a string".into()),
    };
    let settings = server.settings(&super::cwd(server, params)?).await?;
    let attempt = server.auth_sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let mut active = server
        .auth_attempt
        .lock()
        .map_err(|error| error.to_string())?;
    send_tui_auth_backend_input(None);
    *active = attempt;
    let (tx, rx) = harness_tui::live_update_channel();
    spawn_tui_auth_backend_task(
        Some(attempt),
        None,
        args,
        stdin,
        TuiAuthBackendContext::from_settings(&settings),
        tx,
    );
    drop(active);
    let output = server.output.clone();
    let cache = Arc::clone(&server.settings);
    let sessions = Arc::clone(&server.sessions);
    let active = Arc::clone(&server.auth_attempt);
    let config_path = server.config_path.clone();
    let home = settings.data_dir;
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut refresh_error = None;
        while let Ok(update) = rx.recv() {
            let params = match update {
                LiveUpdate::AuthProviderCatalogRefreshed { .. } => {
                    cache.blocking_lock().clear();
                    refresh_error = runtime
                        .block_on(refresh_sessions(&sessions, config_path.clone(), &home))
                        .err();
                    continue;
                }
                LiveUpdate::AuthBackendNotice { message, level, .. } => {
                    json!({"attempt":attempt,"kind":"notice","level":if matches!(level, OperatorNoticeLevel::Error) {"error"} else {"info"},"message":message})
                }
                LiveUpdate::AuthBackendResult {
                    success, message, ..
                } => {
                    cache.blocking_lock().clear();
                    if let Ok(mut current) = active.lock()
                        && *current == attempt
                    {
                        *current = 0;
                    }
                    let success = success && refresh_error.is_none();
                    let message = refresh_error.take().map_or(message, |error| {
                        format!("Signed in, but live provider refresh failed: {error}")
                    });
                    json!({"attempt":attempt,"kind":"result","success":success,"message":message})
                }
                _ => continue,
            };
            if output
                .send(json!({"method":"auth.update","params":params}))
                .is_err()
            {
                break;
            }
        }
    });
    Ok(json!({"attempt":attempt}))
}

async fn refresh_sessions(
    sessions: &tokio::sync::Mutex<std::collections::BTreeMap<String, Session>>,
    config_path: Option<std::path::PathBuf>,
    home: &std::path::Path,
) -> Result<(), String> {
    let mut sessions = sessions.lock().await;
    for session in sessions.values_mut() {
        let deps = harness::CliDeps::real()
            .with_current_dir(session.cwd.clone())
            .with_env("HARNESS_HOME", home.to_string_lossy());
        let mut settings = refreshed_settings_after_auth(
            Some("login"),
            config_path.clone(),
            session.run_dir.parent().map(std::path::Path::to_path_buf),
            session.cwd.clone(),
            &deps,
            None,
        )?
        .ok_or("provider catalog refresh unavailable")?;
        super::super::runtime_ext::apply_overrides(&mut settings)?;
        session
            .warmup
            .add_signed_in_providers(&settings, Some(&session.coordinator))
            .await?;
        session.metadata = session
            .metadata
            .clone()
            .with_available_models(settings.launch_metadata.available_models().to_vec())
            .with_resolved_models(settings.launch_metadata.resolved_models().to_vec())
            .with_switchable_profiles(settings.launch_metadata.switchable_profiles().to_vec());
        let mut selection = crate::live::recover_mutex_lock(&session.launch_selection);
        selection.metadata = session.metadata.clone();
        selection.config_digest = settings.config_digest;
    }
    Ok(())
}

pub(super) fn auth_input(server: &Server, params: &Value) -> Result<Value, String> {
    let attempt = params
        .get("attempt")
        .and_then(Value::as_u64)
        .ok_or("invalid_params: attempt must be an integer")?;
    let line = match params.get("line") {
        Some(Value::Null) => None,
        Some(Value::String(line)) => Some(line.clone()),
        _ => return Err("invalid_params: line must be a string or null".into()),
    };
    let mut active = server
        .auth_attempt
        .lock()
        .map_err(|error| error.to_string())?;
    if attempt == 0 || *active != attempt {
        return Err("not_found: authentication attempt is no longer active".into());
    }
    let cancel = line.is_none();
    send_tui_auth_backend_input(line);
    if cancel {
        *active = 0;
        let _ = server.output.send(json!({"method":"auth.update","params":{"attempt":attempt,"kind":"result","success":false,"message":"Authentication cancelled"}}));
    }
    Ok(Value::Null)
}

pub(super) async fn auth_logout(server: &Server, params: &Value) -> Result<Value, String> {
    let provider = string(params, "provider")?.to_owned();
    if harness_core::auth::ProviderId::parse(&provider).is_none() {
        return Err("invalid_params: invalid provider".into());
    }
    let config_path = server.config_path.clone();
    let session_dir = server.session_dir.clone();
    let root = server.config_context.discovery.current_dir.clone();
    tokio::task::spawn_blocking(move || {
        let deps = harness::CliDeps::real().with_current_dir(root);
        let (message, _, success) = run_tui_auth_backend_streaming_with_deps(
            vec!["logout".into(), provider],
            config_path,
            session_dir,
            "",
            &deps,
            None,
            None,
        );
        if success {
            Ok(())
        } else {
            Err(message)
        }
    })
    .await
    .map_err(|error| error.to_string())??;
    server.settings.lock().await.clear();
    Ok(Value::Null)
}

#[cfg(test)]
#[path = "catalog_lifecycle_tests.rs"]
mod lifecycle_tests;
