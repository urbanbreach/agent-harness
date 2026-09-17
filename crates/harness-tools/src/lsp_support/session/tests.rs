use super::*;

struct NoProcess;
impl LspChild for NoProcess {
    fn kill(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn wait(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn silent_server_and_unsettled_push_never_become_clean_at_deadline() {
    // An open channel with no messages models a server that stays alive but never answers.
    let (_sender, messages) = mpsc::sync_channel(1);
    let (stdin, _writes) = mpsc::sync_channel(16);
    let mut session = LspSession {
        child: Box::new(NoProcess),
        stdin,
        messages,
        next_id: 1,
        root: PathBuf::from("/workspace"),
        diagnostics: BTreeMap::new(),
        published: BTreeMap::new(),
        opened: BTreeMap::new(),
        next_document_version: 0,
        pull_results: BTreeMap::new(),
        control: SessionControl::default(),
        diagnostic_provider: None,
        save_options: None,
        initialized: false,
        server_quiescent: None,
        server_error: None,
        deadline: Instant::now(),
    };
    let result = session.request("textDocument/diagnostic", json!({}));
    assert!(matches!(result, Err(ToolError::Execution(message)) if message.contains("timed out")));
    let path = Path::new("/workspace/source.rs");
    // Even an empty versionless publish is not fresh until its quiescence window completes.
    session
        .diagnostics
        .insert(path.display().to_string(), Vec::new());
    session
        .published
        .insert(path.display().to_string(), (Instant::now(), false));
    let result = session.collect_diagnostics(path);
    assert!(
        matches!(result, Err(ToolError::Execution(message)) if message.contains("unavailable"))
    );
}

#[test]
fn lsp_framing_rejects_unbounded_headers_and_payloads() {
    for frame in [
        "x".repeat(8193),
        "Content-Length: 16777217\r\n\r\n".to_string(),
        "Content-Type: utf-8\r\n\r\n".to_string(),
    ] {
        assert!(read_message(&mut io::Cursor::new(frame)).is_err());
    }
}
