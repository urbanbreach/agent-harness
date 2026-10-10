use super::HarnessConfig;
use serde_json::{json, Value};

/// The file format, including shorthand expanded before deserialization.
pub fn config_json_schema() -> Value {
    let mut schema = schemars::schema_for!(HarnessConfig).to_value();
    let defs = &mut schema["$defs"];
    for (name, fields) in [
        ("ProviderConfig", "baseURL base_url baseUrl;apiKey api_key;apiKeyEnv api_key_env apiKeyEnvironment;timeoutMs timeout_ms;authProvider auth_provider;apiMode api_mode;cacheRetention cache_retention"),
        ("OpenAiCompatibleProviderOptions", "baseURL base_url baseUrl;apiKey api_key;apiKeyEnv api_key_env apiKeyEnvironment;timeoutMs timeout_ms;authProvider auth_provider;apiMode api_mode;cacheRetention cache_retention"),
        ("AnthropicProviderOptions", "baseURL base_url baseUrl;apiKey api_key;apiKeyEnv api_key_env apiKeyEnvironment;timeoutMs timeout_ms"),
        ("ModelConfig", "name display_name displayName;max_input_tokens maxInputTokens;max_output_tokens maxOutputTokens"),
        ("ModelCost", "cache_read cacheRead;cache_write cacheWrite"),
        ("ModelVariantConfig", "name display_name displayName;context_window_tokens contextWindowTokens;max_input_tokens maxInputTokens;max_output_tokens maxOutputTokens"),
        ("ModelMetadataConfig", "release_stage releaseStage;context_window_tokens contextWindowTokens;supports_tool_calls supportsToolCalls;supports_reasoning_summaries supportsReasoningSummaries"),
        ("ModelVariantMetadataConfig", "reasoning_effort reasoningEffort;text_verbosity textVerbosity;recommended_for recommendedFor"),
        ("ProfileConfig", "system_prompt systemPrompt prompt;model_ref modelRef model;top_p topP;max_iters maxIters maxSteps steps;tool_failure_mode toolFailureMode"),
        ("PermissionsConfig", "shell_allowlist shellAllowlist"),
        ("PermissionDefaultsConfig", "webfetch webFetch;websearch webSearch;codesearch codeSearch;lsp codeLsp"),
        ("ProfilePermissions", "shell bash;webfetch webFetch;websearch webSearch;codesearch codeSearch;lsp codeLsp"),
        ("ShellAllowlist", "mode policy_mode policyMode;cwd_roots cwdRoots"),
        ("RuntimeConfig", "background_tasks backgroundTasks;session_dir sessionDir;provider_retry providerRetry"),
        ("BackgroundTaskSettings", "defaultConcurrency default_concurrency;providerConcurrency provider_concurrency;modelConcurrency model_concurrency;staleTimeoutMs stale_timeout_ms;messageStalenessTimeoutMs message_staleness_timeout_ms"),
        ("ProviderRetryRuntimeConfig", "max_retries maxRetries;base_delay_ms baseDelayMs;max_delay_ms maxDelayMs"),
        ("CompactionSettings", "threshold_percent thresholdPercent;threshold_tokens thresholdTokens;model_thresholds modelThresholds;agent_thresholds agentThresholds;reserve_tokens reserveTokens;keep_recent_tokens keepRecentTokens;auto_retry_overflow autoRetryOverflow;structured_summary_contract structuredSummaryContract;estimated_token_triggers estimatedTokenTriggers;fallback_input_tokens fallbackInputTokens;split_oversized_turns splitOversizedTurns;suppress_auto_compaction suppressAutoCompaction"),
        ("RuntimePermissionsConfig", "ask_timeout_ms askTimeoutMs"),
        ("PromptRuntimeConfig", "wait_timeout_ms waitTimeoutMs"),
        ("UiConfig", "default_profile defaultProfile;maxEventsInMemory max_events_in_memory;maxTranscriptCharsInMemory max_transcript_chars_in_memory"),
        ("FormatterConfig", "experimental_oxfmt experimentalOxfmt"),
        ("LifecycleHookConfig", "id name;timeout_ms timeoutMs"),
        ("SkillsConfig", "project_roots projectRoots paths;global_roots globalRoots;disabled disabledIds;walk_to_git_root walkToGitRoot"),
        ("IntegrationsConfig", "remote_search remoteSearch"),
        ("RemoteSearchConfig", "auth_token authToken;require_auth requireAuth;timeout_secs timeoutSecs;max_retries maxRetries;retry_backoff_ms retryBackoffMs"),
        ("McpServerConfig", "env environment;endpoint url;timeout_secs timeoutSecs timeout"),
    ] {
        if let Some(def) = defs.get_mut(name) {
            aliases(def, fields);
        }
    }
    if let Some(variants) = defs["McpServerConfig"]["oneOf"].as_array_mut() {
        for variant in variants {
            if variant["properties"]["transport"]["const"] == "http" {
                variant["properties"]["transport"] = json!({"enum":["http","streamable_http"]});
            }
        }
    }
    defs["ProfileConfig"]["properties"]["permissions"] = permission_schema(true);
    let eval = &mut defs["EvalConfig"]["properties"];
    eval["languages"]["items"] = json!({"type":"string","enum":["js","py","rb","jl"]});
    eval["languages"]["minItems"] = json!(1);
    eval["languages"]["uniqueItems"] = json!(true);
    eval["route_tools"]["maxItems"] = json!(256);
    eval["route_tools"]["uniqueItems"] = json!(true);
    eval["route_tools"]["items"] = json!({"type":"string","minLength":1,"maxLength":256,"pattern":"^\\S+$","not":{"enum":["eval","question"]}});
    for field in [
        "cell_timeout_seconds",
        "foreground_window_seconds",
        "run_budget_seconds",
        "hard_limit_seconds",
    ] {
        eval[field]["minimum"] = json!(1);
        eval[field]["maximum"] = json!(86400);
    }
    for field in ["max_detached_cells", "parallel_pool_width"] {
        eval[field]["minimum"] = json!(1);
        eval[field]["maximum"] = json!(256);
    }
    eval["output_head_bytes"]["maximum"] = json!(51200);
    eval["output_max_columns"]["maximum"] = json!(16384);
    for (field, maximum) in [("memory_limit_mb", 4096), ("timeout_seconds", 86400)] {
        defs["EvalSandboxConfig"]["properties"][field]["minimum"] = json!(1);
        defs["EvalSandboxConfig"]["properties"][field]["maximum"] = json!(maximum);
    }
    if let Some(properties) = defs["EvalMemoryConfig"]["properties"].as_object_mut() {
        for value in properties.values_mut() {
            value["maximum"] = json!(1_048_576);
        }
    }
    defs["FormatterConfig"]["properties"]["languages"] =
        json!({"type":"object","additionalProperties":{"$ref":"#/$defs/FormatterOverride"}});
    defs["ProfileConfig"]["properties"]["tools"] = json!({"anyOf":[
        {"type":"array","items":{"type":"string"}},
        {"type":"object","additionalProperties":{"type":"boolean"}}
    ]});
    aliases(&mut defs["ProfileConfig"], "permissions permission");
    let properties = &mut schema["properties"];
    properties["permissions"] = permission_schema(false);
    properties["model"] = json!({"type":"string"});
    properties["instructions"] = json!({"anyOf":[
        {"type":"string"},{"type":"array","items":{"type":"string"}}
    ]});
    properties["mcp"] =
        json!({"type":"object","additionalProperties":{"$ref":"#/$defs/McpServerConfig"}});
    for name in ["formatter", "lsp"] {
        let object = std::mem::take(&mut properties[name]);
        let boolean = if name == "formatter" {
            json!({"type":"boolean"})
        } else {
            json!({"const":false})
        };
        properties[name] = json!({"anyOf":[boolean,object]});
    }
    aliases(&mut schema, "providers provider;agents agent;permissions permission;model_profile modelProfile model_profiles;hashline_edit hashlineEdit;model_roles modelRoles");
    schema.sort_all_objects();
    schema
}

fn permission_schema(profile: bool) -> Value {
    let mode = json!({"enum":["allow","ask","deny"]});
    let selectors = json!({"anyOf":[mode,{"type":"object","additionalProperties":mode}]});
    let mut properties = json!({});
    for name in [
        "bash",
        "shell",
        "edit",
        "task",
        "read",
        "external_directory",
    ] {
        properties[name] = selectors.clone();
    }
    for name in [
        "*",
        "network",
        "eval",
        "question",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
        "doom_loop",
    ] {
        properties[name] = mode.clone();
    }
    if profile {
        properties["todowrite"] = mode.clone();
    } else {
        for name in ["shell_allowlist", "shellAllowlist"] {
            properties[name] = json!({"$ref":"#/$defs/ShellAllowlist"});
        }
    }
    let canonical = if profile {
        "ProfilePermissions"
    } else {
        "PermissionsConfig"
    };
    let trigger = if profile {
        json!({"required":["rules"]})
    } else {
        json!({"anyOf":[{"required":["defaults"]},{"required":["rules"]}]})
    };
    json!({"anyOf":[mode,{"allOf":[{"$ref":format!("#/$defs/{canonical}")},trigger]},
        {"type":"object","additionalProperties":false,"properties":properties}]})
}

// Schemars describes serde's canonical names but not aliases.
fn aliases(schema: &mut Value, fields: &str) {
    if let Some(variants) = schema.get_mut("oneOf").and_then(Value::as_array_mut) {
        for variant in variants {
            aliases(variant, fields);
        }
    }
    for spellings in fields.split(';') {
        let names: Vec<_> = spellings.split_whitespace().collect();
        let Some(canonical) = names.first() else {
            continue;
        };
        let Some(value) = schema["properties"].get(canonical).cloned() else {
            continue;
        };
        for name in names.iter().skip(1) {
            schema["properties"][name] = value.clone();
        }
        let required = schema
            .get_mut("required")
            .and_then(Value::as_array_mut)
            .is_some_and(|required| {
                let present = required.iter().any(|v| v == canonical);
                required.retain(|v| v != canonical);
                present
            });
        let mut constraints = Vec::new();
        if required {
            constraints.push(json!({"anyOf":names.iter().map(|name| json!({"required":[name]})).collect::<Vec<_>>()}));
        }
        for (index, name) in names.iter().enumerate() {
            for other in names.iter().skip(index + 1) {
                constraints.push(json!({"not":{"required":[name,other]}}));
            }
        }
        if schema.get("allOf").is_none() {
            schema["allOf"] = json!([]);
        }
        if let Some(all) = schema["allOf"].as_array_mut() {
            all.extend(constraints);
        }
    }
}
