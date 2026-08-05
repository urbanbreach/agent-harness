#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use std::fs;
use std::path::PathBuf;

use harness_testkit::tui_fidelity_compare::hash_bytes;
use harness_testkit::tui_fidelity_task_gate::{self, TaskGateInput};

#[test]
fn task_gate_admits_verifies_and_completes_only_once() {
    let fixture = Fixture::new();

    let admission = tui_fidelity_task_gate::admit(&fixture.input()).expect("task admission");
    assert_eq!(admission, fixture.admission_receipt);

    fixture.write_verification_inputs();
    let verification = tui_fidelity_task_gate::verify(&fixture.input()).expect("task verification");
    assert_eq!(verification, fixture.gate_receipt);

    let completion = tui_fidelity_task_gate::complete(&fixture.input()).expect("task completion");
    assert_eq!(completion, fixture.completion_receipt);
    assert!(fs::read_to_string(&fixture.plan)
        .expect("completed plan")
        .contains("- [x] 58. Repair"));
    let boulder: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture.boulder).expect("completed Boulder"))
            .expect("completed Boulder JSON");
    assert!(boulder["pending_plan_sha256"].as_str().is_some());
    assert!(boulder["task_completion_receipts"]["58"].is_object());
    assert!(boulder["works"]["work-1"]["pending_plan_sha256"]
        .as_str()
        .is_some());
    assert!(boulder["works"]["work-1"]["task_completion_receipts"]["58"].is_object());
    assert_eq!(
        boulder["pending_plan_sha256"],
        boulder["works"]["work-1"]["pending_plan_sha256"]
    );
    assert_eq!(
        boulder["task_completion_receipts"]["58"],
        boulder["works"]["work-1"]["task_completion_receipts"]["58"]
    );

    let error = tui_fidelity_task_gate::complete(&fixture.input())
        .expect_err("completion replay must fail");
    assert!(error.to_string().contains("receipt replay"));
}

#[test]
fn task_gate_rejects_worker_spawn_bootstrap() {
    let fixture = Fixture::new();
    let mut boulder: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture.boulder).expect("Boulder"))
            .expect("Boulder JSON");
    boulder["bootstrap_admission_58"]["worker_spawn_allowed"] = serde_json::Value::Bool(true);
    fs::write(
        &fixture.boulder,
        serde_json::to_vec_pretty(&boulder).expect("Boulder JSON"),
    )
    .expect("mutated Boulder");

    let error = tui_fidelity_task_gate::admit(&fixture.input())
        .expect_err("worker-spawning bootstrap must fail closed");
    assert!(error.to_string().contains("worker spawning"));
}

#[test]
fn task_gate_rejects_divergent_active_work_completion_mirror() {
    let fixture = Fixture::new();
    let mut boulder: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture.boulder).expect("Boulder"))
            .expect("Boulder JSON");
    boulder["works"]["work-1"]["pending_plan_sha256"] =
        serde_json::Value::String("divergent".to_owned());
    fs::write(
        &fixture.boulder,
        serde_json::to_vec_pretty(&boulder).expect("Boulder JSON"),
    )
    .expect("mutated Boulder");

    let error = tui_fidelity_task_gate::admit(&fixture.input())
        .expect_err("divergent active-work mirror must fail closed");
    assert!(error.to_string().contains("mirror divergence"));
}

#[test]
fn task_gate_finalization_failure_rolls_back_all_published_state() {
    let fixture = Fixture::new();
    tui_fidelity_task_gate::admit(&fixture.input()).expect("task admission");
    fixture.write_verification_inputs();
    tui_fidelity_task_gate::verify(&fixture.input()).expect("task verification");

    let mut input = fixture.input();
    input.finalize_closure = true;
    let error = tui_fidelity_task_gate::complete(&input)
        .expect_err("missing closure receipt must fail closed");

    assert!(error.to_string().contains("closure receipt"));
    assert!(fs::read_to_string(&fixture.plan)
        .expect("rolled-back plan")
        .contains("- [ ] 58. Repair"));
    let boulder: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&fixture.boulder).expect("rolled-back Boulder"))
            .expect("rolled-back Boulder JSON");
    assert!(boulder.get("pending_plan_sha256").is_none());
    assert!(!fixture.completion_receipt.exists());
}

struct Fixture {
    root: tempfile::TempDir,
    candidate: String,
    boulder: PathBuf,
    plan: PathBuf,
    evidence_root: PathBuf,
    admission_receipt: PathBuf,
    aggregate_receipt: PathBuf,
    verification_receipt: PathBuf,
    gate_receipt: PathBuf,
    completion_receipt: PathBuf,
    revocations: Vec<PathBuf>,
    shard: PathBuf,
    sealed_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("task gate root");
        let plan = root.path().join("plan.md");
        fs::write(&plan, "- [ ] 58. Repair\n").expect("plan");
        let plan_sha = hash_bytes(&fs::read(&plan).expect("plan bytes")).expect("plan hash");
        let revocations = ["revocation-a.json", "revocation-b.json"]
            .into_iter()
            .map(|name| root.path().join(name))
            .collect::<Vec<_>>();
        for (index, path) in revocations.iter().enumerate() {
            fs::write(
                path,
                format!("{{\"status\":\"REJECTED\",\"index\":{index}}}"),
            )
            .expect("revocation");
        }
        let bindings = revocations
            .iter()
            .map(|path| {
                serde_json::json!({
                    "path": path.to_string_lossy(),
                    "sha256": hash_bytes(&fs::read(path).expect("revocation bytes")).expect("revocation hash"),
                })
            })
            .collect::<Vec<_>>();
        let boulder = root.path().join("boulder.json");
        fs::write(
            &boulder,
            serde_json::to_vec_pretty(&serde_json::json!({
                "status": "active",
                "active_work_id": "work-1",
                "active_plan": "plan.md",
                "plan_name": "plan",
                "session_ids": [],
                "task_sessions": {},
                "reviewed_plan_sha256": plan_sha,
                "completion_revocation_bindings": bindings,
                "bootstrap_admission_58": {
                    "status": "admitted",
                    "task": 58,
                    "execution_owner": "root_orchestrator",
                    "worker_spawn_allowed": false,
                    "task_sessions_empty_at_promotion": true,
                },
                "works": {
                    "work-1": {
                        "status": "active",
                        "active_plan": "plan.md",
                        "plan_name": "plan",
                        "session_ids": [],
                    }
                },
            }))
            .expect("Boulder JSON"),
        )
        .expect("Boulder");
        let evidence_root = root.path().join("evidence");
        let sealed_root = evidence_root.join("sealed");
        let admission_receipt = evidence_root.join("admission.json");
        let aggregate_receipt = evidence_root.join("watchdog.json");
        let verification_receipt = evidence_root.join("verification.json");
        let gate_receipt = evidence_root.join("gate.json");
        let completion_receipt = evidence_root.join("completion.json");
        let shard = evidence_root.join("owner-shard.json");
        Self {
            root,
            candidate: "a".repeat(40),
            boulder,
            plan,
            evidence_root,
            admission_receipt,
            aggregate_receipt,
            verification_receipt,
            gate_receipt,
            completion_receipt,
            revocations,
            shard,
            sealed_root,
        }
    }

    fn input(&self) -> TaskGateInput {
        TaskGateInput {
            task: "58".to_owned(),
            candidate_sha256: self.candidate.clone(),
            boulder: self.boulder.clone(),
            plan: self.plan.clone(),
            evidence_root: self.evidence_root.clone(),
            admission_receipt: self.admission_receipt.clone(),
            aggregate_receipt: self.aggregate_receipt.clone(),
            verification_receipt: self.verification_receipt.clone(),
            gate_receipt: self.gate_receipt.clone(),
            completion_receipt: self.completion_receipt.clone(),
            revocations: self.revocations.clone(),
            shards: vec![self.shard.clone()],
            dependencies: Vec::new(),
            finalize_closure: false,
            closure_receipt: None,
        }
    }

    fn write_verification_inputs(&self) {
        fs::create_dir_all(&self.sealed_root).expect("sealed evidence");
        fs::write(
            &self.sealed_root.join("seal-manifest.json"),
            serde_json::json!({"records": [{"state": "passed"}]}).to_string(),
        )
        .expect("seal manifest");
        fs::write(
            &self.aggregate_receipt,
            serde_json::json!({
                "schema_version": "harness.tui-fidelity.watchdog.v1",
                "watchdog": "scripts/tui-fidelity/watchdog.sh",
                "status": "passed",
                "command": ["owner-shard"],
                "deadline_seconds": 120,
                "duration_millis": 10,
                "exit_code": 0,
                "timed_out": false,
                "cancelled": false,
                "surviving_process_group": false,
            })
            .to_string(),
        )
        .expect("watchdog receipt");
        fs::write(
            &self.verification_receipt,
            serde_json::json!({
                "schema_version": "harness.tui-fidelity.verification.v1",
                "sealed": true,
                "candidate_sha": self.candidate,
                "evidence_path": self.sealed_root,
                "key_count": 1,
                "scheduler": {"passed": 1, "failed": 0, "cancelled": 0, "skipped": 0},
            })
            .to_string(),
        )
        .expect("verification receipt");
        fs::write(
            &self.shard,
            serde_json::json!({"status": "passed", "candidate_sha256": self.candidate}).to_string(),
        )
        .expect("owner shard");
    }
}
