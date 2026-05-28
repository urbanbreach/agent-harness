use std::process::Command;

use serde_json::{json, Value};

pub(crate) fn ast_grep_adapter_readiness() -> Value {
    match Command::new("ast-grep").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            json!({
                "available": true,
                "command": "ast-grep",
                "version": version,
                "no_network_probes": true,
            })
        }
        Ok(output) => json!({
            "available": false,
            "command": "ast-grep",
            "status": output.status.code(),
            "stderr": String::from_utf8_lossy(&output.stderr).trim().chars().take(500).collect::<String>(),
            "no_network_probes": true,
        }),
        Err(err) => json!({
            "available": false,
            "command": "ast-grep",
            "error": err.to_string(),
            "no_network_probes": true,
        }),
    }
}
