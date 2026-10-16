use super::*;
pub(crate) fn execute(
    mock: bool,
    mock_script: Option<PathBuf>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    context: ConfigLoadContext,
) -> Result<(), String> {
    let script = mock_script
        .as_deref()
        .map(mock_script::Script::read)
        .transpose()?;
    let mock = mock || script.is_some();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(serve(
            tokio::io::stdin(),
            tokio::io::stdout(),
            mock,
            script,
            config_path,
            session_dir,
            context,
        ))
}

pub(super) async fn serve<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: R,
    mut writer: W,
    mock: bool,
    mock_script: Option<mock_script::Script>,
    config_path: Option<PathBuf>,
    session_dir: Option<PathBuf>,
    config_context: ConfigLoadContext,
) -> Result<(), String> {
    let (output, mut messages) = mpsc::unbounded_channel::<Value>();
    let server = Arc::new(Server {
        config_path,
        session_dir,
        config_context,
        mock,
        mock_script,
        settings: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        sessions: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
        output,
        auth_attempt: Arc::new(Mutex::new(0)),
        auth_sequence: AtomicU64::new(0),
    });
    let producer = async {
        let mut lines = BufReader::new(reader).lines();
        let mut requests = tokio::task::JoinSet::new();
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            requests.spawn(process_request(Arc::clone(&server), line));
            while requests.try_join_next().is_some() {}
        }
        while let Some(result) = requests.join_next().await {
            result.map_err(|e| e.to_string())?;
        }
        super::super::auth_backend::send_tui_auth_backend_input(None);
        let ids = server
            .sessions
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut stop_error = None;
        for id in ids {
            if let Err(error) = server.close(&id).await {
                stop_error.get_or_insert(error);
            }
        }
        match stop_error {
            Some(error) => Err(error),
            None => Ok::<_, String>(()),
        }
    };
    tokio::pin!(producer);
    loop {
        tokio::select! {
            result=&mut producer=> { result?; while let Ok(message)=messages.try_recv() { write_message(&mut writer,&message).await?; } return Ok(()); },
            Some(message)=messages.recv()=>write_message(&mut writer,&message).await?,
        }
    }
}
pub(super) async fn write_message(
    writer: &mut (impl AsyncWrite + Unpin),
    message: &Value,
) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())
}
async fn process_request(server: Arc<Server>, line: String) {
    let request = serde_json::from_str::<Value>(&line);
    let id = request
        .as_ref()
        .ok()
        .and_then(|r| r.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    let result = match request {
        Ok(request) if id.as_u64().is_some_and(|id| id > 0) => {
            match request.get("method").and_then(Value::as_str) {
                Some(method) => {
                    server
                        .handle(method, request.get("params").unwrap_or(&json!({})))
                        .await
                }
                None => Err("invalid_params: method is required".into()),
            }
        }
        _ => Err("invalid_params: expected a JSON request with a positive integer id".into()),
    };
    let response = match result {
        Ok(value) => json!({"id":id,"result":value}),
        Err(message) => {
            let (code, message) = message
                .split_once(": ")
                .filter(|(code, _)| matches!(*code, "invalid_params" | "not_found"))
                .unwrap_or(("failed", &message));
            json!({"id":id,"error":{"code":code,"message":message}})
        }
    };
    let _ = server.output.send(response);
}
pub(crate) fn launch() -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = std::env::var_os("HARNESS_GUI")
        .map(PathBuf::from)
        .unwrap_or_else(|| current.with_file_name("harness-gui"));
    if !path.is_file() {
        return Err(format!(
            "GUI executable not found at {}; set HARNESS_GUI to the desktop executable",
            path.display()
        ));
    }
    let status = std::process::Command::new(&path)
        .env("HARNESS_BIN", current)
        .status()
        .map_err(|e| format!("could not launch {}: {e}", path.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("GUI exited with {status}"))
    }
}
