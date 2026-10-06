mod transport;
pub(crate) use transport::{Control, Kernel};

use crate::{Result, SessionOptions};
use serde_json::{json, Value};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

static NEXT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(super) struct Process {
    pub child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    frame: Vec<u8>,
    diagnostics: tokio::task::JoinHandle<String>,
    pub runtime: Value,
    language: String,
    capture: tempfile::TempDir,
    #[cfg(unix)]
    group: Option<rustix::process::Pid>,
}

impl Process {
    pub async fn start(options: &SessionOptions, language: &str) -> Result<Self> {
        let capture = tempfile::Builder::new()
            .prefix("kernel-output-")
            .tempdir_in(&options.artifacts)?;
        let mut command = if language == "js" {
            let executable = interpreter(options, &["node"]).map_err(|_| "JavaScript eval requires Node.js 24 or newer on PATH; install Node.js and restart Harness")?;
            crate::javascript::command(&executable, capture.path())?
        } else if language == "py" {
            let mut command = Command::new(interpreter(options, &["python3", "python"])?);
            command.args([
                "-u",
                "-c",
                &format!(
                    "import sys\nsys.stdout.write('{{\"type\":\"status\",\"event\":{{\"op\":\"kernel-startup\",\"stage\":\"stdlib-imports\"}}}}\\n')\nsys.stdout.flush()\n{}\n{}\n{}\n{}",
                    include_str!("python_memory.py"),
                    include_str!("python_io.py"),
                    include_str!("python_tools.py"),
                    include_str!("python.py")
                ),
            ]);
            command
        } else if language == "rb" {
            let mut command = Command::new(interpreter(options, &["ruby"])?);
            command.args(["-e", include_str!("ruby.rb")]);
            command
        } else if language == "jl" {
            let mut command = Command::new(interpreter(options, &["julia"])?);
            command.args([
                "--startup-file=no",
                "--history-file=no",
                "-e",
                include_str!("julia.jl"),
            ]);
            command
        } else {
            return Err(format!("native {language} kernel is not installed").into());
        };
        command
            .current_dir(&options.cwd)
            .env_clear()
            .envs(options.environment.iter().filter(|(name, _)| {
                !name.to_string_lossy().starts_with("PI_")
                    && (language != "js" || *name != "NODE_OPTIONS")
            }))
            .envs(&options.session_env)
            .env("HARNESS_EVAL_CAPTURE_DIR", capture.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.as_std_mut().process_group(0);
        }
        let executable = std::path::PathBuf::from(command.as_std().get_program());
        let mut child = command.spawn()?;
        #[cfg(unix)]
        let group = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw);
        let input = child.stdin.take().ok_or("eval stdin unavailable")?;
        let output = BufReader::new(child.stdout.take().ok_or("eval stdout unavailable")?);
        let mut stderr = child.stderr.take().ok_or("eval stderr unavailable")?;
        let diagnostics = tokio::spawn(async move {
            let mut tail = Vec::new();
            let mut bytes = [0; 4096];
            while let Ok(count) = stderr.read(&mut bytes).await {
                if count == 0 {
                    break;
                }
                tail.extend_from_slice(&bytes[..count]);
                tail.drain(..tail.len().saturating_sub(8192));
            }
            String::from_utf8_lossy(&tail).into_owned()
        });
        let mut kernel = Self {
            child,
            input,
            output,
            frame: Vec::new(),
            diagnostics,
            runtime: Value::Null,
            language: language.to_owned(),
            capture,
            #[cfg(unix)]
            group,
        };
        kernel.send(json!({"type":"init","localRoot":options.local_dir,"captureRoot":kernel.capture.path(),"parallelPoolWidth":options.settings.parallel_pool_width,
            "memory":options.settings.memory,"generation":NEXT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)})).await?;
        let ready = kernel.ready().await?;
        kernel.runtime = ready["runtime"].clone();
        kernel.runtime["path"] = json!(executable);
        Ok(kernel)
    }

    async fn ready(&mut self) -> Result<Value> {
        let guard = Duration::from_secs(if self.language == "py" { 11 } else { 15 });
        let stages = [
            "process-start",
            "stdlib-imports",
            "runtime-init",
            "host-init",
        ];
        let mut stage = 0;
        let mut deadline = tokio::time::Instant::now() + guard;
        loop {
            let event = tokio::time::timeout_at(deadline, self.receive())
                .await
                .map_err(|_| {
                    format!(
                        "{} kernel startup stalled at {}",
                        self.language, stages[stage]
                    )
                })??;
            if event["type"] == "ready" {
                return Ok(event);
            }
            if event["type"] != "status" || event["event"]["op"] != "kernel-startup" {
                return Err(format!(
                    "{} kernel did not initialize at {}",
                    self.language, stages[stage]
                )
                .into());
            }
            if let Some(next) = stages
                .iter()
                .position(|name| event["event"]["stage"] == *name)
                .filter(|next| *next > stage)
            {
                stage = next;
                deadline = tokio::time::Instant::now() + guard;
            }
        }
    }

    pub async fn send(&mut self, value: Value) -> Result<()> {
        let mut bytes = if self.language == "jl" {
            julia_literal(&value).into_bytes()
        } else {
            serde_json::to_vec(&value)?
        };
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("eval message exceeds 32 MiB".into());
        }
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(10), self.input.write_all(&bytes)).await??;
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<Value> {
        let remaining = (32 * 1024 * 1024 + 1_usize).saturating_sub(self.frame.len());
        let count = (&mut self.output)
            .take(remaining as u64)
            .read_until(b'\n', &mut self.frame)
            .await?;
        if count == 0 {
            let diagnostic =
                tokio::time::timeout(Duration::from_millis(100), &mut self.diagnostics)
                    .await
                    .ok()
                    .and_then(std::result::Result::ok)
                    .unwrap_or_default();
            return Err(format!(
                "eval worker exited before the cell completed{}",
                if diagnostic.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", diagnostic.trim())
                }
            )
            .into());
        }
        let bytes = std::mem::take(&mut self.frame);
        if bytes.len() > 32 * 1024 * 1024 || !bytes.ends_with(b"\n") {
            return Err("invalid eval worker frame".into());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub async fn interrupt(&mut self, id: &str) -> Result<()> {
        self.send(json!({"type":"cancel","id":id})).await?;
        #[cfg(unix)]
        if self.language == "jl"
            && let Some(pid) = self
                .child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .and_then(rustix::process::Pid::from_raw)
        {
            rustix::process::kill_process(pid, rustix::process::Signal::INT)?;
        }
        Ok(())
    }

    pub async fn stop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.group.take() {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        self.diagnostics.abort();
    }
}

pub(crate) fn interpreter(options: &SessionOptions, names: &[&str]) -> Result<std::path::PathBuf> {
    let path = options
        .environment
        .get(std::ffi::OsStr::new("PATH"))
        .ok_or("eval interpreter PATH is unset")?;
    for name in names {
        for directory in std::env::split_paths(path) {
            let candidate = options
                .cwd
                .join(directory)
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
            let Ok(metadata) = std::fs::metadata(&candidate) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o111 == 0 {
                    continue;
                }
            }
            return Ok(candidate);
        }
    }
    Err(format!("eval interpreter is unavailable: {}", names.join(" or ")).into())
}

// Julia's standard library has no JSON parser. Encode trusted protocol values
// as literals; escaping `$` prevents data from becoming string interpolation.
fn julia_literal(value: &Value) -> String {
    match value {
        Value::Null => "nothing".into(),
        Value::String(_) => value.to_string().replace('$', "\\$"),
        Value::Array(values) => format!(
            "Any[{}]",
            values
                .iter()
                .map(julia_literal)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => format!(
            "Dict{{String,Any}}({})",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}=>{}",
                    julia_literal(&Value::String(key.clone())),
                    julia_literal(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.group.take() {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
        self.diagnostics.abort();
    }
}
