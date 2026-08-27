use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn off_policy_is_available_and_not_required() {
    // arrange
    // act
    // assert
    // Given: Off policy on any platform
    let platform = SandboxPlatform::Linux;

    // When
    let availability = evaluate_availability_for_platform(SandboxPolicy::Off, platform);
    let prepared = prepare_sandbox_for_platform(SandboxPolicy::Off, platform);

    // Then: available no-op; spawn may proceed without claiming confinement
    assert!(availability.is_available());
    assert_eq!(prepared, SandboxPrepareResult::NotRequired { platform });
    assert!(prepared.allows_spawn_without_false_success());
    assert!(!prepared.is_unavailable());
}

#[test]
fn sandbox_policy_parse_rejects_unknown_input() {
    // arrange
    // act
    // assert
    // Public policy names parse; anything else stays None so callers fail closed
    // to Off (see resolve_os_sandbox_policy in harness-tools shell_run).
    assert_eq!(SandboxPolicy::parse("off"), Some(SandboxPolicy::Off));
    assert_eq!(
        SandboxPolicy::parse("workspace_write"),
        Some(SandboxPolicy::WorkspaceWrite)
    );
    assert_eq!(SandboxPolicy::parse("strict"), Some(SandboxPolicy::Strict));
    assert_eq!(SandboxPolicy::parse(""), None);
    assert_eq!(SandboxPolicy::parse("permissive"), None);
}

#[test]
fn non_off_policy_returns_structured_unavailable_not_silent_allow() {
    // arrange
    // act
    // assert
    // Given: confinement-required policies (default prepare path does not apply Landlock)
    for policy in [
        SandboxPolicy::WorkspaceWrite,
        SandboxPolicy::ReadOnly,
        SandboxPolicy::Strict,
    ] {
        for platform in [
            SandboxPlatform::Linux,
            SandboxPlatform::Macos,
            SandboxPlatform::Windows,
            SandboxPlatform::Other,
        ] {
            // When
            let availability = evaluate_availability_for_platform(policy, platform);
            let prepared = prepare_sandbox_for_platform(policy, platform);

            // Then: structured unavailable — never Available/Prepared (fake success)
            assert!(
                availability.is_unavailable(),
                "policy={policy} platform={platform} should be unavailable"
            );
            assert!(
                prepared.is_unavailable(),
                "prepare should be unavailable for {policy} on {platform}"
            );
            assert!(
                !prepared.allows_spawn_without_false_success(),
                "must not treat unavailable confinement as allow"
            );
            match prepared {
                SandboxPrepareResult::Unavailable {
                    policy: p,
                    platform: pl,
                    reason,
                } => {
                    assert_eq!(p, policy);
                    assert_eq!(pl, platform);
                    assert!(
                        reason.contains("not enforceable")
                            || reason.contains("not implemented")
                            || reason.contains("not wired")
                            || reason.contains("Landlock"),
                        "reason should explain unavailability: {reason}"
                    );
                }
                other => panic!("expected Unavailable, got {other:?}"),
            }
        }
    }
}

#[test]
fn parse_public_policy_names() {
    // arrange
    // act
    // assert
    assert_eq!(SandboxPolicy::parse("off"), Some(SandboxPolicy::Off));
    assert_eq!(
        SandboxPolicy::parse("workspace"),
        Some(SandboxPolicy::WorkspaceWrite)
    );
    assert_eq!(
        SandboxPolicy::parse("read-only"),
        Some(SandboxPolicy::ReadOnly)
    );
    assert_eq!(SandboxPolicy::parse("strict"), Some(SandboxPolicy::Strict));
    assert_eq!(SandboxPolicy::parse("bogus"), None);
    assert!(matches!(
        require_policy("nope"),
        Err(SandboxError::UnknownPolicy { .. })
    ));
}

#[test]
fn permissions_layer_is_documented_as_distinct_from_sandbox() {
    // arrange
    // act
    // assert
    let off = prepare_sandbox_for_platform(SandboxPolicy::Off, SandboxPlatform::Linux);
    assert!(matches!(off, SandboxPrepareResult::NotRequired { .. }));
    let strict = prepare_sandbox_for_platform(SandboxPolicy::Strict, SandboxPlatform::Linux);
    assert!(matches!(strict, SandboxPrepareResult::Unavailable { .. }));
}

#[test]
fn list_os_profiles_covers_all_policies_with_honest_availability() {
    // arrange
    // act
    // assert
    // Given: Linux host bucket (injectable)
    let platform = SandboxPlatform::Linux;

    // When
    let profiles = list_os_profiles_for_platform(platform);

    // Then: every public policy once; only Off is available without child apply wiring
    assert_eq!(profiles.len(), OS_SANDBOX_POLICIES.len());
    let ids: Vec<&str> = profiles.iter().map(OsSandboxProfile::policy_id).collect();
    assert_eq!(ids, vec!["off", "workspace_write", "read_only", "strict"]);
    for profile in &profiles {
        if profile.policy == SandboxPolicy::Off {
            assert!(
                profile.is_available(),
                "Off must be available without enforcement"
            );
        } else {
            assert!(
                !profile.is_available(),
                "non-Off must not claim enforcement: {}",
                profile.policy_id()
            );
            assert!(matches!(
                profile.availability,
                SandboxAvailability::Unavailable { .. }
            ));
        }
    }
}

#[test]
fn detect_landlock_reports_available_when_probe_succeeds() {
    // arrange
    // act
    // assert
    // Given / When
    let support = detect_landlock_with(|| LandlockSupport::Available {
        detection: "test_probe".to_string(),
    });

    // Then
    assert!(support.is_available());
    assert_eq!(
        support,
        LandlockSupport::Available {
            detection: "test_probe".to_string()
        }
    );
}

#[test]
fn detect_landlock_reports_unavailable_when_probe_fails() {
    // arrange
    // act
    // assert
    // Given / When
    let support = detect_landlock_with(|| LandlockSupport::Unavailable {
        reason: "Landlock LSM not present".to_string(),
    });

    // Then
    assert!(support.is_unavailable());
    match support {
        LandlockSupport::Unavailable { reason } => {
            assert!(reason.contains("Landlock"));
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn probe_lsm_list_detects_landlock_token() {
    // arrange
    // act
    // assert
    assert!(lsm_list_contains_landlock("capability,landlock,yama"));
    assert!(lsm_list_contains_landlock("landlock"));
    assert!(!lsm_list_contains_landlock("capability,yama,bpf"));
    assert!(!lsm_list_contains_landlock(""));
    assert!(!lsm_list_contains_landlock("landlocker")); // substring false friend
}

#[test]
fn build_fs_plan_workspace_write_allows_workspace_and_temp_writes() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();

    // When
    let plan = build_fs_plan(SandboxPolicy::WorkspaceWrite, &roots)
        .expect("workspace_write should build a plan");

    // Then
    assert_eq!(plan.policy, SandboxPolicy::WorkspaceWrite);
    assert!(plan.write_roots.contains(&roots.workspace_root));
    assert!(plan.write_roots.contains(&roots.temp_dir));
    assert!(plan.write_roots.contains(&roots.harness_state_dir));
    assert!(plan.read_roots.iter().any(|p| p.as_os_str() == "/usr"));
    assert!(plan.read_roots.iter().any(|p| p.as_os_str() == "/bin"));
}

#[test]
fn build_fs_plan_read_only_writes_only_state_and_temp() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();

    // When
    let plan =
        build_fs_plan(SandboxPolicy::ReadOnly, &roots).expect("read_only should build a plan");

    // Then
    assert!(!plan.write_roots.contains(&roots.workspace_root));
    assert!(plan.write_roots.contains(&roots.harness_state_dir));
    assert!(plan.write_roots.contains(&roots.temp_dir));
}

#[test]
fn build_fs_plan_strict_limits_reads_to_workspace_and_essentials() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();

    // When
    let plan = build_fs_plan(SandboxPolicy::Strict, &roots).expect("strict should build a plan");

    // Then: no broad /usr read; workspace is readable; writes limited
    assert!(plan.read_roots.contains(&roots.workspace_root));
    assert!(!plan.read_roots.iter().any(|p| p.as_os_str() == "/usr"));
    assert!(!plan.write_roots.contains(&roots.workspace_root));
    assert!(plan.write_roots.contains(&roots.harness_state_dir));
}

#[test]
fn build_fs_plan_off_returns_none() {
    // arrange
    // act
    // assert
    assert!(build_fs_plan(SandboxPolicy::Off, &sample_roots()).is_none());
}

#[test]
fn prepare_for_spawn_fails_closed_when_landlock_missing() {
    // arrange
    // act
    // assert
    // Given
    let landlock = LandlockSupport::Unavailable {
        reason: "Landlock LSM not enabled".to_string(),
    };
    let roots = sample_roots();
    let apply_calls = AtomicUsize::new(0);

    // When
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::WorkspaceWrite,
        SandboxPlatform::Linux,
        &landlock,
        Some(&roots),
        Some(&|_| {
            apply_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );

    // Then: unavailable, apply never called
    assert!(prepared.is_unavailable());
    assert!(!prepared.allows_spawn_without_false_success());
    assert_eq!(apply_calls.load(Ordering::SeqCst), 0);
    match prepared {
        SandboxPrepareResult::Unavailable { reason, .. } => {
            assert!(
                reason.contains("Landlock") || reason.contains("not enabled"),
                "reason={reason}"
            );
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn prepare_for_spawn_never_claims_prepared_without_apply() {
    // arrange
    // act
    // assert
    // Given: Landlock present but no apply callback (bash pre_exec not wired)
    let landlock = LandlockSupport::Available {
        detection: "lsm_list".to_string(),
    };
    let roots = sample_roots();

    // When
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::ReadOnly,
        SandboxPlatform::Linux,
        &landlock,
        Some(&roots),
        None,
    );

    // Then: fail closed — Landlock present is not silent allow
    assert!(prepared.is_unavailable());
    assert!(!prepared.allows_spawn_without_false_success());
    match prepared {
        SandboxPrepareResult::Unavailable { reason, .. } => {
            assert!(
                reason.contains("not wired")
                    || reason.contains("pre_exec")
                    || reason.contains("apply"),
                "reason should mention missing apply wiring: {reason}"
            );
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn prepare_for_spawn_prepared_only_when_apply_succeeds() {
    // arrange
    // act
    // assert
    // Given
    let landlock = LandlockSupport::Available {
        detection: "lsm_list".to_string(),
    };
    let roots = sample_roots();
    let apply_calls = AtomicUsize::new(0);

    // When
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::WorkspaceWrite,
        SandboxPlatform::Linux,
        &landlock,
        Some(&roots),
        Some(&|plan| {
            apply_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(plan.policy, SandboxPolicy::WorkspaceWrite);
            assert!(!plan.write_roots.is_empty());
            Ok(())
        }),
    );

    // Then
    assert_eq!(apply_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        prepared,
        SandboxPrepareResult::Prepared {
            policy: SandboxPolicy::WorkspaceWrite,
            platform: SandboxPlatform::Linux,
        }
    );
    assert!(prepared.allows_spawn_without_false_success());
}

#[test]
fn prepare_for_spawn_unavailable_when_apply_fails() {
    // arrange
    // act
    // assert
    // Given
    let landlock = LandlockSupport::Available {
        detection: "lsm_list".to_string(),
    };
    let roots = sample_roots();

    // When
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::Strict,
        SandboxPlatform::Linux,
        &landlock,
        Some(&roots),
        Some(&|_| Err("landlock_restrict_self failed".to_string())),
    );

    // Then
    assert!(prepared.is_unavailable());
    assert!(!prepared.allows_spawn_without_false_success());
    match prepared {
        SandboxPrepareResult::Unavailable { reason, .. } => {
            assert!(reason.contains("landlock_restrict_self") || reason.contains("failed"));
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn prepare_for_spawn_unavailable_on_non_linux_even_with_apply() {
    // arrange
    // act
    // assert
    // Given
    let landlock = LandlockSupport::Available {
        detection: "ignored".to_string(),
    };
    let roots = sample_roots();

    // When
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::WorkspaceWrite,
        SandboxPlatform::Macos,
        &landlock,
        Some(&roots),
        Some(&|_| Ok(())),
    );

    // Then: Seatbelt not implemented — fail closed
    assert!(prepared.is_unavailable());
    match prepared {
        SandboxPrepareResult::Unavailable { reason, .. } => {
            assert!(
                reason.contains("macos")
                    || reason.contains("Seatbelt")
                    || reason.contains("not implemented"),
                "reason={reason}"
            );
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[test]
fn bash_spawn_integration_point_is_documented() {
    // arrange
    // act
    // assert
    assert!(BASH_SPAWN_SANDBOX_INTEGRATION.contains("shell_run"));
    assert!(BASH_SPAWN_SANDBOX_INTEGRATION.contains("TokioShellCommandRunner"));
}

#[test]
fn apply_placeholder_fails_closed() {
    // arrange
    // act
    // assert
    let plan = build_fs_plan(SandboxPolicy::WorkspaceWrite, &sample_roots()).expect("plan");
    let err = apply_landlock_fs_plan_not_implemented(&plan).expect_err("must fail closed");
    assert!(
        err.contains("refused in-process")
            || err.contains("pre_exec")
            || err.contains("not implemented"),
        "unexpected honesty error: {err}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn apply_landlock_fs_plan_enforces_in_child_process() {
    // arrange
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    if !detect_landlock().is_available() {
        return;
    }

    let base = std::env::temp_dir().join(format!(
        "harness-landlock-enforcement-{}",
        std::process::id()
    ));
    let workspace = base.join("ws");
    let state = workspace.join(".agent-harness");
    let temp = base.join("tmp");
    std::fs::create_dir_all(&state).expect("workspace state");
    std::fs::create_dir_all(&temp).expect("temp");
    let roots = SandboxPathRoots {
        workspace_root: workspace.clone(),
        harness_state_dir: state,
        temp_dir: temp,
    };
    let plan = build_fs_plan(SandboxPolicy::WorkspaceWrite, &roots).expect("plan");
    // Outside write_roots but under the same parent temp tree so the path is real.
    let outside = base.join("outside-deny.txt");
    let _ = std::fs::remove_file(&outside);
    let outside_path = outside.display().to_string();
    let plan_for_child = plan.clone();

    // act — child pre_exec applies Landlock, then tries to write outside write_roots
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(format!("echo confined > '{outside_path}'"));
    // SAFETY: pre_exec runs only in the forked child before exec; Landlock
    // restrict_self confines that child only. Test-only audited call site.
    #[allow(unsafe_code, reason = "test-only audited pre_exec Landlock call")]
    unsafe {
        command.pre_exec(move || {
            apply_landlock_fs_plan(&plan_for_child).map_err(std::io::Error::other)?;
            Ok(())
        });
    }
    let status = command.status().expect("spawn child");

    // assert — write outside roots fails under enforced plan
    assert!(
        !status.success() || !outside.exists(),
        "child write outside write_roots must fail under Landlock; status={status:?} exists={}",
        outside.exists()
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn prepare_with_real_apply_hook_is_prepared_when_landlock_available() {
    // arrange
    // act
    // assert
    let roots = sample_roots();
    let landlock = detect_landlock();
    if !landlock.is_available() {
        return;
    }
    // Do not call real apply in-process; use a hook that records the plan.
    let prepared = prepare_sandbox_for_spawn(
        SandboxPolicy::WorkspaceWrite,
        SandboxPlatform::Linux,
        &landlock,
        Some(&roots),
        Some(&|plan: &SandboxFsPlan| {
            assert!(!plan.read_roots.is_empty());
            assert!(!plan.write_roots.is_empty());
            Ok(())
        }),
    );
    assert!(
        matches!(prepared, SandboxPrepareResult::Prepared { .. }),
        "expected Prepared when apply hook succeeds: {prepared:?}"
    );
}

#[test]
fn summarize_fs_plan_reports_counts_and_paths() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();
    let plan = build_fs_plan(SandboxPolicy::WorkspaceWrite, &roots).expect("plan");

    // When
    let summary = summarize_fs_plan(&plan);

    // Then
    assert_eq!(summary.policy, SandboxPolicy::WorkspaceWrite);
    assert_eq!(summary.read_root_count, plan.read_roots.len());
    assert_eq!(summary.write_root_count, plan.write_roots.len());
    assert!(summary
        .write_roots
        .iter()
        .any(|p| p == "/tmp/ws" || p.ends_with("/tmp/ws")));
    assert!(summary.one_line().contains("policy=workspace_write"));
    assert!(summary.one_line().contains("read_roots="));
}

#[test]
fn describe_fs_plan_for_policy_none_for_off_and_some_for_enforced() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();

    // When / Then
    assert!(describe_fs_plan_for_policy(SandboxPolicy::Off, &roots).is_none());
    let strict = describe_fs_plan_for_policy(SandboxPolicy::Strict, &roots).expect("strict plan");
    assert_eq!(strict.policy, SandboxPolicy::Strict);
    assert!(strict.write_root_count >= 1);
    assert!(strict
        .write_roots
        .iter()
        .all(|p| !p.contains("/tmp/ws") || p.contains(".agent-harness") || p == "/tmp"));
}

#[test]
fn landlock_support_one_line_covers_available_and_unavailable() {
    // arrange
    // act
    // assert
    // Given
    let available = LandlockSupport::Available {
        detection: "lsm=landlock".to_string(),
    };
    let unavailable = LandlockSupport::Unavailable {
        reason: "Landlock is Linux-only; not available on this platform".to_string(),
    };

    // When / Then
    assert!(available.is_available());
    assert!(available.one_line().contains("Landlock: available"));
    assert!(available.one_line().contains("lsm=landlock"));
    assert!(unavailable.is_unavailable());
    assert!(unavailable.one_line().contains("Landlock: unavailable"));
    assert!(unavailable.one_line().contains("Linux-only"));

    // When: injectable detect paths also surface diagnostics
    let detected = detect_landlock_with(|| LandlockSupport::Available {
        detection: "probe-ok".to_string(),
    });
    let failed = detect_landlock_with(|| LandlockSupport::Unavailable {
        reason: "probe-failed".to_string(),
    });
    assert!(detected.one_line().contains("probe-ok"));
    assert!(failed.one_line().contains("probe-failed"));
}

fn sample_roots() -> SandboxPathRoots {
    SandboxPathRoots {
        workspace_root: PathBuf::from("/tmp/ws"),
        harness_state_dir: PathBuf::from("/tmp/ws/.agent-harness"),
        temp_dir: PathBuf::from("/tmp"),
    }
}

#[test]
fn probe_os_sandbox_product_multi_policy_walk_is_honest() {
    // arrange
    // act
    // assert
    // Given
    let roots = sample_roots();
    let platform = SandboxPlatform::Linux;

    // When
    let probe = probe_os_sandbox_product_for_platform(platform, Some(&roots));

    // Then: full public policy inventory + prepare walk ends on Strict
    assert_eq!(probe.platform, platform);
    assert_eq!(probe.profiles.len(), OS_SANDBOX_POLICIES.len());
    assert_eq!(probe.profiles_summary.total, OS_SANDBOX_POLICIES.len());
    assert_eq!(
        probe.profiles_summary.available + probe.profiles_summary.unavailable,
        probe.profiles_summary.total
    );
    assert_eq!(probe.prepare_results.len(), OS_SANDBOX_POLICIES.len());
    assert!(
        probe.last_prepare.one_line().contains("strict"),
        "last prepare must be strict: {}",
        probe.last_prepare.one_line()
    );
    assert!(probe.non_off_prepare_all_unavailable());
    match &probe.last_prepare {
        SandboxPrepareResult::Unavailable {
            policy: SandboxPolicy::Strict,
            ..
        } => {}
        other => panic!("expected Strict Unavailable prepare, got {other:?}"),
    }

    // Then: multi-policy FS plan walk WorkspaceWrite→ReadOnly→Strict
    assert_eq!(
        probe.fs_plan_summaries.len(),
        OS_SANDBOX_ENFORCED_POLICIES.len()
    );
    let plan_ids: Vec<&str> = probe
        .fs_plan_summaries
        .iter()
        .map(|s| s.policy.as_str())
        .collect();
    assert_eq!(plan_ids, vec!["workspace_write", "read_only", "strict"]);
    let last = probe.last_fs_plan.as_ref().expect("last fs plan");
    assert_eq!(last.policy, SandboxPolicy::Strict);
    assert!(last.read_root_count >= 1);
    assert!(last.write_root_count >= 1);

    // Then: apply honesty fails closed per enforced plan (presence ≠ confinement)
    assert_eq!(
        probe.apply_honesty.len(),
        OS_SANDBOX_ENFORCED_POLICIES.len()
    );
    for reason in &probe.apply_honesty {
        assert!(
            reason.contains("not implemented") || reason.contains("pre_exec"),
            "apply honesty reason: {reason}"
        );
    }
    assert!(probe.landlock.one_line().contains("Landlock:"));
    assert!(probe.one_line().contains("OS sandbox product:"));
    assert!(probe.one_line().contains("last_plan=strict"));
    assert_eq!(probe.profiles[0].policy_id(), "off");
}

#[test]
fn probe_os_sandbox_product_without_roots_skips_fs_plan_walk() {
    // arrange
    // act
    // assert
    // When
    let probe = probe_os_sandbox_product_for_platform(SandboxPlatform::Linux, None);

    // Then: profiles + prepare still run; no FS plans without roots
    assert_eq!(probe.profiles_summary.total, OS_SANDBOX_POLICIES.len());
    assert_eq!(probe.prepare_results.len(), OS_SANDBOX_POLICIES.len());
    assert!(probe.fs_plan_summaries.is_empty());
    assert!(probe.last_fs_plan.is_none());
    assert!(probe.apply_honesty.is_empty());
    assert!(probe.non_off_prepare_all_unavailable());
}
