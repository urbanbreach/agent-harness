use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_testkit::tui_fidelity_runner::{
    RendererConfig, RunnerConfig, RunnerTiming, RuntimeBinary, SourceGuardConfig,
};

pub const STARTUP_SMOKE: &str = include_str!("../fixtures/tui_fidelity/startup-smoke.json");

pub fn dirty_source_guard(root: &Path) -> PathBuf {
    write_executable(
        root,
        "dirty-source-guard",
        "#!/usr/bin/env bash\nprintf 'dirty reference source\\n' >&2\nexit 1\n",
    )
}

pub struct Fixture {
    _temp: tempfile::TempDir,
    pub config: RunnerConfig,
}

impl Fixture {
    pub fn new(reference_mode: &str, harness_mode: &str, renderer_mode: &str) -> Self {
        let temp = tempfile::tempdir().expect("fixture tempdir");
        let reference = write_program(temp.path(), "reference", reference_mode, "reference");
        let harness = write_program(temp.path(), "harness", harness_mode, "harness");
        let source_guard = write_source_guard(temp.path());
        let renderer = write_renderer(temp.path(), renderer_mode);
        let browser = write_executable(temp.path(), "browser", "#!/usr/bin/env bash\nexit 0\n");
        let config = RunnerConfig {
            repo_root: temp.path().to_path_buf(),
            evidence_dir: temp.path().join("evidence"),
            reference: RuntimeBinary::from_path(&reference, "reference-revision")
                .expect("reference identity"),
            harness: RuntimeBinary::from_path(&harness, "harness-revision")
                .expect("harness identity"),
            source_guard: SourceGuardConfig {
                program: source_guard,
                reference_root: temp.path().join("reference-source"),
                revision: "reference-revision".to_owned(),
            },
            renderer: RendererConfig {
                node_program: PathBuf::from("node"),
                script: renderer,
                browser_program: browser,
                font_family: "DejaVu Sans Mono".to_owned(),
                node_modules: None,
            },
            timing: RunnerTiming {
                tick: Duration::from_millis(2),
                scenario_timeout: Duration::from_secs(2),
                normal_exit_timeout: Duration::from_millis(250),
                cleanup_timeout: Duration::from_millis(500),
            },
        };
        Self {
            _temp: temp,
            config,
        }
    }

    pub fn root(&self) -> &Path {
        self._temp.path()
    }
}

fn write_program(root: &Path, name: &str, mode: &str, identity: &str) -> PathBuf {
    let body = match mode {
        "normal" => "trap 'exit 0' INT\nstty raw -echo\nprintf '\\033[2Jfixture-ready\\r\\n\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\ndone\n",
        "premature" => "printf 'premature\\n'\nexit 0\n",
        "skipped" => "stty raw -echo\nprintf 'Skipped\\r\\n'\nwhile :; do sleep 1; done\n",
        "hang" => "stty raw -echo\ntrap '' INT TERM HUP\nprintf 'hanging\\r\\n\\033[1;1H❯'\nwhile :; do sleep 1; done\n",
        "delayed-prompt" => "trap 'exit 0' INT TERM HUP\nprintf 'booting\\r\\n'\nsleep 0.05\nstty raw -echo\nprintf '\\033[2J\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  if [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]]; then exit 0; fi\ndone\n",
        "survivor" => "trap 'exit 0' INT\nstty raw -echo\nsetsid bash -c 'trap \"\" HUP INT TERM; while :; do sleep 1; done' &\nprintf 'survivor-ready\\r\\n\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\ndone\n",
        "cleanup-failure" => "trap 'exit 0' INT\nstty raw -echo\nprintf 'cleanup-failure-ready\\r\\n\\033[1;1H❯'\nbuffer=''\nwhile IFS= read -r -n 1 c; do\n  buffer=${buffer}${c}\n  if [[ \"$buffer\" == *'/exit' ]]; then\n    target=${TUI_FIDELITY_RUN_ROOT:-$(dirname \"$PWD\")}\n    cd /\n    rm -rf \"$target\"\n    printf 'blocks recursive directory cleanup\\n' >\"$target\"\n    exit 0\n  fi\ndone\n",
        other => panic!("unsupported fixture mode: {other}"),
    };
    write_executable(
        root,
        name,
        &format!("#!/usr/bin/env bash\n# {identity}\nset -eu\n{body}"),
    )
}

fn write_source_guard(root: &Path) -> PathBuf {
    write_executable(
        root,
        "source-guard",
        "#!/usr/bin/env bash\nset -eu\nreceipt=''\nwhile (($#)); do\n  if [[ $1 == --receipt ]]; then receipt=$2; shift 2; else shift; fi\ndone\n[[ ${TUI_TEST_DIRTY_SOURCE:-0} == 1 ]] && { printf 'dirty reference source\\n' >&2; exit 1; }\nmkdir -p \"$(dirname \"$receipt\")\"\nprintf '{\"clean_pre\":true,\"clean_post\":true}\\n' >\"$receipt\"\n",
    )
}

fn write_renderer(root: &Path, mode: &str) -> PathBuf {
    if mode == "hang" {
        let path = root.join("renderer.mjs");
        fs::write(
            &path,
            "import fs from 'node:fs';\nimport path from 'node:path';\nconst a=process.argv.slice(2); const get=(n)=>a[a.indexOf(n)+1];\nconst out=get('--evidence-dir'); fs.mkdirSync(out,{recursive:true}); fs.writeFileSync(path.join(out,'renderer.pid'),String(process.pid));\nprocess.on('SIGTERM',()=>{}); setInterval(()=>{},1000);\n",
        )
        .expect("write hanging renderer");
        return path;
    }
    let script = format!(
        "import fs from 'node:fs';\nimport path from 'node:path';\nconst a=process.argv.slice(2); const get=(n)=>a[a.indexOf(n)+1];\nconst out=get('--evidence-dir'); fs.mkdirSync(out,{{recursive:true}});\nconst ansi=fs.readFileSync(get('--from-file')); fs.writeFileSync(path.join(out,'terminal-ansi.txt'),ansi); fs.writeFileSync(path.join(out,'terminal.txt'),'fixture\\n');\nif ('{mode}' !== 'missing-checkpoint') fs.writeFileSync(path.join(out,'terminal.png'),Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAGQAAAAeCAYAAADaW7vzAAAAhUlEQVR4nO3YoREAIBAEMWr4/nuFIhAfseI8swHDmZnbnAZn+wAtEPoS9EIAhECA8IEAsQMBAgcCRA0ECBkIEC8QIFggQKRAgDCBADGE9XUCIAQChA8EiB0IEDgQIGogQMhAgHiBAMECASIFAoQJBIghrK8TACEQIHwgQOxAgMCBzH7Unz3xRiXr7uuycwAAAABJRU5ErkJggg==','base64'));\nfs.writeFileSync(path.join(out,'metadata.json'),JSON.stringify({{browserCapture:'captured',dimensions:{{cols:Number(get('--cols')),rows:Number(get('--rows')),fontFamily:get('--font-family')}},capabilities:{{unicodeVersion:'11',devicePixelRatio:2,browser:'fixture-browser',fontLoaded:true,color:'truecolor',graphics:'sixel-disabled'}}}}));\n"
    );
    let path = root.join("renderer.mjs");
    fs::write(&path, script).expect("write renderer");
    path
}

fn write_executable(root: &Path, name: &str, body: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, body).expect("write executable fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod fixture");
    path
}
