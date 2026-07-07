use super::session::{LspChild, LspProcess, LspProcessStarter, LspSession};
use super::{
    server_for_path, LspOperation, LspPosition, LspServerSpec, SUPPORTED_LSP_OPERATION_NAMES,
};
use crate::workspace_paths::file_uri_from_path;
use crate::UnwrapOrAbort;
use harness_core::config::LspConfig;
use harness_core::tool::ToolError;
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::Path;

struct FakeLspChild {
    killed: bool,
    waited: bool,
}

impl LspChild for FakeLspChild {
    fn kill(&mut self) -> io::Result<()> {
        self.killed = true;
        Ok(())
    }

    fn wait(&mut self) -> io::Result<()> {
        self.waited = true;
        Ok(())
    }
}

struct FakeLspStarter {
    started: RefCell<Vec<(Vec<String>, std::path::PathBuf)>>,
}

impl FakeLspStarter {
    fn new() -> Self {
        Self {
            started: RefCell::new(Vec::new()),
        }
    }
}

impl LspProcessStarter for FakeLspStarter {
    fn start(&self, spec: &LspServerSpec, root: &Path) -> Result<LspProcess, ToolError> {
        self.started
            .borrow_mut()
            .push((spec.command.clone(), root.to_path_buf()));
        Ok(LspProcess {
            child: Box::new(FakeLspChild {
                killed: false,
                waited: false,
            }),
            stdin: Box::new(Vec::<u8>::new()),
            stdout: Box::new(io::Cursor::new(lsp_startup_responses())),
        })
    }
}

fn lsp_message(body: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&body).unwrap_or_abort();
    let mut message = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    message.extend(body);
    message
}

fn lsp_startup_responses() -> Vec<u8> {
    lsp_message(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"capabilities": {}}
    }))
}

#[test]
fn file_uri_from_path_percent_encodes_spaces() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let path = tempdir.path().join("space file.rs");
    fs::write(&path, "fn demo() {}\n").unwrap_or_abort();
    let uri = file_uri_from_path(&path);
    assert!(uri.starts_with("file://"));
    assert!(uri.contains("space%20file.rs"));
}

#[test]
fn lsp_position_translates_one_based_to_zero_based() {
    let position = LspPosition::from_one_based(3, 9).unwrap_or_abort();
    assert_eq!(position.line(), 2);
    assert_eq!(position.character(), 8);
}

#[test]
fn lsp_position_rejects_non_positive_coordinates() {
    let err = LspPosition::from_one_based(0, 1).expect_err("line must be >= 1");
    assert!(
        matches!(err, ToolError::InvalidArguments(message) if message == "line and character must be >= 1")
    );
}

#[test]
fn lsp_operation_parse_rejects_unsupported_values_with_stable_message() {
    let err = LspOperation::parse("renameSymbol").expect_err("operation should fail");
    assert!(
        matches!(err, ToolError::InvalidArguments(message) if message == format!(
            "unsupported lsp operation: renameSymbol; use lsp.rename for the explicit workspace-editing rename flow; supported operations: {}",
            SUPPORTED_LSP_OPERATION_NAMES.join(", ")
        ))
    );
}

#[test]
fn server_for_path_rejects_unsupported_extension_with_stable_message() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let path = tempdir.path().join("fixture.lua");
    fs::write(&path, "print('hello')\n").unwrap_or_abort();
    let err = match server_for_path(&path, &LspConfig::default()) {
        Ok(_) => panic!("lua should be unsupported"),
        Err(err) => err,
    };
    assert!(
        matches!(err, ToolError::InvalidArguments(message) if message.contains("unsupported lsp language extension: .lua"))
    );
}

#[test]
fn lsp_session_start_can_use_injected_process_starter_without_spawning() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let starter = FakeLspStarter::new();
    let spec = LspServerSpec::builtin("rust", &["fake-lsp", "--stdio"], &[".rs"], &[]);

    let session = LspSession::start_with_starter(&spec, tempdir.path(), &starter).unwrap_or_abort();

    assert_eq!(session.next_id, 2);
    assert_eq!(starter.started.borrow().len(), 1);
    assert_eq!(starter.started.borrow()[0].0, vec!["fake-lsp", "--stdio"]);
    assert_eq!(starter.started.borrow()[0].1, tempdir.path());
}

#[test]
fn lsp_operation_supported_names_match_roundtrip_strings() {
    for operation in SUPPORTED_LSP_OPERATION_NAMES {
        let parsed = LspOperation::parse(operation).unwrap_or_abort();
        assert_eq!(parsed.as_str(), *operation);
    }
}
