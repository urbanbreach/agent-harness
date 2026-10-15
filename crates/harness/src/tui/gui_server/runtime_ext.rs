use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use harness_core::config::{HarnessConfig, McpServerConfig};
use harness_core::store::{create_private_dir, open_private_file, write_private_atomic};
use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent, ToolChoice,
};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use super::super::live_settings::LiveSettings;
use super::Server;

static TOGGLES_LOCK: Mutex<()> = Mutex::new(());

pub(super) async fn dispatch(
    server: &Server,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    match method {
        "llm.complete" => match settings(server, params).await {
            Ok(settings) => tokio::time::timeout(
                std::time::Duration::from_secs(180),
                complete(&settings, params),
            )
            .await
            .map_err(|_| "timeout: completion exceeded 180 seconds".to_string())?,
            Err(error) => Err(error),
        },
        "toggles.get" | "toggles.set" => toggles(server, method, params).await,
        "trust.check" => match settings(server, params).await {
            Ok(_) => Ok(
                json!({"supported":false,"trusted":null,"status":"unknown","reason":"The runtime has no authoritative folder-trust service; subagent startup currently assumes project trust."}),
            ),
            Err(error) => Err(error),
        },
        "trust.set" => Err(
            "unavailable: folder trust cannot be enforced consistently by the current runtime"
                .into(),
        ),
        "app.doctor" | "app.configSources" => inspect(server, method, params).await,
        "app.releaseNotes" => Ok(
            json!({"version":env!("CARGO_PKG_VERSION"),"markdown":include_str!("../../../../../CHANGELOG.md")}),
        ),
        "session.info" => session_info(server, params).await,
        _ => Err("method_not_found: unknown runtime method".into()),
    }
}

async fn settings(server: &Server, params: &Value) -> Result<LiveSettings, String> {
    server
        .settings(Path::new(super::required(params, "cwd")?))
        .await
}

/// A single configured-provider request, with neither tool definitions nor execution.
pub(super) async fn complete(settings: &LiveSettings, params: &Value) -> Result<Value, String> {
    let config = settings
        .config
        .as_ref()
        .ok_or("unavailable: no configured provider")?;
    let request = completion_request(settings, params)?;
    let provider_id = request
        .provider_id
        .clone()
        .ok_or("unavailable: no provider selected")?;
    let model = request.model_id.clone();
    let deps = harness::CliDeps::real()
        .with_current_dir(settings.workspace_root.clone())
        .with_env("HARNESS_HOME", settings.data_dir.to_string_lossy());
    let registry = Arc::new(harness_core::redact::SecretRegistry::default());
    let mut providers = crate::bootstrap::provider_map(config, &deps, &registry)?;
    let provider = providers
        .remove(&provider_id)
        .ok_or("unavailable: selected provider is disabled or unavailable")?;
    let text = collect_completion(provider.as_ref(), request).await?;
    Ok(json!({"text":text,"model":model,"provider":provider_id}))
}

fn completion_request(
    settings: &LiveSettings,
    params: &Value,
) -> Result<CompletionRequest, String> {
    let prompt = super::required(params, "prompt")?;
    if prompt.trim().is_empty() {
        return Err("invalid_params: prompt must not be empty".into());
    }
    let config = settings
        .config
        .as_ref()
        .ok_or("unavailable: no configured provider")?;
    let profile = settings.launch_metadata.profile();
    let selector = match params.get("model") {
        Some(value) => value
            .as_str()
            .ok_or("invalid_params: model must be a string")?,
        None => config
            .agents
            .get(profile)
            .map(|agent| agent.model_ref.as_str())
            .ok_or("unavailable: selected profile has no configured model")?,
    };
    let target = harness_core::config::resolve_model_selection(config, selector, None)
        .map_err(|error| error.to_string())?
        .primary;
    let mut messages = Vec::with_capacity(2);
    if let Some(system) = params.get("system") {
        messages.push(CompletionMessage::text(
            MessageRole::System,
            system
                .as_str()
                .ok_or("invalid_params: system must be a string")?,
        ));
    }
    messages.push(CompletionMessage::text(MessageRole::User, prompt));
    Ok(CompletionRequest {
        provider_id: Some(target.provider),
        model_id: target.model,
        messages,
        variant: target.variant,
        reasoning_effort: target.reasoning_effort,
        text_verbosity: target.text_verbosity,
        reasoning_summary: target.reasoning_summary,
        thinking: target.thinking,
        max_tokens: target.limits.max_output.tokens,
        tools: None,
        tool_choice: Some(ToolChoice::None),
        stream: true,
        context: harness_providers::ProviderRequestContext {
            working_directory: Some(settings.workspace_root.clone()),
            initiator: harness_providers::ProviderRequestInitiator::User,
            ..Default::default()
        },
        ..Default::default()
    })
}

async fn collect_completion(
    provider: &dyn Provider,
    request: CompletionRequest,
) -> Result<String, String> {
    let mut stream = provider.stream_completion(request).await;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                return Ok(text)
            }
            ProviderStreamEvent::Error { message, .. } => return Err(message),
            ProviderStreamEvent::Aborted { .. } => {
                return Err("cancelled: completion was aborted".into())
            }
            ProviderStreamEvent::ToolCallDelta { .. }
            | ProviderStreamEvent::ToolCallComplete { .. } => {
                return Err(
                    "provider_error: provider attempted a tool call in a tool-free completion"
                        .into(),
                );
            }
            _ => {}
        }
    }
    Err("provider_error: completion stream ended without a terminal result".into())
}

fn overrides_path(settings: &LiveSettings) -> Result<PathBuf, String> {
    let canonical = settings
        .workspace_root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let key = blake3::hash(canonical.as_os_str().as_encoded_bytes()).to_hex();
    Ok(settings
        .data_dir
        .join("gui/runtime")
        .join(format!("{key}.json")))
}

fn read_overrides(path: &Path) -> Result<BTreeMap<String, bool>, String> {
    let file = match open_private_file(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("invalid_state: runtime overrides exceed 1 MiB".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid_state: runtime overrides: {error}"))
}

fn entries(config: &HarnessConfig) -> Vec<Value> {
    let mut entries = Vec::new();
    for (name, profile) in &config.agents {
        entries.push(
            json!({"id":format!("profile:{name}"),"kind":"profile","name":name,"enabled":true}),
        );
        for tool in &profile.tools {
            entries.push(json!({"id":format!("tool:{name}:{tool}"),"kind":"tool","name":tool,"agent":name,"enabled":true}));
        }
    }
    for (index, hook) in config.hooks.lifecycle.iter().enumerate() {
        let id = hook
            .id
            .clone()
            .unwrap_or_else(|| format!("{} #{index}", hook.event.as_str()));
        entries.push(json!({"id":format!("hook:{id}"),"kind":"hook","name":id,"enabled":true}));
    }
    for (name, server) in &config.integrations.mcp.servers {
        entries.push(
            json!({"id":format!("mcp:{name}"),"kind":"mcp","name":name,"enabled":server.enabled()}),
        );
    }
    entries
}

fn apply_config(
    config: &mut HarnessConfig,
    overrides: &BTreeMap<String, bool>,
) -> Result<(), String> {
    let default = crate::bootstrap::interactive_profile_name(config);
    if overrides.get(&format!("profile:{default}")) == Some(&false) {
        return Err("invalid_params: cannot disable the default interactive profile".into());
    }
    for (name, profile) in &mut config.agents {
        profile
            .tools
            .retain(|tool| overrides.get(&format!("tool:{name}:{tool}")) != Some(&false));
    }
    config
        .agents
        .retain(|name, _| overrides.get(&format!("profile:{name}")) != Some(&false));
    let mut index = 0;
    config.hooks.lifecycle.retain(|hook| {
        let id = hook
            .id
            .clone()
            .unwrap_or_else(|| format!("{} #{index}", hook.event.as_str()));
        index += 1;
        overrides.get(&format!("hook:{id}")) != Some(&false)
    });
    for (name, server) in &mut config.integrations.mcp.servers {
        if let Some(value) = overrides.get(&format!("mcp:{name}")) {
            match server {
                McpServerConfig::Stdio { enabled, .. } | McpServerConfig::Http { enabled, .. } => {
                    *enabled = *value
                }
            }
        }
    }
    config.validate().map_err(|error| error.to_string())
}

fn save_overrides(
    path: &Path,
    config: &HarnessConfig,
    overrides: &BTreeMap<String, bool>,
) -> Result<(), String> {
    apply_config(&mut config.clone(), overrides)?;
    let parent = path.parent().ok_or("invalid runtime override path")?;
    create_private_dir(parent).map_err(|error| error.to_string())?;
    write_private_atomic(
        path,
        &serde_json::to_vec(overrides).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

/// Apply once to fresh settings before they are cached or used to start a session.
pub(super) fn apply_overrides(settings: &mut LiveSettings) -> Result<(), String> {
    let overrides = read_overrides(&overrides_path(settings)?)?;
    if overrides.is_empty() {
        return Ok(());
    }
    if let Some(config) = settings.config.as_mut() {
        apply_config(config, &overrides)?;
        settings.config_digest =
            blake3::hash(&serde_json::to_vec(config).map_err(|error| error.to_string())?)
                .to_hex()
                .to_string();
    }
    Ok(())
}

async fn toggles(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let cwd = PathBuf::from(super::required(params, "cwd")?);
    // Resolve authored config afresh: disabled entries must remain available to re-enable.
    let mut settings = server.settings(&cwd).await?;
    let context = server.config_context.clone().with_current_dir(cwd.clone());
    let loaded = harness_core::config::load_config_or_defaults_with_lookup(
        server.config_path.as_deref(),
        &context,
        &|name| std::env::var(name).ok(),
    )
    .map_err(|error| error.to_string())?;
    settings.config = Some(loaded.config);
    let path = overrides_path(&settings)?;
    let result = {
        let _guard = TOGGLES_LOCK.lock().map_err(|error| error.to_string())?;
        let mut overrides = read_overrides(&path)?;
        let config = settings
            .config
            .as_ref()
            .ok_or("unavailable: no configuration")?;
        let mut rows = entries(config);
        if method == "toggles.set" {
            let id = super::required(params, "id")?;
            let enabled = params["enabled"]
                .as_bool()
                .ok_or("invalid_params: enabled must be a boolean")?;
            if !rows.iter().any(|entry| entry["id"].as_str() == Some(id)) {
                return Err("not_found: toggle not found".into());
            }
            overrides.insert(id.to_owned(), enabled);
            save_overrides(&path, config, &overrides)?;
        }
        for entry in &mut rows {
            if let Some(enabled) = entry["id"].as_str().and_then(|id| overrides.get(id)) {
                entry["enabled"] = json!(enabled);
            }
        }
        json!({"entries":rows,"appliesTo":"newSessions","persisted":true})
    };
    if method == "toggles.set" {
        server.settings.lock().await.clear();
    }
    Ok(result)
}

async fn inspect(server: &Server, method: &str, params: &Value) -> Result<Value, String> {
    let settings = settings(server, params).await?;
    let context = server
        .config_context
        .clone()
        .with_current_dir(settings.workspace_root.clone());
    let loaded = harness_core::config::load_config_or_defaults_with_lookup(
        server.config_path.as_deref(),
        &context,
        &|name| std::env::var(name).ok(),
    )
    .map_err(|error| error.to_string())?;
    if method == "app.configSources" {
        let mut layers = loaded
            .paths
            .iter()
            .map(|path| json!({"path":path,"kind":"runtime"}))
            .collect::<Vec<_>>();
        if context.runtime_content.is_some() {
            layers.push(json!({"path":"HARNESS_CONFIG_CONTENT","kind":"environment"}));
        }
        layers.extend(
            harness_core::config::resolve_tui_config_layer_paths_with_context(&context.discovery)
                .iter()
                .map(|path| json!({"path":path,"kind":"tui"})),
        );
        return Ok(json!({"primaryPath":loaded.primary_path(),"layers":layers,
            "searched":harness_core::config::config_search_paths(&context.discovery, false),
            "runtimeOverrides":overrides_path(&settings)?,"scope":"local_configuration"}));
    }
    let config = settings.config.as_ref().unwrap_or(&loaded.config);
    let catalog = harness_core::config::configured_model_catalog(config);
    let unknown = catalog
        .iter()
        .filter(|entry| !entry.limits.is_selectable_known())
        .count();
    Ok(
        json!({"noNetworkProbes":true,"providerExecutionProof":false,"scope":"local_configuration",
        "checks":[{"name":"configuration","status":"pass"},
        {"name":"providers","status":if config.providers.is_empty() {"unavailable"} else {"unknown"},"message":"Provider connectivity has not been tested."},
        {"name":"modelLimits","status":if catalog.is_empty() || unknown > 0 {"unknown"} else {"pass"},"unknown":unknown}],
        "trust":{"supported":false,"status":"unknown"}}),
    )
}

async fn session_info(server: &Server, params: &Value) -> Result<Value, String> {
    let id = super::required(params, "sessionId")?;
    let (cwd, metadata, coordinator, access_mode) = {
        let sessions = server.sessions.lock().await;
        let session = sessions
            .get(id)
            .ok_or("not_found: live session not found")?;
        (
            session.cwd.clone(),
            session.metadata.clone(),
            session.coordinator.clone(),
            match session.access.load(std::sync::atomic::Ordering::Relaxed) {
                1 => "autoEdits",
                2 => "full",
                3 => "auto",
                _ => "supervised",
            },
        )
    };
    let settings = server.settings(&cwd).await?;
    let info = coordinator
        .run_info()
        .await
        .map_err(|error| error.to_string())?;
    let config = settings.config.as_ref();
    let mcp = config.map(|config| config.integrations.mcp.servers.iter().map(|(name, server)|
        json!({"name":name,"enabled":server.enabled(),"status":if server.enabled() {"unknown"} else {"disabled"}})).collect::<Vec<_>>()).unwrap_or_default();
    let lsp = config.map(|config| config.lsp.servers.keys().map(|name|
        json!({"name":name,"status":if config.lsp.disabled {"disabled"} else {"unknown"}})).collect::<Vec<_>>()).unwrap_or_default();
    Ok(
        json!({"sessionId":id,"runDir":info.run_dir,"cwd":cwd,"accessMode":access_mode,
        "profile":metadata.profile(),"model":metadata.model(),"provider":metadata.provider(),
        "selectionStatus":"launch_snapshot","mcp":mcp,"lsp":lsp,
        "integrationStatusReason":"Current configuration only, not the session's launch configuration or observed health; no live MCP/LSP health snapshot is exposed by the coordinator.",
        "sandbox":{"status":"unknown","enforced":null,"reason":"Permissions are policy checks, not an OS sandbox; no platform enforcement probe is available."},
        "goals":{"supported":false,"reason":"The coordinator exposes no durable thread-objective/status/token-budget state or continuation intent. A generic prompt or compaction resume is not a goal scheduler.","guidance":"Disable goal scheduling controls; use an ordinary prompt to describe the objective and show usage separately."}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_shot_sends_only_explicit_messages_and_disables_tools() -> Result<(), String> {
        let config = harness_core::config::load_config_from_str(r#"{
            provider: { local: {
                type: "openai_compatible",
                options: { baseURL: "http://127.0.0.1:8317/v1", apiKey: "test-key" },
                models: { "example": { name: "Example" } }
            }},
            model: "local/example",
            agent: { default: { model: "local/example", tools: ["read"], system_prompt: "Do not inherit this." } }
        }"#).map_err(|error| error.to_string())?;
        let settings = LiveSettings {
            config: Some(config),
            config_path: None,
            session_dir: PathBuf::new(),
            data_dir: PathBuf::new(),
            workspace_root: PathBuf::from("/"),
            shell_allowlist: Default::default(),
            deterministic: false,
            yolo: false,
            seed: 0,
            config_digest: String::new(),
            launch_mode_label: None,
            launch_metadata: harness_tui::app::LaunchMetadata::new(
                "default",
                "local",
                Some("example".into()),
            ),
            toggles: Default::default(),
        };
        let request = completion_request(
            &settings,
            &json!({"prompt":"Hello","system":"Only answer."}),
        )?;
        let provider = harness_providers::mock::MockProvider::script([vec![
            ProviderStreamEvent::TextDelta("answer".into()),
            ProviderStreamEvent::Done { usage: None },
        ]]);
        assert_eq!(collect_completion(&provider, request).await?, "answer");
        let requests = provider.captured_requests().await;
        let request = requests.first().ok_or("missing provider request")?;
        assert!(request.tools.is_none());
        assert_eq!(request.tool_choice, Some(ToolChoice::None));
        assert_eq!(
            request.messages,
            vec![
                CompletionMessage::text(MessageRole::System, "Only answer."),
                CompletionMessage::text(MessageRole::User, "Hello"),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn tool_free_completion_rejects_tools_and_incomplete_streams() {
        use harness_providers::mock::MockProvider;
        for events in [
            vec![ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call".into(),
                function_name: "shell".into(),
                arguments_json: "{}".into(),
            }],
            vec![ProviderStreamEvent::TextDelta("partial".into())],
            vec![ProviderStreamEvent::Aborted { usage: None }],
        ] {
            let provider = MockProvider::script([events]);
            assert!(collect_completion(&provider, CompletionRequest::default())
                .await
                .is_err());
        }
    }

    #[test]
    fn invalid_override_never_replaces_persisted_decisions() -> Result<(), String> {
        let root = tempfile::tempdir().map_err(|error| error.to_string())?;
        let path = root.path().join("overrides.json");
        write_private_atomic(&path, b"{\"hook:existing\":false}")
            .map_err(|error| error.to_string())?;
        let mut overrides = read_overrides(&path)?;
        let config = HarnessConfig::default();
        let default = crate::bootstrap::interactive_profile_name(&config);
        overrides.insert(format!("profile:{default}"), false);
        assert!(save_overrides(&path, &config, &overrides).is_err());
        assert_eq!(read_overrides(&path)?.get("hook:existing"), Some(&false));
        assert_eq!(read_overrides(&path)?.len(), 1);
        Ok(())
    }
}
