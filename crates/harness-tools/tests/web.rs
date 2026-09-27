use base64::Engine;
use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Mutex, Semaphore},
};
use tokio_stream::StreamExt;

const IMAGE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

#[tokio::test]
async fn fetching_checks_redirects_bounds_content_and_cancels_network_work(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::start().await?;
    let root = tempfile::tempdir()?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "webfetch".into(),
            pattern: format!("{}/blocked", fixture.origin).into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec!["webfetch".into()];
    config.agent_profiles.insert("default".into(), profile);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("web", root.path()).await?;
    let agent = handle
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let actor = EventActor::new(ActorKind::Worker, Some(agent));
    assert!(fixture.paths.lock().await.is_empty());
    let url = |path: &str| format!("{}{}", fixture.origin, path);
    let page = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "webfetch",
            json!({"url":url("/redirect"),"format":"markdown"}),
        )
        .await?;
    assert!(page.display_text.contains("# Page & title"));
    assert!(page.display_text.contains("[Reference](/target)"));
    assert!(!page.display_text.contains("SCRIPT_CONTENT"));
    let text = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "webfetch",
            json!({"url":url("/html"),"format":"text"}),
        )
        .await?;
    assert!(
        text.display_text.contains("Page & title") && !text.display_text.contains("[Reference]")
    );
    let image = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "webfetch",
            json!({"url":url("/image")}),
        )
        .await?;
    assert_eq!(image.attachments.len(), 1);
    assert_eq!(
        image.attachments[0].bytes()?,
        base64::engine::general_purpose::STANDARD.decode(IMAGE)?
    );
    for (path, bytes, suffix) in [
        ("/pdf", &b"%PDF-1.7\nPDF_WEB_BODY\n%%EOF"[..], ".pdf"),
        ("/binary", &[0; 8][..], ".bin"),
    ] {
        let output = handle
            .execute_agent_tool_call(actor.clone(), None, "webfetch", json!({"url":url(path)}))
            .await?;
        assert!(output.attachments.is_empty());
        assert!(output.display_text.contains("contents were not extracted"));
        let artifact = output.artifacts.first().ok_or("missing binary artifact")?;
        assert!(artifact.path.ends_with(suffix));
        assert_eq!(std::fs::read(run.run_dir.join(&artifact.path))?, bytes);
    }
    for path in ["/latin1", "/gzip"] {
        let page = handle
            .execute_agent_tool_call(actor.clone(), None, "webfetch", json!({"url":url(path)}))
            .await?;
        assert_eq!(page.display_text.trim(), "café", "{path}");
    }
    for path in [
        "/denied-redirect",
        "/large-header",
        "/large-stream",
        "/failure",
        "/gzip-large",
        "/unknown-charset",
        "/secret-pdf",
    ] {
        assert!(
            handle
                .execute_agent_tool_call(actor.clone(), None, "webfetch", json!({"url":url(path)}))
                .await
                .is_err(),
            "{path} should fail"
        );
    }
    assert!(!fixture
        .paths
        .lock()
        .await
        .iter()
        .any(|path| path == "/blocked"));
    assert!(handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "webfetch",
            json!({"url":url("/allowed/../blocked")})
        )
        .await
        .is_err_and(|e| e.contains("permission denied")));
    for url in [
        "file:///etc/passwd".into(),
        format!(
            "http://user:password@{}/html",
            fixture.origin.trim_start_matches("http://")
        ),
    ] {
        assert!(handle
            .execute_agent_tool_call(actor.clone(), None, "webfetch", json!({"url":url}))
            .await
            .is_err());
    }
    let mut events = handle.subscribe_new_events().await?;
    let pending = handle
        .request_tool_call(actor, None, "webfetch", json!({"url":url("/slow")}))
        .await?;
    tokio::time::timeout(Duration::from_secs(2), fixture.slow.acquire())
        .await??
        .forget();
    handle.cancel_task(&pending, "cancel fetch").await?;
    tokio::time::timeout(Duration::from_secs(2),async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload,EventV1::ToolCallFinished(t) if t.tool_call_id.as_str() == pending) {return Ok::<_,Box<dyn std::error::Error>>(());}
        }
        Err("fetch did not stop".into())
    }).await??;
    handle.stop_run().await?;
    let journal = std::fs::read_to_string(run.events_path)?;
    assert!(!journal.contains("ERROR_BODY_SECRET") && !journal.contains(IMAGE));
    assert!(!journal.contains("PDF_WEB_BODY") && !journal.contains("sk-private-pdf-credential"));
    Ok(())
}

struct Fixture {
    origin: String,
    paths: Arc<Mutex<Vec<String>>>,
    slow: Arc<Semaphore>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn start() -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let paths = Arc::new(Mutex::new(Vec::new()));
        let slow = Arc::new(Semaphore::new(0));
        let (requests, waiter) = (Arc::clone(&paths), Arc::clone(&slow));
        let server = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((socket,_)) = accepted else {break;};
                        let (paths,slow) = (Arc::clone(&requests),Arc::clone(&waiter));
                        clients.spawn(async move {
                            let mut input = BufReader::new(socket);
                            let mut line = String::new();
                            input.read_line(&mut line).await?;
                            let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                            loop { line.clear(); if input.read_line(&mut line).await? == 0 || line == "\r\n" {break;} }
                            paths.lock().await.push(path.clone());
                            let mut socket = input.into_inner();
                            if matches!(path.as_str(), "/latin1" | "/gzip" | "/gzip-large" | "/unknown-charset" | "/binary" | "/pdf" | "/secret-pdf") {
                                use std::io::Write;
                                let mut headers = "Content-Type: text/plain; CHARSET=\"windows-1252\"\r\n";
                                let body = match path.as_str() {
                                    "/gzip" | "/gzip-large" => {
                                        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
                                        if path == "/gzip-large" { gzip.write_all(&vec![b'x';6*1024*1024])?; }
                                        else { gzip.write_all("café".as_bytes())?; }
                                        headers = "Content-Type: text/plain; charset=utf-8\r\nContent-Encoding: gzip\r\n";
                                        gzip.finish()?
                                    }
                                    "/unknown-charset" => { headers = "Content-Type: text/plain; charset=unknown-encoding\r\n"; b"hello".to_vec() },
                                    "/binary" => { headers = "Content-Type: application/octet-stream\r\n"; vec![0;8] },
                                    "/pdf" => { headers = "Content-Type: application/pdf\r\n"; b"%PDF-1.7\nPDF_WEB_BODY\n%%EOF".to_vec() },
                                    "/secret-pdf" => { headers = "Content-Type: application/pdf\r\n"; b"%PDF-1.7\nsk-private-pdf-credential\n%%EOF".to_vec() },
                                    _ => b"caf\xe9".to_vec(),
                                };
                                socket.write_all(format!("HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await?;
                                return socket.write_all(&body).await;
                            }
                            if path == "/image" {
                                let body = base64::engine::general_purpose::STANDARD.decode(IMAGE).map_err(std::io::Error::other)?;
                                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await?;
                                return socket.write_all(&body).await;
                            }
                            if path == "/slow" { slow.add_permits(1); let _ = socket.read_u8().await; return Ok::<_,std::io::Error>(()); }
                            if path == "/large-header" { return socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6000000\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n").await; }
                            if path == "/large-stream" {
                                socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n").await?;
                                let chunk = vec![b'x';1024*1024];
                                for _ in 0..6 { socket.write_all(b"100000\r\n").await?; socket.write_all(&chunk).await?; socket.write_all(b"\r\n").await?; }
                                return socket.write_all(b"0\r\n\r\n").await;
                            }
                            let (status,headers,body) = match path.as_str() {
                                "/redirect" => ("302 Found","Location: /html\r\n",""),
                                "/denied-redirect" => ("302 Found","Location: /blocked\r\n",""),
                                "/failure" => ("500 Internal Server Error","","ERROR_BODY_SECRET"),
                                _ => ("200 OK","Content-Type: text/html; charset=utf-8\r\n","<html><head><script>SCRIPT_CONTENT</script></head><body><h1>Page &amp; title</h1><p><a href='/target'>Reference</a></p></body></html>"),
                            };
                            socket.write_all(format!("HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await
                        });
                    }
                    _ = clients.join_next(), if !clients.is_empty() => {}
                }
            }
        });
        Ok(Self {
            origin,
            paths,
            slow,
            server,
        })
    }
}
