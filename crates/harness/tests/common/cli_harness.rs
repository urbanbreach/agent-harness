#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use harness_providers::Provider;
use harness_testkit::workspace::TestWorkspace;

const EVENTS_FILE_NAME: &str = "events.jsonl";
const ARTIFACTS_DIR_NAME: &str = "artifacts";

#[derive(Debug)]
pub(crate) struct CliHarnessOutput {
    pub(crate) status: CliHarnessStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) session_capture: Option<CliHarnessSessionCapture>,
    #[allow(dead_code)]
    pub(crate) workspace: Option<TestWorkspace>,
}

impl CliHarnessOutput {
    pub(crate) fn single_run(&self) -> &CliHarnessRunCapture {
        let capture = self
            .session_capture
            .as_ref()
            .expect("CliHarnessOutput was not configured to capture a session dir");
        assert_eq!(
            capture.runs.len(),
            1,
            "expected exactly one captured run under {}, got {}",
            capture.session_dir.display(),
            capture.runs.len()
        );
        &capture.runs[0]
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CliHarnessSessionCapture {
    pub(crate) session_dir: PathBuf,
    pub(crate) runs: Vec<CliHarnessRunCapture>,
}

#[derive(Debug, Clone)]
pub(crate) struct CliHarnessRunCapture {
    pub(crate) run_dir: PathBuf,
    pub(crate) events_path: PathBuf,
    pub(crate) events: String,
    pub(crate) artifacts: Vec<CliHarnessArtifactCapture>,
}

#[derive(Debug, Clone)]
pub(crate) struct CliHarnessArtifactCapture {
    pub(crate) path: PathBuf,
    pub(crate) relative_path: PathBuf,
    #[allow(dead_code)]
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CliHarnessStatus {
    code: i32,
}

impl CliHarnessStatus {
    pub(crate) fn success(self) -> bool {
        self.code == 0
    }

    #[allow(dead_code)]
    pub(crate) fn code(self) -> i32 {
        self.code
    }
}

#[derive(Default)]
pub(crate) struct CliHarness {
    args: Vec<OsString>,
    stdin: Vec<u8>,
    current_dir: Option<PathBuf>,
    env: BTreeMap<String, Option<String>>,
    provider_override: Option<Arc<dyn Provider>>,
    capture_session_dir: Option<PathBuf>,
    workspace: Option<TestWorkspace>,
}

impl CliHarness {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub(crate) fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = stdin.into();
        self
    }

    pub(crate) fn current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub(crate) fn test_workspace(mut self, workspace: TestWorkspace) -> Self {
        let root = workspace.root().to_path_buf();
        let session_dir = workspace.sessions_dir();
        self.current_dir = Some(root);
        self.capture_session_dir = Some(session_dir);
        self.workspace = Some(workspace);
        self
    }

    pub(crate) fn capture_session_dir(mut self, session_dir: impl Into<PathBuf>) -> Self {
        self.capture_session_dir = Some(session_dir.into());
        self
    }

    pub(crate) fn env<K, V>(mut self, name: K, value: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.env.insert(
            name.as_ref().to_string_lossy().into_owned(),
            Some(value.as_ref().to_string_lossy().into_owned()),
        );
        self
    }

    pub(crate) fn env_remove<K>(mut self, name: K) -> Self
    where
        K: AsRef<OsStr>,
    {
        self.env
            .insert(name.as_ref().to_string_lossy().into_owned(), None);
        self
    }

    pub(crate) fn provider_override(mut self, provider: Arc<dyn Provider>) -> Self {
        self.provider_override = Some(provider);
        self
    }

    pub(crate) fn output(self) -> CliHarnessOutput {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(OsString::from("harness"));
        argv.extend(self.args);

        let mut stdin = Cursor::new(self.stdin);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = harness::CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let mut deps = harness::CliDeps::real();
        let capture_session_dir = self.capture_session_dir.clone();
        if let Some(current_dir) = self.current_dir {
            deps = deps.with_current_dir(current_dir);
        }
        for (name, value) in self.env {
            deps = match value {
                Some(value) => deps.with_env(name, value),
                None => deps.without_env(name),
            };
        }
        if let Some(provider) = self.provider_override {
            deps = deps.with_provider_override(provider);
        }
        let outcome = harness::run(argv, &mut io, deps);
        let session_capture = capture_session_dir
            .as_deref()
            .map(capture_session_dir_contents)
            .transpose()
            .expect("capture session dir");

        CliHarnessOutput {
            status: CliHarnessStatus { code: outcome.code },
            stdout,
            stderr,
            session_capture,
            workspace: self.workspace,
        }
    }
}

fn capture_session_dir_contents(session_dir: &Path) -> io::Result<CliHarnessSessionCapture> {
    let mut runs = Vec::new();
    if session_dir.exists() {
        for entry in fs::read_dir(session_dir)? {
            let entry = entry?;
            let run_dir = entry.path();
            let events_path = run_dir.join(EVENTS_FILE_NAME);
            if run_dir.is_dir() && events_path.exists() {
                let events = fs::read_to_string(&events_path)?;
                let artifacts = capture_artifacts(&run_dir)?;
                runs.push(CliHarnessRunCapture {
                    run_dir,
                    events_path,
                    events,
                    artifacts,
                });
            }
        }
    }
    runs.sort_by(|left, right| left.run_dir.cmp(&right.run_dir));
    Ok(CliHarnessSessionCapture {
        session_dir: session_dir.to_path_buf(),
        runs,
    })
}

fn capture_artifacts(run_dir: &Path) -> io::Result<Vec<CliHarnessArtifactCapture>> {
    let artifacts_dir = run_dir.join(ARTIFACTS_DIR_NAME);
    let mut artifacts = Vec::new();
    if artifacts_dir.exists() {
        collect_artifacts(run_dir, &artifacts_dir, &mut artifacts)?;
    }
    artifacts.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(artifacts)
}

fn collect_artifacts(
    run_dir: &Path,
    dir: &Path,
    artifacts: &mut Vec<CliHarnessArtifactCapture>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_artifacts(run_dir, &path, artifacts)?;
        } else if path.is_file() {
            let relative_path = path
                .strip_prefix(run_dir)
                .expect("artifact path lives under run dir")
                .to_path_buf();
            let bytes = fs::read(&path)?;
            artifacts.push(CliHarnessArtifactCapture {
                path,
                relative_path,
                bytes,
            });
        }
    }
    Ok(())
}
