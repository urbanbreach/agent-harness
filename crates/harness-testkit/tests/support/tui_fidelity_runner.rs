use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_testkit::tui_fidelity_runner::{
    CandidateAuthorityBinding, CandidateBinaryBinding, CandidateBinding, CandidateFileBinding,
    CandidateReceiptKind, CandidateRepositoryBinding, RendererConfig, RunnerConfig, RunnerTiming,
    RuntimeBinary, SourceGuardConfig,
};
use sha2::{Digest as _, Sha256};

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
        let aggregate = write_executable(
            temp.path(),
            "target/test/debug/tui_fidelity_aggregate",
            "#!/usr/bin/env bash\nexit 0\n",
        );
        let renderer = write_renderer(temp.path(), renderer_mode);
        let browser = write_executable(temp.path(), "browser", "#!/usr/bin/env bash\nexit 0\n");
        initialize_repository(temp.path());
        let repository = repository_binding(temp.path());
        let guard_bytes = source_guard_receipt(&repository);
        let source_guard = write_source_guard(temp.path(), &guard_bytes);
        let harness_identity =
            RuntimeBinary::from_path(&harness, &repository.head).expect("harness identity");
        let authority_path = temp
            .path()
            .join("configs/tui-fidelity-reference-authority.json");
        let reference_receipt_path = temp
            .path()
            .join("configs/tui-fidelity-reference-binary-receipt.json");
        let config = RunnerConfig {
            repo_root: temp.path().to_path_buf(),
            evidence_dir: temp.path().join("evidence"),
            reference: RuntimeBinary::from_path(&reference, "reference-revision")
                .expect("reference identity"),
            harness: harness_identity.clone(),
            candidate_binding: CandidateBinding {
                schema_version: "harness.tui-fidelity.candidate-binding.v2".to_owned(),
                receipt_kind: CandidateReceiptKind::Release,
                repository,
                binaries: CandidateBinaryBinding {
                    harness_sha256: harness_identity.sha256,
                    runner_sha256: sha256_path(&std::env::current_exe().expect("test binary")),
                    aggregate_sha256: sha256_path(&aggregate),
                },
                target_dir: harness
                    .parent()
                    .and_then(Path::parent)
                    .expect("candidate target")
                    .canonicalize()
                    .expect("canonical candidate target"),
                authority: CandidateAuthorityBinding {
                    path: authority_path.clone(),
                    revision: "reference-revision".to_owned(),
                    sha256: sha256_path(&authority_path),
                },
                reference_receipt: CandidateFileBinding {
                    path: reference_receipt_path.clone(),
                    sha256: sha256_path(&reference_receipt_path),
                },
                source_guard_receipt_sha256: sha256_bytes(&guard_bytes),
                parity_acceptance_eligible: true,
                release_eligible: true,
                clean_release: true,
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

    pub fn relocate_candidate_bundle(&mut self, candidate: &Path) {
        fs::create_dir_all(candidate.parent().expect("candidate parent"))
            .expect("candidate directory");
        fs::copy(&self.config.harness.path, candidate).expect("candidate copy");
        fs::copy(
            self.config
                .candidate_binding
                .target_dir
                .join("debug/tui_fidelity_aggregate"),
            candidate
                .parent()
                .expect("candidate profile")
                .join("tui_fidelity_aggregate"),
        )
        .expect("aggregate copy");
        self.config.harness =
            RuntimeBinary::from_path(candidate, &self.config.candidate_binding.repository.head)
                .expect("relative evidence candidate identity");
        self.config.candidate_binding.binaries.harness_sha256 = self.config.harness.sha256.clone();
        self.config.candidate_binding.target_dir = candidate
            .parent()
            .and_then(Path::parent)
            .expect("candidate target")
            .canonicalize()
            .expect("canonical candidate target");
    }

    pub fn expose_runtime_workspace_to_source_guard(&mut self) {
        fs::write(
            self.root().join(".gitignore"),
            "target/\nevidence/\nbrowser\nrenderer.mjs\nsource-guard\nsource-guard-template.json\n",
        )
        .expect("unignore runtime workspace");
        git(self.root(), &["add", ".gitignore"]);
        git(self.root(), &["commit", "-qm", "observe runtime workspace"]);

        let repository = repository_binding(self.root());
        let guard_bytes = source_guard_receipt(&repository);
        fs::write(self.root().join("source-guard-template.json"), &guard_bytes)
            .expect("refresh source guard template");
        self.config.source_guard.program = write_workspace_sensitive_source_guard(self.root());
        self.config.harness = RuntimeBinary::from_path(&self.config.harness.path, &repository.head)
            .expect("refresh Harness identity");
        self.config.candidate_binding.repository = repository;
        self.config.candidate_binding.binaries.harness_sha256 = self.config.harness.sha256.clone();
        self.config.candidate_binding.source_guard_receipt_sha256 = sha256_bytes(&guard_bytes);
    }
}

fn write_program(root: &Path, name: &str, mode: &str, identity: &str) -> PathBuf {
    let body = match mode {
        "normal" => "trap 'exit 0' INT\nstty raw -echo\nprintf '\\033[2Jfixture-ready\\r\\n\\033[1;1H❯'\nfirst_input=1\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\n  if [[ $first_input == 1 ]]; then sleep 0.02; printf '%s' \"$c\"; first_input=0; fi\ndone\n",
        "missing-telemetry" => "trap 'exit 0' INT\nstty raw -echo\nprintf '\\033[2Jfixture-ready\\r\\n\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\ndone\n",
        "premature" => "printf 'premature\\n'\nexit 0\n",
        "skipped" => "stty raw -echo\nprintf 'Skipped\\r\\n'\nwhile :; do sleep 1; done\n",
        "hang" => "stty raw -echo\ntrap '' INT TERM HUP\nprintf 'hanging\\r\\n\\033[1;1H❯'\nwhile :; do sleep 1; done\n",
        "delayed-prompt" => "trap 'exit 0' INT TERM HUP\nprintf 'booting\\r\\n'\nsleep 0.05\nstty raw -echo\nprintf '\\033[2J\\033[1;1H❯'\nfirst_input=1\nwhile IFS= read -r -n 1 c; do\n  if [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]]; then exit 0; fi\n  if [[ $first_input == 1 ]]; then printf '\\033[2;1Hinput-ready\\033[1;1H'; first_input=0; fi\ndone\n",
        "survivor" => "trap 'exit 0' INT\nstty raw -echo\nsetsid bash -c 'trap \"\" HUP INT TERM; while :; do sleep 1; done' &\nprintf 'survivor-ready\\r\\n\\033[1;1H❯'\nwhile IFS= read -r -n 1 c; do\n  [[ \"$c\" == $'\\003' || \"$c\" == $'\\021' ]] && exit 0\ndone\n",
        "cleanup-failure" => "trap 'exit 0' INT\nstty raw -echo\nprintf 'cleanup-failure-ready\\r\\n\\033[1;1H❯'\nbuffer=''\nwhile IFS= read -r -n 1 c; do\n  buffer=${buffer}${c}\n  if [[ \"$buffer\" == *'/exit' ]]; then\n    target=${TUI_FIDELITY_RUN_ROOT:-$(dirname \"$PWD\")}\n    cd /\n    rm -rf \"$target\"\n    printf 'blocks recursive directory cleanup\\n' >\"$target\"\n    exit 0\n  fi\ndone\n",
        other => panic!("unsupported fixture mode: {other}"),
    };
    let fixture_reset = if identity == "reference" {
        "\\033[H\\033[J"
    } else {
        "\\033[2J\\033[H"
    };
    write_executable(
        root,
        name,
        &format!(
            "#!/usr/bin/env bash\n# {identity}\nset -eu\nfixture_reset=$'{fixture_reset}'\n{}{body}",
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
  frame_causes='"fixture:cause:1"'
  if [[ -n ${{TUI_FIDELITY_INTERACTION_QUEUE:-}} && -s "$TUI_FIDELITY_INTERACTION_QUEUE" ]]; then
    cause_sequence=2
    while IFS= read -r queued_interaction; do
      interaction_id=$(printf %s "$queued_interaction" | sed -n 's/.*"interaction_id":"\([^"]*\)".*/\1/p')
      receipt_count=$(printf %s "$queued_interaction" | sed -n 's/.*"receipt_count":\([0-9][0-9]*\).*/\1/p')
      event_class=$(printf %s "$queued_interaction" | sed -n 's/.*"event_class":"\([^"]*\)".*/\1/p')
      [[ -z $interaction_id ]] && continue
      [[ -z $receipt_count ]] && receipt_count=1
      cause_kind=terminal_input
      [[ $event_class == resize ]] && cause_kind=resize
      receipt_ordinal=0
      while ((receipt_ordinal < receipt_count)); do
        if [[ $cause_sequence == 2 ]]; then
          causes="$causes,{{\"cause_id\":\"fixture:cause:$cause_sequence\",\"interaction_id\":\"$interaction_id\",\"received_at\":$cause_sequence,\"kind\":\"$cause_kind\",\"resulting_revision\":1,\"outcome\":{{\"kind\":\"visible_change\",\"cause_id\":\"fixture:cause:$cause_sequence\",\"revision\":1}}}}"
          frame_causes="$frame_causes,\"fixture:cause:$cause_sequence\""
        else
          causes="$causes,{{\"cause_id\":\"fixture:cause:$cause_sequence\",\"interaction_id\":\"$interaction_id\",\"received_at\":$cause_sequence,\"kind\":\"$cause_kind\",\"resulting_revision\":null,\"outcome\":{{\"kind\":\"no_visible_change\",\"cause_id\":\"fixture:cause:$cause_sequence\",\"closed_at\":$cause_sequence}}}}"
        fi
        cause_sequence=$((cause_sequence + 1))
        receipt_ordinal=$((receipt_ordinal + 1))
      done
    done <"$TUI_FIDELITY_INTERACTION_QUEUE"
  fi
  sed -e "s/@DIGEST@/$digest/g" -e "s/@COUNT@/$count/g" -e "s|@CAUSES@|$causes|g" -e "s|@FRAME_CAUSES@|$frame_causes|g" >"$TUI_FIDELITY_PRESENTATION_TRACE" <<'JSON'
{{"trace_id":"fixture","causes":[@CAUSES@],"demands":[{{"target_revision":1,"earliest_requested_at":2,"latest_requested_at":2,"cause_ids":[@FRAME_CAUSES@],"reason":"startup","coalesced_request_count":0}}],"frames":[{{"sequence":1,"revision":1,"cause_ids":[@FRAME_CAUSES@],"requested_at":2,"render_started_at":3,"render_ended_at":4,"submitted_at":5,"write_started_at":6,"write_ended_at":7,"acknowledged_at":8,"frame_kind":"full_repaint","byte_count":@COUNT@,"byte_sha256":"@DIGEST@","acknowledgement":{{"kind":"success"}}}}],"acknowledgements":[{{"sequence":1,"revision":1,"cause_ids":[@FRAME_CAUSES@],"requested_at":2,"render_started_at":3,"render_ended_at":4,"submitted_at":5,"write_started_at":6,"write_ended_at":7,"acknowledged_at":8,"frame_kind":"full_repaint","byte_count":@COUNT@,"byte_sha256":"@DIGEST@","outcome":{{"kind":"success"}}}}],"outcomes":[],"aggregates":{{"coalesced_requests":0,"queue_saturation":0,"resyncs":0,"full_repaints":1,"bytes_written":@COUNT@,"idle_redraws":0}}}}
JSON
}}
trap emit_presentation_trace EXIT
"#
    )
}

fn write_source_guard(root: &Path, receipt: &[u8]) -> PathBuf {
    fs::write(root.join("source-guard-template.json"), receipt).expect("source guard template");
    write_executable(
        root,
        "source-guard",
        "#!/usr/bin/env bash\nset -eu\nreceipt=''\nwhile (($#)); do\n  if [[ $1 == --receipt ]]; then receipt=$2; shift 2; else shift; fi\ndone\n[[ ${TUI_TEST_DIRTY_SOURCE:-0} == 1 ]] && { printf 'dirty reference source\\n' >&2; exit 1; }\nmkdir -p \"$(dirname \"$receipt\")\"\ncp \"$(dirname \"$0\")/source-guard-template.json\" \"$receipt\"\n",
    )
}

fn write_workspace_sensitive_source_guard(root: &Path) -> PathBuf {
    write_executable(
        root,
        "source-guard",
        "#!/usr/bin/env bash\nset -eu\nreceipt=''\nwhile (($#)); do\n  if [[ $1 == --receipt ]]; then receipt=$2; shift 2; else shift; fi\ndone\nmkdir -p \"$(dirname \"$receipt\")\"\ncp \"$(dirname \"$0\")/source-guard-template.json\" \"$receipt\"\nif find \"$(dirname \"$0\")/tmp/tui-fidelity\" -type f -print -quit 2>/dev/null | grep -q .; then printf ' ' >>\"$receipt\"; fi\n",
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
    let png = "iVBORw0KGgoAAAANSUhEUgAAAGQAAAAeCAYAAADaW7vzAAAAhUlEQVR4nO3YoREAIBAEMWr4/nuFIhAfseI8swHDmZnbnAZn+wAtEPoS9EIAhECA8IEAsQMBAgcCRA0ECBkIEC8QIFggQKRAgDCBADGE9XUCIAQChA8EiB0IEDgQIGogQMhAgHiBAMECASIFAoQJBIghrK8TACEQIHwgQOxAgMCBzH7Unz3xRiXr7uuycwAAAABJRU5ErkJggg==";
    let script = format!(
        "import fs from 'node:fs';\nimport path from 'node:path';\nconst a=process.argv.slice(2); const get=(n)=>a[a.indexOf(n)+1];\nconst out=get('--evidence-dir'); fs.mkdirSync(out,{{recursive:true}});\nconst ansi=fs.readFileSync(get('--from-file')); fs.writeFileSync(path.join(out,'terminal-ansi.txt'),ansi); fs.writeFileSync(path.join(out,'terminal.txt'),'fixture\\n');\nif ('{mode}' !== 'missing-checkpoint') fs.writeFileSync(path.join(out,'terminal.png'),Buffer.from('{png}','base64'));\nfs.writeFileSync(path.join(out,'metadata.json'),JSON.stringify({{browserCapture:'captured',dimensions:{{cols:Number(get('--cols')),rows:Number(get('--rows')),fontFamily:get('--font-family')}},capabilities:{{unicodeVersion:'11',devicePixelRatio:2,browser:'fixture-browser',fontLoaded:true,color:'truecolor',graphics:'sixel-disabled'}},rendererBinding:{{node:process.version,xterm:'6.0.0',unicode11:'0.9.0',nodePty:'1.1.0',pngjs:'7.0.0',puppeteerCore:'24.43.1'}}}}));\n"
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

fn initialize_repository(root: &Path) {
    fs::write(
        root.join(".gitignore"),
        "target/\nevidence/\ntmp/\nbrowser\nrenderer.mjs\nsource-guard\nsource-guard-template.json\n",
    )
    .expect("fixture gitignore");
    fs::write(root.join("Cargo.lock"), "# fixture lock\n").expect("fixture lock");
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"stable\"\n",
    )
    .expect("fixture toolchain");
    fs::create_dir_all(root.join("configs")).expect("fixture configs");
    fs::write(
        root.join("configs/tui-fidelity-reference-authority.json"),
        "{\"schema_version\":\"harness.tui-fidelity.reference-authority.v1\",\"status\":\"active\",\"reference\":{\"source_revision\":\"reference-revision\",\"receipt_path\":\"configs/tui-fidelity-reference-binary-receipt.json\"}}\n",
    )
    .expect("fixture authority");
    fs::write(
        root.join("configs/tui-fidelity-reference-binary-receipt.json"),
        "{\"fixture\":\"reference-receipt\"}\n",
    )
    .expect("fixture reference receipt");
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}

pub(super) fn repository_binding(root: &Path) -> CandidateRepositoryBinding {
    CandidateRepositoryBinding {
        canonical_path: root.canonicalize().expect("canonical fixture root"),
        head: git_text(root, &["rev-parse", "HEAD"]),
        tree: git_text(root, &["rev-parse", "HEAD^{tree}"]),
        clean: git_output(
            root,
            &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
        )
        .is_empty(),
        tracked_source_sha256: manifest_sha256(root, false),
        dirty_diff_sha256: sha256_bytes(&git_output(root, &["diff", "--binary", "HEAD", "--"])),
        untracked_manifest_sha256: manifest_sha256(root, true),
        cargo_lock_sha256: sha256_path(&root.join("Cargo.lock")),
        toolchain_sha256: sha256_path(&root.join("rust-toolchain.toml")),
        cargo_config_sha256: None,
    }
}

fn source_guard_receipt(repository: &CandidateRepositoryBinding) -> Vec<u8> {
    let value = serde_json::json!({
        "schema": "harness.tui-fidelity.source-guard.v2",
        "reference": guard_source(
            &repository.canonical_path,
            "reference-revision",
            &"a".repeat(40),
            &"b".repeat(64),
            &"c".repeat(64),
            &"d".repeat(64),
            &"e".repeat(64),
            &"f".repeat(64),
        ),
        "harness": guard_source(
            &repository.canonical_path,
            &repository.head,
            &repository.tree,
            &repository.tracked_source_sha256,
            &repository.dirty_diff_sha256,
            &repository.untracked_manifest_sha256,
            &repository.cargo_lock_sha256,
            &repository.toolchain_sha256,
        ),
        "tools": {}
    });
    let mut bytes = serde_json::to_vec(&value).expect("guard JSON");
    bytes.push(b'\n');
    bytes
}

#[expect(
    clippy::too_many_arguments,
    reason = "guard fixture mirrors the receipt boundary"
)]
fn guard_source(
    path: &Path,
    revision: &str,
    tree: &str,
    source: &str,
    dirty: &str,
    untracked: &str,
    lock: &str,
    toolchain: &str,
) -> serde_json::Value {
    serde_json::json!({
        "path": path,
        "revision": revision,
        "tree": tree,
        "status_sha256": sha256_bytes(b""),
        "source_sha256": source,
        "dirty_diff_sha256": dirty,
        "untracked_manifest_sha256": untracked,
        "cargo_lock_sha256": lock,
        "toolchain_sha256": toolchain,
        "cargo_config_sha256": null,
        "clean_pre": true,
        "clean_post": true
    })
}

fn manifest_sha256(root: &Path, untracked: bool) -> String {
    let args = if untracked {
        ["ls-files", "--others", "--exclude-standard", "-z"].as_slice()
    } else {
        ["ls-files", "-z"].as_slice()
    };
    let mut records = git_output(root, args)
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter(|path| untracked || approved_source_path(path))
        .map(|path| {
            let source_path = root.join(OsStr::from_bytes(path));
            let mut record = if source_path.exists() {
                sha256_path(&source_path).into_bytes()
            } else {
                b"deleted".to_vec()
            };
            record.extend_from_slice(b"  ");
            record.extend_from_slice(path);
            record.push(0);
            record
        })
        .collect::<Vec<_>>();
    records.sort_unstable();
    sha256_bytes(&records.concat())
}

fn approved_source_path(path: &[u8]) -> bool {
    path.ends_with(b".rs")
        || path.ends_with(b".sh")
        || path.ends_with(b".py")
        || path.ends_with(b".json")
        || path.ends_with(b".jsonc")
        || path.ends_with(b".toml")
        || path.ends_with(b".yaml")
        || path.ends_with(b".yml")
        || path == b"Cargo.lock"
        || path == b"rust-toolchain"
}

fn git(root: &Path, args: &[&str]) {
    let output = crate::harness_bin::command("git")
        .args(args)
        .current_dir(root)
        .env("GIT_MASTER", "1")
        .output()
        .expect("fixture git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = crate::harness_bin::command("git")
        .args(args)
        .current_dir(root)
        .env("GIT_MASTER", "1")
        .output()
        .expect("fixture git");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn git_text(root: &Path, args: &[&str]) -> String {
    String::from_utf8(git_output(root, args))
        .expect("git text")
        .trim()
        .to_owned()
}

pub(super) fn sha256_path(path: &Path) -> String {
    sha256_bytes(&fs::read(path).expect("digest input"))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
