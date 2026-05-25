use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub stdin: Vec<u8>,
}

impl CommandInvocation {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdin: Vec::new(),
        }
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = stdin.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: Vec::new(),
        }
    }

    pub fn failure(exit_code: i32, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            exit_code,
            stdout: Vec::new(),
            stderr: stderr.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub output: CommandOutput,
}

impl ScriptedCommand {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
        output: CommandOutput,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            output,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCommandError {
    NoScriptedCommand {
        invocation: Box<CommandInvocation>,
    },
    Mismatch {
        expected_program: String,
        expected_args: Box<[String]>,
        actual: Box<CommandInvocation>,
    },
}

impl fmt::Display for FakeCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScriptedCommand { invocation } => {
                write!(
                    f,
                    "no scripted command for {} {:?}",
                    invocation.program, invocation.args
                )
            }
            Self::Mismatch {
                expected_program,
                expected_args,
                actual,
            } => write!(
                f,
                "scripted command mismatch: expected {} {:?}, got {} {:?}",
                expected_program, expected_args, actual.program, actual.args
            ),
        }
    }
}

impl std::error::Error for FakeCommandError {}

pub trait CommandRunner {
    fn run(&self, invocation: CommandInvocation) -> Result<CommandOutput, FakeCommandError>;
}

#[derive(Debug)]
pub struct FakeIdSource {
    seed: u64,
    next: AtomicU64,
}

impl FakeIdSource {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            next: AtomicU64::new(1),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn next_prefixed(&self, prefix: impl AsRef<str>) -> String {
        let index = self.next.fetch_add(1, Ordering::SeqCst);
        format!("{}_{:016x}_{index:06}", prefix.as_ref(), self.seed)
    }

    pub fn next_counter(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpInvocation {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub bearer_token: Option<String>,
    pub body: serde_json::Value,
}

impl HttpInvocation {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::new(),
            bearer_token: None,
            body: serde_json::Value::Null,
        }
    }

    pub fn headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub fn bearer_token(mut self, bearer_token: impl Into<String>) -> Self {
        self.bearer_token = Some(bearer_token.into());
        self
    }

    pub fn body(mut self, body: impl Into<serde_json::Value>) -> Self {
        self.body = body.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpOutput {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpOutput {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into().into_bytes(),
        }
    }

    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self::text(status, body.to_string())
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedHttpCall {
    pub method: String,
    pub url: String,
    pub output: HttpOutput,
}

impl ScriptedHttpCall {
    pub fn new(method: impl Into<String>, url: impl Into<String>, output: HttpOutput) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            output,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeHttpError {
    NoScriptedCall {
        invocation: Box<HttpInvocation>,
    },
    Mismatch {
        expected_method: String,
        expected_url: String,
        actual: Box<HttpInvocation>,
    },
}

impl fmt::Display for FakeHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScriptedCall { invocation } => {
                write!(
                    f,
                    "no scripted HTTP call for {} {}",
                    invocation.method, invocation.url
                )
            }
            Self::Mismatch {
                expected_method,
                expected_url,
                actual,
            } => write!(
                f,
                "scripted HTTP call mismatch: expected {} {}, got {} {}",
                expected_method, expected_url, actual.method, actual.url
            ),
        }
    }
}

impl std::error::Error for FakeHttpError {}

pub trait HttpClient {
    fn send(&self, invocation: HttpInvocation) -> Result<HttpOutput, FakeHttpError>;
}

#[derive(Debug, Default)]
pub struct FakeHttpClient {
    script: Mutex<VecDeque<ScriptedHttpCall>>,
    calls: Mutex<Vec<HttpInvocation>>,
}

impl FakeHttpClient {
    pub fn new(script: impl IntoIterator<Item = ScriptedHttpCall>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<HttpInvocation> {
        self.calls.lock().expect("HTTP calls lock poisoned").clone()
    }

    pub fn remaining_scripted_calls(&self) -> usize {
        self.script.lock().expect("HTTP script lock poisoned").len()
    }
}

impl HttpClient for FakeHttpClient {
    fn send(&self, invocation: HttpInvocation) -> Result<HttpOutput, FakeHttpError> {
        self.calls
            .lock()
            .expect("HTTP calls lock poisoned")
            .push(invocation.clone());

        let scripted = self
            .script
            .lock()
            .expect("HTTP script lock poisoned")
            .pop_front()
            .ok_or_else(|| FakeHttpError::NoScriptedCall {
                invocation: Box::new(invocation.clone()),
            })?;

        if scripted.method != invocation.method || scripted.url != invocation.url {
            return Err(FakeHttpError::Mismatch {
                expected_method: scripted.method,
                expected_url: scripted.url,
                actual: Box::new(invocation),
            });
        }

        Ok(scripted.output)
    }
}

#[derive(Debug, Default)]
pub struct FakeCommandRunner {
    script: Mutex<VecDeque<ScriptedCommand>>,
    calls: Mutex<Vec<CommandInvocation>>,
}

impl FakeCommandRunner {
    pub fn new(script: impl IntoIterator<Item = ScriptedCommand>) -> Self {
        Self {
            script: Mutex::new(script.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<CommandInvocation> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }

    pub fn remaining_scripted_commands(&self) -> usize {
        self.script.lock().expect("script lock poisoned").len()
    }
}

impl CommandRunner for FakeCommandRunner {
    fn run(&self, invocation: CommandInvocation) -> Result<CommandOutput, FakeCommandError> {
        self.calls
            .lock()
            .expect("calls lock poisoned")
            .push(invocation.clone());

        let scripted = self
            .script
            .lock()
            .expect("script lock poisoned")
            .pop_front()
            .ok_or_else(|| FakeCommandError::NoScriptedCommand {
                invocation: Box::new(invocation.clone()),
            })?;

        if scripted.program != invocation.program || scripted.args != invocation.args {
            return Err(FakeCommandError::Mismatch {
                expected_program: scripted.program,
                expected_args: scripted.args.into_boxed_slice(),
                actual: Box::new(invocation),
            });
        }

        Ok(scripted.output)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CommandInvocation, CommandOutput, CommandRunner, FakeCommandError, FakeCommandRunner,
        FakeHttpClient, FakeHttpError, FakeIdSource, HttpClient, HttpInvocation, HttpOutput,
        ScriptedCommand, ScriptedHttpCall,
    };
    use serde_json::json;

    #[test]
    fn fake_runner_replays_scripted_commands_in_order_and_records_calls() {
        let runner = FakeCommandRunner::new([
            ScriptedCommand::new(
                "git",
                ["status", "--short"],
                CommandOutput::success("clean"),
            ),
            ScriptedCommand::new("cargo", ["test"], CommandOutput::failure(101, "failed")),
        ]);

        let first = runner
            .run(CommandInvocation::new("git").args(["status", "--short"]))
            .expect("first command");
        assert_eq!(first.stdout, b"clean".to_vec());

        let second = runner
            .run(CommandInvocation::new("cargo").args(["test"]))
            .expect("second command");
        assert_eq!(second.exit_code, 101);
        assert_eq!(second.stderr, b"failed".to_vec());

        assert_eq!(runner.remaining_scripted_commands(), 0);
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn fake_runner_reports_mismatches_without_spawning() {
        let runner = FakeCommandRunner::new([ScriptedCommand::new(
            "cargo",
            ["check"],
            CommandOutput::success(""),
        )]);

        let err = runner
            .run(CommandInvocation::new("cargo").args(["test"]))
            .expect_err("mismatch");

        assert!(matches!(err, FakeCommandError::Mismatch { .. }));
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn fake_http_client_replays_scripted_calls_in_order_and_records_calls() {
        let client = FakeHttpClient::new([
            ScriptedHttpCall::new(
                "GET",
                "https://example.test/one",
                HttpOutput::text(200, "ok"),
            ),
            ScriptedHttpCall::new(
                "POST",
                "https://example.test/two",
                HttpOutput::json(201, json!({"created": true})),
            ),
        ]);

        let first = client
            .send(HttpInvocation::new("GET", "https://example.test/one"))
            .expect("first response");
        assert_eq!(first.status, 200);
        assert_eq!(first.body_text(), "ok");

        let second = client
            .send(
                HttpInvocation::new("POST", "https://example.test/two")
                    .bearer_token("token")
                    .body(json!({"name":"demo"})),
            )
            .expect("second response");
        assert_eq!(second.status, 201);

        assert_eq!(client.remaining_scripted_calls(), 0);
        let calls = client.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].bearer_token.as_deref(), Some("token"));
        assert_eq!(calls[1].body, json!({"name":"demo"}));
    }

    #[test]
    fn fake_http_client_reports_mismatches_without_network() {
        let client = FakeHttpClient::new([ScriptedHttpCall::new(
            "GET",
            "https://example.test/expected",
            HttpOutput::text(200, ""),
        )]);

        let err = client
            .send(HttpInvocation::new("GET", "https://example.test/actual"))
            .expect_err("mismatch");

        assert!(matches!(err, FakeHttpError::Mismatch { .. }));
        assert_eq!(client.calls().len(), 1);
    }

    #[test]
    fn fake_id_source_generates_seeded_stable_ids() {
        let ids = FakeIdSource::new(42);

        assert_eq!(ids.seed(), 42);
        assert_eq!(ids.next_prefixed("run"), "run_000000000000002a_000001");
        assert_eq!(ids.next_prefixed("task"), "task_000000000000002a_000002");

        let other = FakeIdSource::new(43);
        assert_eq!(other.next_prefixed("run"), "run_000000000000002b_000001");
    }

    #[test]
    fn fake_id_source_counter_is_manual_and_monotonic() {
        let ids = FakeIdSource::new(7);

        assert_eq!(ids.next_counter(), 1);
        assert_eq!(ids.next_counter(), 2);
        assert_eq!(
            ids.next_prefixed("artifact"),
            "artifact_0000000000000007_000003"
        );
    }
}
