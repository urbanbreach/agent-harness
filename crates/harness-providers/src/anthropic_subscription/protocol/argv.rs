//! SDK `ProcessTransport` argv, child environment and the initialize request.
use super::*;

impl QueryOptions {
    pub fn set_extra_arg(&mut self, name: &str, value: Option<&str>) {
        let value = value.map(str::to_owned);
        match self.extra_args.iter_mut().find(|(key, _)| key == name) {
            Some(entry) => entry.1 = value,
            None => self.extra_args.push((name.into(), value)),
        }
    }

    /// SDK `ProcessTransport.initialize` argv.
    pub fn argv(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
        ]
        .map(str::to_owned)
        .into();
        match &self.thinking {
            Some(Thinking::Adaptive { display }) => {
                args.extend(["--thinking".into(), "adaptive".into()]);
                args.extend(["--thinking-display".into(), display.clone()]);
            }
            Some(Thinking::Budget(0)) => args.extend(["--thinking".into(), "disabled".into()]),
            Some(Thinking::Budget(tokens)) => {
                args.extend(["--max-thinking-tokens".into(), tokens.to_string()]);
            }
            None => {}
        }
        if let Some(effort) = &self.effort {
            args.extend(["--effort".into(), effort.clone()]);
        }
        if let Some(turns) = self.max_turns {
            args.extend(["--max-turns".into(), turns.to_string()]);
        }
        args.extend(["--model".into(), self.model.clone()]);
        args.extend(["--permission-prompt-tool".into(), "stdio".into()]);
        if let Some(resume) = &self.resume {
            args.push(format!("--resume={resume}"));
        }
        args.extend(["--tools".into(), self.tools.join(",")]);
        args.push(format!(
            "--setting-sources={}",
            self.setting_sources.join(",")
        ));
        args.extend(["--permission-mode".into(), self.permission_mode.clone()]);
        if self.include_partial_messages {
            args.push("--include-partial-messages".into());
        }
        if self.fork_session {
            args.push("--fork-session".into());
        }
        if let Some(at) = &self.resume_session_at {
            args.push(format!("--resume-session-at={at}"));
        }
        if let Some(id) = &self.session_id {
            args.push(format!("--session-id={id}"));
        }
        let settings = self.settings.to_string();
        let extra = self
            .extra_args
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_deref()))
            .chain(std::iter::once(("settings", Some(settings.as_str()))));
        for (name, value) in extra {
            match value {
                None => args.push(format!("--{name}")),
                Some(value) if value.len() > 1 && value.starts_with('-') => {
                    args.push(format!("--{name}={value}"));
                }
                Some(value) => args.extend([format!("--{name}"), value.into()]),
            }
        }
        args
    }

    /// The child environment `query()` and the transport derive from `options.env`.
    pub fn child_env(&self) -> BTreeMap<String, String> {
        let mut env = self.env.clone();
        env.entry("CLAUDE_CODE_ENTRYPOINT".into())
            .or_insert_with(|| "sdk-ts".into());
        env.entry("CLAUDE_AGENT_SDK_VERSION".into())
            .or_insert_with(|| SDK_VERSION.into());
        if !env
            .keys()
            .any(|key| key.eq_ignore_ascii_case("CLAUDE_CODE_SDK_READS_SESSION_STATE"))
        {
            env.insert("CLAUDE_CODE_SDK_READS_SESSION_STATE".into(), "1".into());
        }
        env.remove("NODE_OPTIONS");
        if env
            .get("DEBUG_CLAUDE_AGENT_SDK")
            .is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
        {
            env.insert("DEBUG".into(), "1".into());
        } else {
            env.remove("DEBUG");
        }
        env
    }

    pub fn initialize_request(&self) -> Value {
        let mut request = json!({
            "subtype": "initialize",
            "hooks": {"PreToolUse": [{
                "matcher": HOST_CAPTURED_SDK_TOOL_MATCHER,
                "hookCallbackIds": ["hook_0"],
            }]},
        });
        if !self.custom_tools.is_empty() {
            request["sdkMcpServers"] = json!([CUSTOM_TOOLS_MCP_SERVER_NAME]);
            request["sdkMcpServerManifests"] = json!({CUSTOM_TOOLS_MCP_SERVER_NAME: {
                "initializeResult": mcp_initialize_result(MCP_PROTOCOL_VERSION),
                "toolsListResult": self.tools_list_result(),
            }});
        }
        match &self.system_prompt {
            SystemPrompt::Custom(prompt) => request["systemPrompt"] = json!([prompt]),
            SystemPrompt::Preset { append } => {
                if let Some(append) = append {
                    request["appendSystemPrompt"] = json!(append);
                }
            }
        }
        request
    }

    pub(super) fn tools_list_result(&self) -> Value {
        let tools: Vec<_> = self
            .custom_tools
            .iter()
            .map(|tool| {
                let mut entry = json!({
                    "name": tool.name,
                    "inputSchema": tool.input_schema,
                    "execution": {"taskSupport": "forbidden"},
                });
                if let Some(description) = &tool.description {
                    entry["description"] = json!(description);
                }
                entry
            })
            .collect();
        json!({"tools": tools})
    }
}
