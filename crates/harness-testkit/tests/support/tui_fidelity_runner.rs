use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_testkit::tui_fidelity_runner::{
    CandidateBinding, RendererConfig, RunnerConfig, RunnerTiming, RuntimeBinary, SourceGuardConfig,
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
        let target = temp.path().join("target/test/debug");
        fs::create_dir_all(&target).expect("fixture target");
        let reference = write_program(&target, "reference", reference_mode, "reference");
        let harness = write_program(&target, "harness", harness_mode, "harness");
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
            candidate_binding: CandidateBinding {
                candidate_sha: "harness-revision".to_owned(),
                candidate_binary_sha256: RuntimeBinary::from_path(&harness, "harness-revision")
                    .expect("candidate identity")
                    .sha256,
                runner_sha256: "f".repeat(64),
                target_dir: harness
                    .parent()
                    .and_then(Path::parent)
                    .expect("candidate target")
                    .to_path_buf(),
                freshness_relation:
                    "test fixture current Git HEAD + worktree-local isolated target".to_owned(),
            },
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
        "missing-telemetry" => "trap 'exit 0' INT\nstty raw -echo\nprintf '\\033[2Jfixture-ready\\r\\n\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\ndone\n",
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
        &format!(
            "#!/usr/bin/env bash\n# {identity}\nset -eu\n{}{body}",
            presentation_trace_hook(mode)
        ),
    )
}

fn presentation_trace_hook(mode: &str) -> String {
    let frame = match mode {
        "normal" => r#"$'\033[2Jfixture-ready\r\n\033[1;1H❯'"#,
        "delayed-prompt" => r#"$'\033[2J\033[1;1H❯'"#,
        _ => return String::new(),
    };
    format!(
        r#"trace_frame={frame}
emit_presentation_trace() {{
  [[ -z ${{TUI_FIDELITY_PRESENTATION_TRACE:-}} ]] && return
  mkdir -p "$(dirname "$TUI_FIDELITY_PRESENTATION_TRACE")"
  digest=$(printf %s "$trace_frame" | sha256sum | awk '{{print $1}}')
  count=$(printf %s "$trace_frame" | wc -c)
  causes='{{"cause_id":"fixture:cause:1","interaction_id":null,"received_at":1,"kind":"startup","resulting_revision":1,"outcome":{{"kind":"visible_change","cause_id":"fixture:cause:1","revision":1}}}}'
  if [[ -n ${{TUI_FIDELITY_INTERACTION_QUEUE:-}} && -s "$TUI_FIDELITY_INTERACTION_QUEUE" ]]; then
    cause_sequence=2
    while IFS= read -r queued_interaction; do
      interaction_id=$(printf %s "$queued_interaction" | sed -n 's/.*"interaction_id":"\([^"]*\)".*/\1/p')
      [[ -z $interaction_id ]] && continue
      causes="$causes,{{\"cause_id\":\"fixture:cause:$cause_sequence\",\"interaction_id\":\"$interaction_id\",\"received_at\":$cause_sequence,\"kind\":\"terminal_input\",\"resulting_revision\":null,\"outcome\":{{\"kind\":\"no_visible_change\",\"cause_id\":\"fixture:cause:$cause_sequence\",\"closed_at\":$cause_sequence}}}}"
      cause_sequence=$((cause_sequence + 1))
    done <"$TUI_FIDELITY_INTERACTION_QUEUE"
  fi
  sed -e "s/@DIGEST@/$digest/g" -e "s/@COUNT@/$count/g" -e "s|@CAUSES@|$causes|g" >"$TUI_FIDELITY_PRESENTATION_TRACE" <<'JSON'
{{"trace_id":"fixture","causes":[@CAUSES@],"demands":[{{"target_revision":1,"earliest_requested_at":2,"latest_requested_at":2,"cause_ids":["fixture:cause:1"],"reason":"startup","coalesced_request_count":0}}],"frames":[{{"sequence":1,"revision":1,"cause_ids":["fixture:cause:1"],"requested_at":2,"render_started_at":3,"render_ended_at":4,"submitted_at":5,"write_started_at":6,"write_ended_at":7,"acknowledged_at":8,"frame_kind":"full_repaint","byte_count":@COUNT@,"byte_sha256":"@DIGEST@","acknowledgement":{{"kind":"success"}}}}],"acknowledgements":[{{"sequence":1,"revision":1,"cause_ids":["fixture:cause:1"],"requested_at":2,"render_started_at":3,"render_ended_at":4,"submitted_at":5,"write_started_at":6,"write_ended_at":7,"acknowledged_at":8,"frame_kind":"full_repaint","byte_count":@COUNT@,"byte_sha256":"@DIGEST@","outcome":{{"kind":"success"}}}}],"outcomes":[],"aggregates":{{"coalesced_requests":0,"queue_saturation":0,"resyncs":0,"full_repaints":1,"bytes_written":@COUNT@,"idle_redraws":0}}}}
JSON
}}
trap emit_presentation_trace EXIT
"#
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
