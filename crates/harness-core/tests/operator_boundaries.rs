use harness_core::foreground_demote::*;

#[test]
fn demotion_requires_a_connected_runtime_and_a_registered_handle(
) -> Result<(), Box<dyn std::error::Error>> {
    let request = DemoteToBackgroundRequest::new("task-1", ForegroundKind::Task);
    let called = std::cell::Cell::new(false);
    let unavailable = apply_demote_to_background(&request, false, |_, _| {
        called.set(true);
        true
    })?;
    assert!(unavailable.is_unavailable());
    assert!(!called.get());
    let results = demote_task_handles_against_registry(&["task-1", "task-2"], &["task-1"])?;
    assert!(
        matches!(&results[0], DemoteToBackgroundResult::Demoted { handle_id, background_id, kind: ForegroundKind::Task } if handle_id == "task-1" && background_id == handle_id)
    );
    assert!(results[1].is_rejected());
    assert_eq!(summarize_demote_outcomes(&results).demoted, 1);
    assert!(default_demote_policy(&request)?.is_unavailable());
    assert!(demote_task_handle_against_registry(" ", &[]).is_err());
    Ok(())
}

#[test]
fn graph_queries_are_read_only_and_rebuilt_indexes_replace_stale_symbols(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::code_graph::*;
    let temp = tempfile::tempdir()?;
    let runtime = tempfile::tempdir()?;
    assert!(detect_persistent_graph(runtime.path()).is_unavailable());
    assert!(
        query_persistent_graph(runtime.path(), &GraphQuery::symbol_def("alpha")).is_unavailable()
    );
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    std::fs::write(
        temp.path().join("lib.rs"),
        "pub fn alpha() {\n    beta();\n    let function = beta;\n}\n",
    )?;
    std::fs::write(temp.path().join("other.rs"), "fn beta() {}\n")?;
    std::fs::create_dir(temp.path().join("target"))?;
    std::fs::write(temp.path().join("target/generated.rs"), "fn ignored() {}")?;
    std::fs::write(temp.path().join(".gitignore"), "ignored-source/\n")?;
    std::fs::create_dir(temp.path().join("ignored-source"))?;
    std::fs::write(
        temp.path().join("ignored-source/other.rs"),
        "fn ignored_vendor() {}",
    )?;
    let (path, index) = build_persistent_graph_index(temp.path(), runtime.path())?;
    assert_eq!(index.symbols.len(), 2);
    assert!(!temp.path().join(".harness").exists());
    assert!(detect_persistent_graph(runtime.path()).is_available());
    let batch = query_persistent_graph_batch(
        runtime.path(),
        &[
            GraphQuery::symbol_def("beta"),
            GraphQuery::with_kind("beta", GraphQueryKind::Callers),
        ],
    );
    assert_eq!(batch.summary().hit_results, 2);
    let GraphQueryResult::Hit { hits, .. } = &batch.results[1] else {
        return Err("caller was not found".into());
    };
    assert_eq!(
        hits.first()
            .map(|h| (h.symbol.as_str(), h.path.as_str(), h.line)),
        Some(("alpha", "lib.rs", 2))
    );
    for (symbol, kind, count) in [
        ("beta", GraphQueryKind::Callers, 1),
        ("alpha", GraphQueryKind::Callees, 1),
        ("beta", GraphQueryKind::References, 2),
    ] {
        assert_eq!(
            query_persistent_graph(runtime.path(), &GraphQuery::with_kind(symbol, kind))
                .hit_count(),
            count
        );
    }
    let before = std::fs::read(&path)?;
    assert_eq!(
        load_simple_graph_index(runtime.path())?
            .ok_or("index missing")?
            .symbols
            .len(),
        2
    );
    assert_eq!(std::fs::read(&path)?, before);
    std::fs::write(temp.path().join("other.rs"), "fn gamma() {}\n")?;
    build_persistent_graph_index(temp.path(), runtime.path())?;
    assert_eq!(
        query_persistent_graph(runtime.path(), &GraphQuery::symbol_def("beta")).hit_count(),
        0
    );
    #[cfg(unix)]
    {
        std::fs::write(temp.path().join("literal\\name.rs"), "fn backslash() {}\n")?;
        build_persistent_graph_index(temp.path(), runtime.path())?;
        assert!(
            matches!(query_persistent_graph(runtime.path(), &GraphQuery::symbol_def("backslash")), GraphQueryResult::Hit {hits, ..} if hits[0].path == "literal\\name.rs")
        );
        let before = std::fs::read(&path)?;
        let unsafe_path = temp.path().join("bad\n.rs");
        std::fs::write(&unsafe_path, "fn rogue() {}")?;
        assert!(build_persistent_graph_index(temp.path(), runtime.path()).is_err());
        assert_eq!(std::fs::read(&path)?, before);
        std::fs::remove_file(unsafe_path)?;
    }
    std::fs::write(&path, "{incomplete")?;
    assert!(
        query_persistent_graph(runtime.path(), &GraphQuery::symbol_def("gamma")).is_unavailable()
    );
    assert_eq!(std::fs::read_to_string(&path)?, "{incomplete");
    Ok(())
}

#[cfg(unix)]
#[test]
fn jujutsu_discovery_is_read_only_and_command_results_remove_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::jujutsu::*;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let binary = temp.path().join("jj-fixture");
    std::fs::write(
        &binary,
        "#!/bin/sh\ntouch executed\nprintf 'jj 1.0 api_key=hidden-key\\n'\n",
    )?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
    let missing = probe_jujutsu_with(temp.path(), |_| Some(binary.clone()));
    assert!(!missing.is_ready());
    assert!(!temp.path().join(".jj").exists());
    assert!(!temp.path().join("executed").exists());
    ensure_jujutsu_repo_marker(temp.path())?;
    let nested = temp.path().join("nested");
    std::fs::create_dir(&nested)?;
    assert!(detect_jujutsu_workspace(&nested).is_repo());
    let probe = probe_jujutsu_with(temp.path(), |_| Some(binary.clone()));
    assert!(probe.is_ready());
    let walk = run_jujutsu_diagnostic_walk_with_probe(&probe);
    assert!(walk.outcomes.iter().all(JujutsuCommandOutcome::is_ok));
    assert!(temp.path().join("executed").exists());
    assert!(!serde_json::to_string(&walk)?.contains("hidden-key"));
    assert!(probe_jujutsu_with(temp.path(), |_| None)
        .cli
        .is_unavailable());
    Ok(())
}

#[test]
fn sandbox_probes_never_claim_enforcement_or_grant_a_read_only_workspace(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::sandbox::*;
    let temp = tempfile::tempdir()?;
    let roots = SandboxPathRoots {
        workspace_root: temp.path().join("workspace"),
        harness_state_dir: temp.path().join("state"),
        temp_dir: temp.path().to_owned(),
    };
    let support = LandlockSupport::Available {
        detection: "fixture".into(),
    };
    let called = std::cell::Cell::new(false);
    let apply = |_: &SandboxFsPlan| {
        called.set(true);
        Ok(())
    };
    assert!(matches!(
        prepare_sandbox_for_spawn(
            SandboxPolicy::Off,
            SandboxPlatform::Linux,
            &support,
            None,
            Some(&apply)
        ),
        SandboxPrepareResult::NotRequired { .. }
    ));
    assert!(!called.get());
    assert!(matches!(
        prepare_sandbox_for_spawn(
            SandboxPolicy::Strict,
            SandboxPlatform::Linux,
            &support,
            Some(&roots),
            None
        ),
        SandboxPrepareResult::Unavailable { .. }
    ));
    let plan = build_fs_plan(SandboxPolicy::ReadOnly, &roots).ok_or("missing plan")?;
    assert!(!plan
        .write_roots
        .iter()
        .any(|p| roots.workspace_root.starts_with(p)));
    assert!(plan.write_roots.contains(&roots.harness_state_dir));
    assert!(matches!(
        prepare_sandbox_for_spawn(
            SandboxPolicy::Strict,
            SandboxPlatform::Linux,
            &support,
            Some(&roots),
            Some(&apply)
        ),
        SandboxPrepareResult::Prepared { .. }
    ));
    assert!(called.get());
    let failure = |_: &SandboxFsPlan| Err("api_key=do-not-record".into());
    let failed = prepare_sandbox_for_spawn(
        SandboxPolicy::Strict,
        SandboxPlatform::Linux,
        &support,
        Some(&roots),
        Some(&failure),
    );
    assert!(matches!(failed, SandboxPrepareResult::Unavailable { .. }));
    assert!(!failed.one_line().contains("do-not-record"));
    let probe = probe_os_sandbox_product(Some(&roots));
    assert!(probe.non_off_prepare_all_unavailable());
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    assert!(SandboxNetworkPolicy::parse("tcp:0,443").is_err());
    assert!(SandboxNetworkPolicy::parse("tcp:").is_err());
    assert!(matches!(
        evaluate_network_confinement_with_landlock(
            &SandboxNetworkPolicy::DenyAll,
            SandboxPlatform::Linux,
            &support
        ),
        NetworkConfinementStatus::Unavailable { .. }
    ));
    Ok(())
}
