use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

pub const DISCLOSURE_SENTINEL: &str = "PACKET2_DISCLOSURE_SENTINEL";
pub const DISCLOSURE_BODY: &str = "PACKET2_DISCLOSURE_BODY";
pub const STREAM_SENTINEL: &str = "PACKET2_STREAM_SENTINEL";
pub const PACKET3_STREAM_REST: &str = "I inspected the requested stream.";
pub const PACKET3_STREAM_MID: &str = "The deterministic output rendered correctly.";
pub const PACKET3_STREAM_SETTLED: &str =
    "The stream probe is complete; all requested work is finished.";
pub const DELTA_COUNT: usize = 10_000;
pub const DELTA_CADENCE: Duration = Duration::from_millis(2);
pub const ISOLATED_COMMAND: &str =
    "printf 'PACKET2_DISCLOSURE_SENTINEL\\nfiller-1\\nfiller-2\\nfiller-3\\nfiller-4\\nfiller-5\\nfiller-6\\nfiller-7\\nfiller-8\\n%s%s\\nline-2\\n' PACKET2_ DISCLOSURE_BODY; exit 7";
const GROK_ISOLATED_COMMAND: &str =
    "printf 'PACKET2_DISCLOSURE_SENTINEL\\n................................................................................................................................\\nPACKET2_DISCLOSURE_BODY\\nline-2\\n'; exit 7";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Packet2FixtureTrace {
    pub request_count: usize,
    pub request_paths: Vec<String>,
    pub advertised_tools: Vec<String>,
    pub negotiated_tool: Option<String>,
    pub tool_result_observed: bool,
    pub delta_count: usize,
    pub first_delta_micros: Option<u128>,
    pub last_delta_micros: Option<u128>,
}

#[derive(Debug, thiserror::Error)]
pub enum Packet2FixtureError {
    #[error("fixture I/O: {0}")]
    Io(String),
    #[error("fixture protocol: {0}")]
    Protocol(String),
    #[error("fixture worker panicked")]
    WorkerPanicked,
}

pub struct Packet2FixtureServer {
    address: SocketAddr,
    trace: Arc<Mutex<Packet2FixtureTrace>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), Packet2FixtureError>>>,
}

#[derive(Clone, Copy)]
enum FixtureStream {
    Sustained,
    Packet3Stream,
    Packet3Settled,
}

struct HttpRequest {
    path: String,
    body: String,
    auxiliary: bool,
}

impl Packet2FixtureServer {
    pub fn start() -> Result<Self, Packet2FixtureError> {
        Self::start_with_stream(FixtureStream::Sustained)
    }

    pub fn start_packet3(scenario_id: &str) -> Result<Self, Packet2FixtureError> {
        Self::start_with_stream(packet3_stream_mode(scenario_id))
    }

    fn start_with_stream(stream_mode: FixtureStream) -> Result<Self, Packet2FixtureError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
        let address = listener.local_addr().map_err(io_error)?;
        let trace = Arc::new(Mutex::new(Packet2FixtureTrace::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_trace = Arc::clone(&trace);
        let worker_stop = Arc::clone(&stop);
        let worker =
            std::thread::spawn(move || serve(listener, &worker_trace, &worker_stop, stream_mode));
        Ok(Self {
            address,
            trace,
            stop,
            worker: Some(worker),
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    pub fn trace(&self) -> Packet2FixtureTrace {
        self.trace.lock().map_or_else(
            |poisoned| poisoned.into_inner().clone(),
            |trace| trace.clone(),
        )
    }

    pub fn finish(mut self) -> Result<Packet2FixtureTrace, Packet2FixtureError> {
        self.stop.store(true, Ordering::Release);
        let worker = self
            .worker
            .take()
            .ok_or_else(|| Packet2FixtureError::Protocol("worker missing".into()))?;
        worker
            .join()
            .map_err(|_| Packet2FixtureError::WorkerPanicked)??;
        Ok(self.trace())
    }
}

fn serve(
    listener: TcpListener,
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    stop: &AtomicBool,
    stream_mode: FixtureStream,
) -> Result<(), Packet2FixtureError> {
    listener.set_nonblocking(true).map_err(io_error)?;
    if matches!(stream_mode, FixtureStream::Packet3Stream) {
        return serve_packet3_stream(listener, trace, stop);
    }
    let (mut first, tool, responses_api) = loop {
        let Some(mut stream) = accept(&listener, stop)? else {
            return Ok(());
        };
        let request = read_request(&mut stream)?;
        update_trace(trace, |value| {
            value.request_paths.push(request.path.clone());
        });
        if !is_generation_path(&request.path) {
            write_json_response(&mut stream)?;
            continue;
        }
        let (tool, names) = advertised_shell_tool(&request.body)?;
        update_trace(trace, |value| value.advertised_tools.extend(names));
        if let Some(tool) = tool {
            break (stream, tool, request.path.ends_with("/responses"));
        }
        send_simple_stream(&mut stream, request.path.ends_with("/responses"))?;
    };
    update_trace(trace, |value| {
        value.request_count = 1;
        value.negotiated_tool = Some(tool.clone());
    });
    send_tool_call(&mut first, &tool, responses_api, stream_mode)?;
    drop(first);

    let (mut second, second_request) = loop {
        let Some(mut stream) = accept(&listener, stop)? else {
            return Ok(());
        };
        let request = read_request(&mut stream)?;
        update_trace(trace, |value| {
            value.request_paths.push(request.path.clone());
        });
        if is_tool_result_request(&request.body) {
            break (stream, request);
        }
        if is_generation_path(&request.path) {
            send_simple_stream(&mut stream, request.path.ends_with("/responses"))?;
        } else {
            write_json_response(&mut stream)?;
        }
    };
    update_trace(trace, |value| {
        value.request_count = 2;
        value.tool_result_observed = true;
    });
    let responses_api = second_request.path.ends_with("/responses");
    match stream_mode {
        FixtureStream::Sustained => send_paced_stream(&mut second, trace, stop, responses_api),
        FixtureStream::Packet3Settled => send_packet3_settled(&mut second, trace, responses_api),
        FixtureStream::Packet3Stream => Err(Packet2FixtureError::Protocol(
            "Packet 3 stream reached tool-result flow".to_owned(),
        )),
    }
}

fn packet3_stream_mode(scenario_id: &str) -> FixtureStream {
    if scenario_id.starts_with("packet3-baseline-stream--") {
        FixtureStream::Packet3Stream
    } else {
        FixtureStream::Packet3Settled
    }
}

fn serve_packet3_stream(
    listener: TcpListener,
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    stop: &AtomicBool,
) -> Result<(), Packet2FixtureError> {
    let mut foreground_served = false;
    loop {
        let Some(mut stream) = accept(&listener, stop)? else {
            return Ok(());
        };
        let request = match read_request(&mut stream) {
            Err(Packet2FixtureError::Protocol(detail))
                if stop.load(Ordering::Acquire) && detail == "incomplete HTTP request" =>
            {
                return Ok(());
            }
            result => result?,
        };
        update_trace(trace, |value| {
            value.request_paths.push(request.path.clone())
        });
        if is_generation_path(&request.path)
            && !request.auxiliary
            && !foreground_served
            && is_packet3_stream_request(&request.body)
        {
            update_trace(trace, |value| value.request_count = 1);
            send_packet3_stream(&mut stream, trace, request.path.ends_with("/responses"))?;
            foreground_served = true;
            continue;
        }
        if is_generation_path(&request.path) {
            send_auxiliary_stream(&mut stream, request.path.ends_with("/responses"))?;
        } else {
            write_json_response(&mut stream)?;
        }
    }
}

fn is_packet3_stream_request(body: &str) -> bool {
    body.contains("stream probe")
}

fn send_packet3_stream(
    stream: &mut TcpStream,
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    responses_api: bool,
) -> Result<(), Packet2FixtureError> {
    write_headers(stream)?;
    if responses_api {
        write_sse(stream, &response_created("packet3-stream").to_string())?;
    }
    let started = Instant::now();
    for (ordinal, text) in [
        PACKET3_STREAM_REST,
        PACKET3_STREAM_MID,
        PACKET3_STREAM_SETTLED,
    ]
    .into_iter()
    .enumerate()
    {
        let separator = if ordinal < 2 { " " } else { "" };
        let chunk = if responses_api {
            serde_json::json!({"type":"response.output_text.delta","sequence_number":ordinal + 1,"item_id":"packet3-stream-message","output_index":0,"content_index":0,"delta":format!("{text}{separator}")})
        } else {
            serde_json::json!({"id":"packet3-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":format!("{text}{separator}")},"finish_reason":null}]})
        };
        write_sse(stream, &chunk.to_string())?;
        let elapsed = started.elapsed().as_micros();
        update_trace(trace, |value| {
            value.delta_count += 1;
            value.first_delta_micros.get_or_insert(elapsed);
            value.last_delta_micros = Some(elapsed);
        });
        if ordinal < 2 {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    if responses_api {
        let text = format!("{PACKET3_STREAM_REST} {PACKET3_STREAM_MID} {PACKET3_STREAM_SETTLED}");
        let output = vec![
            serde_json::json!({"type":"message","id":"packet3-stream-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}),
        ];
        write_sse(
            stream,
            &response_completed_at("packet3-stream", 4, output).to_string(),
        )?;
    }
    ignore_client_disconnect(write_sse(stream, "[DONE]"))
}

fn send_packet3_settled(
    stream: &mut TcpStream,
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    responses_api: bool,
) -> Result<(), Packet2FixtureError> {
    const TEXT: &str = "Packet 3 recovery complete — 漢字かなカナ";
    update_trace(trace, |value| {
        value.delta_count = 1;
        value.first_delta_micros = Some(0);
        value.last_delta_micros = Some(0);
    });
    write_headers(stream)?;
    if responses_api {
        let output = vec![serde_json::json!({
            "type":"message",
            "id":"packet3-message",
            "role":"assistant",
            "status":"completed",
            "content":[{"type":"output_text","text":TEXT,"annotations":[]}]
        })];
        for event in [
            response_created("packet3-settled"),
            serde_json::json!({"type":"response.output_text.delta","sequence_number":1,"item_id":"packet3-message","output_index":0,"content_index":0,"delta":TEXT}),
            response_completed("packet3-settled", output),
        ] {
            write_sse(stream, &event.to_string())?;
        }
    } else {
        let chunk = serde_json::json!({"id":"packet3-settled","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":TEXT},"finish_reason":"stop"}]});
        write_sse(stream, &chunk.to_string())?;
    }
    write_sse(stream, "[DONE]")
}

fn is_tool_result_request(body: &str) -> bool {
    body.contains("packet2-call")
        && (body.contains("function_call_output") || body.contains("\"role\":\"tool\""))
}

fn accept(
    listener: &TcpListener,
    stop: &AtomicBool,
) -> Result<Option<TcpStream>, Packet2FixtureError> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(Some(stream)),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(io_error(error)),
        }
    }
}

fn advertised_shell_tool(body: &str) -> Result<(Option<String>, Vec<String>), Packet2FixtureError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| Packet2FixtureError::Protocol(format!("invalid request JSON: {error}")))?;
    let names = value
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            tool.pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| tool.get("name").and_then(serde_json::Value::as_str))
        })
        .collect::<Vec<_>>();
    let shell = names
        .iter()
        .find(|name| {
            matches!(
                **name,
                "bash" | "shell" | "shell_command" | "run_terminal_command"
            )
        })
        .map(|name| (*name).to_owned());
    Ok((shell, names.into_iter().map(str::to_owned).collect()))
}

fn send_simple_stream(
    stream: &mut TcpStream,
    responses_api: bool,
) -> Result<(), Packet2FixtureError> {
    write_headers(stream)?;
    if responses_api {
        return write_responses_text(stream, "Packet 2");
    }
    let chunk = serde_json::json!({"id":"packet2-bootstrap","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Packet 2"},"finish_reason":"stop"}]});
    write_sse(stream, &chunk.to_string())?;
    write_sse(stream, "[DONE]")
}

fn send_auxiliary_stream(
    stream: &mut TcpStream,
    responses_api: bool,
) -> Result<(), Packet2FixtureError> {
    ignore_client_disconnect(send_simple_stream(stream, responses_api))
}

fn ignore_client_disconnect(
    result: Result<(), Packet2FixtureError>,
) -> Result<(), Packet2FixtureError> {
    match result {
        Err(Packet2FixtureError::Io(detail))
            if detail.contains("Broken pipe") || detail.contains("Connection reset") =>
        {
            Ok(())
        }
        result => result,
    }
}

fn send_tool_call(
    stream: &mut TcpStream,
    tool: &str,
    responses_api: bool,
    stream_mode: FixtureStream,
) -> Result<(), Packet2FixtureError> {
    write_headers(stream)?;
    let command = fixture_command(tool, stream_mode);
    let arguments = serde_json::to_string(&serde_json::json!({"command": command}))
        .map_err(|error| Packet2FixtureError::Protocol(error.to_string()))?;
    if responses_api {
        let created = response_created("packet2-tool-response");
        let added = serde_json::json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"function_call","id":"packet2-item","call_id":"packet2-call","name":tool,"arguments":""}});
        let delta = serde_json::json!({"type":"response.function_call_arguments.delta","sequence_number":2,"item_id":"packet2-item","output_index":0,"delta":arguments});
        let done = serde_json::json!({"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":{"type":"function_call","id":"packet2-item","call_id":"packet2-call","name":tool,"arguments":arguments}});
        let completed = response_completed(
            "packet2-tool-response",
            vec![
                serde_json::json!({"type":"function_call","id":"packet2-item","call_id":"packet2-call","name":tool,"arguments":arguments}),
            ],
        );
        for event in [created, added, delta, done, completed] {
            write_sse(stream, &event.to_string())?;
        }
        return write_sse(stream, "[DONE]");
    }
    let chunk = serde_json::json!({"id":"packet2-tool","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"packet2-call","type":"function","function":{"name":tool,"arguments":arguments}}]},"finish_reason":"tool_calls"}]});
    write_sse(stream, &chunk.to_string())?;
    write_sse(stream, "[DONE]")
}

fn fixture_command(tool: &str, stream_mode: FixtureStream) -> &'static str {
    if matches!(stream_mode, FixtureStream::Packet3Settled) {
        ISOLATED_COMMAND
    } else if tool == "run_terminal_command" {
        GROK_ISOLATED_COMMAND
    } else {
        ISOLATED_COMMAND
    }
}

fn send_paced_stream(
    stream: &mut TcpStream,
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    stop: &AtomicBool,
    responses_api: bool,
) -> Result<(), Packet2FixtureError> {
    write_headers(stream)?;
    if responses_api {
        write_sse(stream, &response_created("packet2-stream").to_string())?;
    }
    let started = Instant::now();
    for ordinal in 0..DELTA_COUNT {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let content = if ordinal == 1_024 {
            STREAM_SENTINEL.to_owned()
        } else {
            "\u{200b}".to_owned()
        };
        let chunk = if responses_api {
            serde_json::json!({"type":"response.output_text.delta","sequence_number":ordinal + 1,"item_id":"packet2-message","output_index":0,"content_index":0,"delta":content})
        } else {
            serde_json::json!({"id":"packet2-stream","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":content},"finish_reason":null}]})
        };
        if let Err(Packet2FixtureError::Io(detail)) = write_sse(stream, &chunk.to_string()) {
            if detail.contains("Broken pipe") || detail.contains("Connection reset") {
                return Ok(());
            }
            return Err(Packet2FixtureError::Io(detail));
        }
        let elapsed = started.elapsed().as_micros();
        update_trace(trace, |value| {
            value.delta_count += 1;
            value.first_delta_micros.get_or_insert(elapsed);
            value.last_delta_micros = Some(elapsed);
        });
        if ordinal % 16 == 15 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if responses_api {
        let output = vec![
            serde_json::json!({"type":"message","id":"packet2-message","role":"assistant","status":"completed","content":[]} ),
        ];
        write_sse(
            stream,
            &response_completed("packet2-stream", output).to_string(),
        )?;
    }
    write_sse(stream, "[DONE]")
}

fn write_responses_text(stream: &mut TcpStream, text: &str) -> Result<(), Packet2FixtureError> {
    let output = vec![
        serde_json::json!({"type":"message","id":"packet2-bootstrap-message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]}),
    ];
    for event in [
        response_created("packet2-bootstrap"),
        serde_json::json!({"type":"response.output_text.delta","sequence_number":1,"item_id":"packet2-bootstrap-message","output_index":0,"content_index":0,"delta":text}),
        response_completed("packet2-bootstrap", output),
    ] {
        write_sse(stream, &event.to_string())?;
    }
    write_sse(stream, "[DONE]")
}

fn response_created(id: &str) -> serde_json::Value {
    serde_json::json!({"type":"response.created","sequence_number":0,"response":{"id":id,"object":"response","created_at":1,"model":"fixture","status":"in_progress","output":[]}})
}

fn response_completed(id: &str, output: Vec<serde_json::Value>) -> serde_json::Value {
    response_completed_at(id, DELTA_COUNT + 1, output)
}

fn response_completed_at(
    id: &str,
    sequence_number: usize,
    output: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({"type":"response.completed","sequence_number":sequence_number,"response":{"id":id,"object":"response","created_at":1,"model":"fixture","status":"completed","output":output,"usage":{"input_tokens":10,"output_tokens":10,"total_tokens":20,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}}})
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, Packet2FixtureError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(io_error)?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stream.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(header_end) = find_bytes(&bytes, b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(str::trim)
                        .and_then(|v| v.parse::<usize>().ok())
                })
                .unwrap_or(0);
            let body_start = header_end + 4;
            if bytes.len() >= body_start + length {
                let body = String::from_utf8(bytes[body_start..body_start + length].to_vec())
                    .map_err(|error| Packet2FixtureError::Protocol(error.to_string()))?;
                let path = headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_ascii_whitespace().nth(1))
                    .unwrap_or_default()
                    .to_owned();
                let auxiliary = is_auxiliary_request(&headers, &body);
                return Ok(HttpRequest {
                    path,
                    body,
                    auxiliary,
                });
            }
        }
    }
    Err(Packet2FixtureError::Protocol(
        "incomplete HTTP request".into(),
    ))
}

fn has_nonempty_header(headers: &str, name: &str) -> bool {
    headers.lines().any(|line| {
        line.split_once(':')
            .is_some_and(|(key, value)| key.eq_ignore_ascii_case(name) && !value.trim().is_empty())
    })
}

fn is_auxiliary_request(headers: &str, body: &str) -> bool {
    if has_nonempty_header(headers, "x-grok-turn-idx") {
        return false;
    }
    if has_nonempty_header(headers, "x-grok-req-id") {
        return true;
    }
    let foreground_by_tools = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .is_some_and(|value| {
            value
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|tools| tools.len() >= 2)
        });
    !foreground_by_tools
}

fn is_generation_path(path: &str) -> bool {
    path.ends_with("/chat/completions") || path.ends_with("/responses")
}

fn write_headers(stream: &mut TcpStream) -> Result<(), Packet2FixtureError> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        )
        .map_err(io_error)
}

fn write_json_response(stream: &mut TcpStream) -> Result<(), Packet2FixtureError> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
        )
        .map_err(io_error)
}

fn write_sse(stream: &mut TcpStream, data: &str) -> Result<(), Packet2FixtureError> {
    stream
        .write_all(format!("data: {data}\n\n").as_bytes())
        .and_then(|()| stream.flush())
        .map_err(io_error)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
fn update_trace(
    trace: &Arc<Mutex<Packet2FixtureTrace>>,
    update: impl FnOnce(&mut Packet2FixtureTrace),
) {
    match trace.lock() {
        Ok(mut value) => update(&mut value),
        Err(poisoned) => update(&mut poisoned.into_inner()),
    }
}
fn io_error(error: std::io::Error) -> Packet2FixtureError {
    Packet2FixtureError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        fixture_command, is_packet3_stream_request, packet3_stream_mode, FixtureStream,
        ISOLATED_COMMAND,
    };

    #[test]
    fn packet3_stream_is_text_only_and_tool_modes_share_command_payload() {
        // Given: Packet 3 stream and tool-family scenario identities.
        let stream = packet3_stream_mode("packet3-baseline-stream--wide-120x40");
        let tool = packet3_stream_mode("packet3-baseline-tool--wide-120x40");

        // When: fixture modes and adapter tool dialects are resolved.
        let grok_command = fixture_command("run_terminal_command", tool);
        let harness_command = fixture_command("bash", tool);

        // Then: stream bypasses tools and tool-family payloads are byte-identical.
        assert!(matches!(stream, FixtureStream::Packet3Stream));
        assert!(matches!(tool, FixtureStream::Packet3Settled));
        assert_eq!(grok_command, ISOLATED_COMMAND);
        assert_eq!(harness_command, ISOLATED_COMMAND);
        assert!(is_packet3_stream_request(
            r#"{"input":[{"content":"stream probe"}]}"#
        ));
        assert!(!is_packet3_stream_request(
            r#"{"input":[{"content":"generate a title"}]}"#
        ));
    }
}
