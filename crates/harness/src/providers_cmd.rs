//! Provider protocol catalog CLI surface (`harness providers protocols`).
//!
//! Prints the honest protocol capability catalog from
//! [`harness_core::provider_protocol`]. Only OpenAI-compatible is supported;
//! other protocols are catalogued as unsupported with explanatory notes. This
//! is a diagnostic readout, not protocol delivery.

use std::io::Write;

use clap::{Args, Subcommand};
use harness_core::provider_protocol::provider_protocol_catalog;

use crate::CliIo;

#[derive(Debug, Args, Clone)]
pub(crate) struct ProvidersCommand {
    #[command(subcommand)]
    command: ProvidersSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum ProvidersSubcommand {
    /// Print the provider protocol capability catalog (honest support levels).
    Protocols,
}

pub(crate) fn execute_with_io(command: ProvidersCommand, io: &mut CliIo<'_>) -> i32 {
    match command.command {
        ProvidersSubcommand::Protocols => run_protocols(io),
    }
}

fn run_protocols(io: &mut CliIo<'_>) -> i32 {
    let catalog = provider_protocol_catalog();
    match serde_json::to_string_pretty(&catalog) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "providers protocols: failed to serialize JSON: {err}"
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::io::Cursor;

    fn run_cli(args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = crate::CliDeps::real();
        let mut argv: Vec<String> = vec!["harness".to_string(), "providers".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn protocols_prints_honest_catalog_with_openai_and_anthropic_supported() {
        // arrange
        // act
        let (code, stdout, stderr) = run_cli(&["protocols"]);

        // assert — catalog JSON with honest support levels
        assert_eq!(code, 0, "stderr: {stderr}");
        let catalog: Vec<serde_json::Value> =
            serde_json::from_str(stdout.trim()).expect("valid JSON array");
        assert!(!catalog.is_empty(), "catalog must not be empty");

        let supported: Vec<_> = catalog
            .iter()
            .filter(|row| row["support"].as_str() == Some("supported"))
            .collect();
        assert_eq!(
            supported.len(),
            2,
            "exactly two protocols should be supported"
        );
        let protocols: Vec<&str> = supported
            .iter()
            .filter_map(|row| row["protocol"].as_str())
            .collect();
        assert!(
            protocols.contains(&"open_ai_compatible"),
            "open_ai_compatible must be supported"
        );
        assert!(
            protocols.contains(&"anthropic_messages"),
            "anthropic_messages must be supported"
        );

        for row in &catalog {
            if row["support"].as_str() == Some("unsupported") {
                assert!(
                    row["notes"].as_str().is_some_and(|n| !n.trim().is_empty()),
                    "unsupported rows carry honest notes"
                );
            }
        }
    }
}
