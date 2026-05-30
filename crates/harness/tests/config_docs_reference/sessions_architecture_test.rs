#[test]
fn sessions_docs_cover_lineage_source_cutoff_summary_and_artifact_semantics() {
    // arrange
    let sessions = read_doc("docs/sessions-and-replay.md");
    let lineage_source = read_doc("crates/harness-core/src/session_lineage.rs");

    for source_anchor in [
        "fork = selected stable prefix",
        "clone = latest stable prefix",
        "source_cutoff_seq",
        "event_rewrite_policy",
        "artifact_policy",
        "copies only artifacts referenced by copied events after byte and digest validation",
    // act
    ] {
        // assert
        assert!(
            lineage_source.contains(source_anchor),
            "lineage implementation missing source anchor `{source_anchor}`"
        );
    }

    for doc_anchor in [
        "fork materializes an explicitly validated stable prefix",
        "clone selects the latest stable completed prefix",
        "source_cutoff_seq",
        "summaries and compaction checkpoints are copied only when copied source events reference them",
        "copied after byte and digest validation",
        "new child events append after the materialized boundary",
        "event_id/run_id/seq are regenerated",
        "correlation_id and causation_id are cleared",
        "restored context is replay-derived from the child log",
    ] {
        assert!(
            sessions.contains(doc_anchor),
            "sessions docs missing lineage semantics anchor `{doc_anchor}`"
        );
    }
}

#[test]
fn sessions_docs_cover_resume_acceptance_realistic_interrupted_session() {
    // arrange
    let sessions = read_doc("docs/sessions-and-replay.md");
    let resume_test = [
        read_doc("crates/harness-core/tests/coord/12_resume_existing_run_persists_bindings_for_test.rs"),
        read_doc("crates/harness-core/tests/coord/12b_resume_acceptance_restores_realistic_interrupted_session_test.rs"),
        read_doc("crates/harness-core/tests/coord/common/resume_acceptance_fixture.rs"),
    ]
    .join("\n");

    for source_anchor in [
        "resume_acceptance_restores_realistic_interrupted_session_and_continues",
        "loaded skill karpathy-guidelines",
        "todo checklist keeps resume acceptance in progress",
        "plan handoff references .agent-harness/plans/run_resume_acceptance_realistic.md",
        "resume artifact written",
        "PermissionGrantRecorded",
        "post-resume answer",
    // act
    ] {
        // assert
        assert!(
            resume_test.contains(source_anchor),
            "resume acceptance test missing source anchor `{source_anchor}`"
        );
    }

    for doc_anchor in [
        "realistic interrupted coding session",
        "loaded skill context",
        "todo checklist state",
        "plan handoff context",
        "tool artifact references",
        "resolved permission grants",
        "post-resume provider turn",
    ] {
        assert!(
            sessions.contains(doc_anchor),
            "sessions docs missing resume acceptance anchor `{doc_anchor}`"
        );
    }
}

#[test]
fn architecture_docs_cover_compaction_contracts_and_preservation_context() {
    // arrange
    let architecture = read_doc("docs/architecture.md");
    let provider_context = read_doc("crates/harness-core/src/coord/provider_context.rs");
    let config = read_doc("crates/harness-core/src/config.rs");
    let coord_tests = read_doc("crates/harness-core/src/coord/tests.rs");

    for source_anchor in [
        "fallback_input_tokens",
        "provider_context_keep_recent_tokens",
        "collect_compacted_file_operation_facts",
        "add_tool_operation_fact",
        "compaction_preserves_file_tool_skill_todo_and_plan_context",
    // act
    ] {
        // assert
        assert!(
            provider_context.contains(source_anchor)
                || config.contains(source_anchor)
                || coord_tests.contains(source_anchor),
            "compaction implementation missing source anchor `{source_anchor}`"
        );
    }

    for doc_anchor in [
        "Threshold policy",
        "fallback_input_tokens",
        "Retained recent turns",
        "provider_context_keep_recent_tokens",
        "File/tool/skill/todo/plan context",
        "Todo/plan bridging",
        "Post-compaction restoration hints",
        "preserved recent turns plus the live user prompt take precedence",
    ] {
        assert!(
            architecture.contains(doc_anchor),
            "architecture docs missing compaction contract anchor `{doc_anchor}`"
        );
    }
}

#[test]
fn docs_do_not_reference_broken_local_markdown_targets_or_deleted_prd_artifacts() {
    // arrange
    let root = repo_root();
    let deleted = [
        "docs/v1-agent-catalog-workspace-intelligence-prd.md",
        "docs/omo-parity-spec.md",
        "docs/v1-skill-contract-capability-governance-prd.md",
        "skills-lock.json",
    ];

    for markdown in markdown_files(&root.join("docs")) {
        let relative = markdown
            .strip_prefix(&root)
            .expect("markdown under repo root")
            .to_string_lossy()
            .replace('\\', "/");
        if matches!(
            relative.as_str(),
            "docs/v1-release-readiness-slice-prd.md"
                | "docs/v1-release-readiness-slice-progress.md"
        ) {
            continue;
        }
        let body = std::fs::read_to_string(&markdown)
            .unwrap_or_else(|err| panic!("read markdown {}: {err}", markdown.display()));
        // act
        for deleted_target in deleted {
            // assert
            assert!(
                !body.contains(deleted_target),
                "{} references deleted target {deleted_target}",
                markdown.display()
            );
        }
        for target in local_markdown_targets(&body) {
            if target.starts_with('#') {
                continue;
            }
            let path_part = target.split('#').next().unwrap_or(target.as_str());
            if path_part.is_empty()
                || path_part.starts_with("http://")
                || path_part.starts_with("https://")
            {
                continue;
            }
            let candidate = markdown.parent().expect("markdown parent").join(path_part);
            assert!(
                candidate.exists(),
                "{} references missing local markdown target {}",
                markdown.display(),
                target
            );
        }
    }
}

#[test]
fn readiness_closeout_docs_are_current_and_back_roadmap_claims() {
    // arrange
    let root = repo_root();
    let roadmap = read_doc("docs/roadmap-v1.md");
    let budgets = read_doc("docs/budgets.md");
    let progress = read_doc("docs/v1-release-readiness-slice-progress.md");
    let claim_matrix = read_doc("docs/claim-evidence-matrix.md");
    let prd_path = root.join("docs/v1-release-readiness-slice-prd.md");
    let progress_path = root.join("docs/v1-release-readiness-slice-progress.md");

    // act
    let active_references = [("roadmap", roadmap.as_str()), ("budgets", budgets.as_str())];

    // assert: active references point at restored, current files.
    assert!(prd_path.exists(), "readiness PRD must exist");
    assert!(progress_path.exists(), "readiness progress log must exist");
    for (doc_name, doc) in active_references {
        assert!(
            doc.contains("docs/v1-release-readiness-slice-progress.md"),
            "{doc_name} must reference the restored readiness progress log"
        );
    }

    // assert: the current strict-V1 closeout has a live progress/evidence row.
    for anchor in [
        "Evidence 2026-05-30 strict V1 roadmap closeout start",
        "roadmap-classification-baseline.md",
        "Baseline captured 56 unchecked roadmap items",
        "Checked release-blocker rows remain evidence-backed",
    ] {
        assert!(
            progress.contains(anchor),
            "readiness progress log missing current closeout anchor `{anchor}`"
        );
    }
    assert!(
        claim_matrix.contains("Strict V1 roadmap closeout readiness evidence is restored"),
        "claim matrix must include the restored readiness closeout evidence row"
    );

    // assert: every checked V1 release-blocker row has progress or matrix evidence text.
    let blocker_section = roadmap
        .split("## V1 release blockers\n")
        .nth(1)
        .expect("roadmap has V1 release blockers section")
        .split("\n## ")
        .next()
        .expect("release blockers section body");
    let evidence = format!("{progress}\n{claim_matrix}").replace('`', "");
    for checked in checked_markdown_items(blocker_section) {
        let evidence_anchor = checked
            .replace("`", "")
            .split_whitespace()
            .take(8)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            evidence.contains(&evidence_anchor),
            "checked release-blocker row lacks progress/matrix evidence anchor `{evidence_anchor}` from `{checked}`"
        );
    }
}

fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|err| panic!("read dir {}: {err}", dir.display()))
    {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            files.extend(markdown_files(&path));
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    files
}

fn checked_markdown_items(section: &str) -> Vec<String> {
    section
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- [x] ")
                .map(|item| item.trim().to_string())
        })
        .collect()
}

fn local_markdown_targets(body: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for segment in body.split("](").skip(1) {
        if let Some((target, _)) = segment.split_once(')') {
            targets.push(target.to_string());
        }
    }
    targets
}
