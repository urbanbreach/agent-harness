//! Integration matrix tests (Task 22).
//!
//! Each integration family gets one real boundary E2E plus bad input,
//! permission denial, process failure, cancellation/restart, and redaction
//! coverage. Families covered here: hooks, plugins, ACP, code graph.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use harness_core::code_graph::{
    build_persistent_graph_index, query_persistent_graph, GraphQuery, GraphQueryKind,
};
use harness_core::config::{
    load_config_from_str, HookLifecycleEvent, HookRuntimeConfig, HooksConfig, LifecycleHookConfig,
    ShellAllowlist,
};
use harness_core::coord::{
    LifecycleHookCommandExecutor, LifecycleHookCommandInvocation, LifecycleHookCommandOutput,
    TokioLifecycleHookCommandExecutor,
};
use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
use harness_core::integrations::acp_stdio::run_stdio_acp_agent_mode_product;
use harness_core::integrations::{
    AcpConnection, AcpConnectionState, AcpError, MockAcpTransport, PluginActivationPermission,
    PluginEnablement, PluginLifecycleError, PluginLifecycleRegistry, PLUGIN_ENTRY_FILE_NAME,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME,
    PLUGIN_REGISTRY_REL, PLUGIN_SKILLS_DIR_NAME,
};
use harness_core::UnwrapOrAbort;
use serde_json::json;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Hooks family
// ---------------------------------------------------------------------------

fn config_with_hooks_json(hooks_json: &str) -> String {
    format!(
        r#"
        {{
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-5.4-mini": {{
                  display_name: "GPT-5.4 Mini",
                }},
              }},
            }},
          }},
          model: "default/gpt-5.4-mini",
          agent: {{
            default: {{
              tools: ["fs.read"],
            }},
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
          hooks: {{
            lifecycle: {hooks_json}
          }},
        }}
        "#
    )
}

include!("integrations_matrix/01_hooks_test.rs");
include!("integrations_matrix/02_plugins_test.rs");
include!("integrations_matrix/03_acp_test.rs");
include!("integrations_matrix/04_code_graph_test.rs");
