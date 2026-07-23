use super::*;

pub(super) fn seed_operator_host_probes_sets_binary_update_and_jujutsu_continuation(
    app: &mut AppState,
) {
    // Then: empty team/demote/remote-auth outcome tallies are seeded; cron seeds probe
    // schedules with executes=true product honesty.
    let teams = app
        .team_registry_summary()
        .expect("team registry summary bound");
    assert!(
        teams.teams >= 2 && teams.active >= 1 && teams.cancelled >= 1,
        "expected multi-team registry with active+cancelled: {teams:?}"
    );
    assert!(teams.members >= 2, "expected multi-member teams: {teams:?}");
    assert!(
        teams.mailbox_messages >= 1,
        "expected multi-message mailbox: {teams:?}"
    );
    let team_create = app.team_last_create().expect("team last create bound");
    assert!(
        team_create.one_line().contains("ok") || team_create.one_line().contains("(probe)"),
        "expected probe team create: {}",
        team_create.one_line()
    );
    let team_first = app.team_first_line().expect("team first bound");
    assert!(
        team_first.contains("(probe)") && team_first.contains("cancelled"),
        "expected cancelled probe team first after cancel success: {team_first}"
    );
    let team_send = app.team_last_send().expect("team last send bound");
    assert!(
        team_send.one_line().contains("ok") || team_send.one_line().contains("probe"),
        "expected probe team send: {}",
        team_send.one_line()
    );
    let team_msg = app
        .team_last_message_line()
        .expect("team last message bound");
    assert!(
        team_msg.contains("probe") || team_msg.contains("mailbox"),
        "expected probe mailbox message: {team_msg}"
    );
    let team_add = app
        .team_last_add_member()
        .expect("team last add-member bound");
    assert!(
        team_add.one_line().contains("ok") || team_add.one_line().contains("probe-agent"),
        "expected probe team add-member: {}",
        team_add.one_line()
    );
    let team_cancel = app.team_last_cancel().expect("team last cancel bound");
    assert!(
        team_cancel.one_line().contains("team cancel: ok"),
        "expected successful cancel of probe team: {}",
        team_cancel.one_line()
    );
    let cron = app
        .cron_schedule_summary()
        .expect("cron schedule summary bound");
    assert!(
        cron.registered >= 4 && cron.with_label >= 3,
        "expected multi-schedule cron registry after remove: {cron:?}"
    );
    assert!(!cron.executor_available);
    let cron_reg = app.cron_last_register().expect("cron last register bound");
    assert!(
        cron_reg.one_line().contains("ok") && cron_reg.one_line().contains("(probe-5)"),
        "expected last multi-register outcome for probe-5: {}",
        cron_reg.one_line()
    );
    let cron_remove = app.cron_last_remove().expect("cron last remove bound");
    assert!(
        cron_remove.one_line().contains("cron remove: ok")
            && cron_remove.one_line().contains("(probe)"),
        "expected successful remove of first probe schedule: {}",
        cron_remove.one_line()
    );
    let cron_first = app
        .cron_first_schedule_line()
        .expect("cron first schedule bound");
    assert!(
        cron_first.contains("(probe-2)") && cron_first.contains("executes=false"),
        "expected remaining probe-2 first schedule: {cron_first}"
    );
    let demote = app
        .demote_outcome_summary()
        .expect("demote outcome summary bound");
    // Multi demote probes: shell unavailable + multi reject + multi demote (total>=5)
    assert!(
        demote.total >= 5 && demote.demoted >= 2,
        "expected multi-handle demote batch: {demote:?}"
    );
    assert!(
        demote.unavailable >= 1 && demote.rejected >= 2,
        "expected unavailable+rejected+demoted mix: {demote:?}"
    );
    assert_eq!(
        demote.demoted + demote.unavailable + demote.rejected,
        demote.total
    );
    let demote_last = app.demote_last_result().expect("demote last result bound");
    assert!(
        demote_last.is_unavailable() || demote_last.is_rejected(),
        "expected shell probe unavailable/rejected: {}",
        demote_last.one_line()
    );
    let demote_task = app
        .demote_last_task_result()
        .expect("demote last task result bound");
    assert!(
        demote_task.is_demoted(),
        "expected demotable task success path: {}",
        demote_task.one_line()
    );
    assert!(
        demote_task.one_line().contains("probe-task-ok")
            || demote_task.one_line().contains("demoted"),
        "expected demoted task one_line: {}",
        demote_task.one_line()
    );
    let hub = app
        .workspace_hub_outcome_summary()
        .expect("workspace hub outcome summary bound");
    assert_eq!(hub.total, 4);
    assert_eq!(hub.connect_unavailable, 0);
    assert_eq!(hub.bind_unavailable, 0);
    assert_eq!(hub.upload_unavailable, 0);
    assert_eq!(hub.recover_unavailable, 0);
    assert!(!hub.all_unavailable());
    let hub_connect = app
        .workspace_hub_last_connect()
        .expect("workspace hub last connect bound");
    assert!(
        hub_connect.one_line().contains("connected"),
        "expected connected connect: {}",
        hub_connect.one_line()
    );
    let hub_bind = app
        .workspace_hub_last_bind()
        .expect("workspace hub last bind bound");
    assert!(
        hub_bind.one_line().contains("bound") && hub_bind.one_line().contains("ws-local-1"),
        "expected multi-endpoint last bound bind: {}",
        hub_bind.one_line()
    );
    let hub_upload = app
        .workspace_hub_last_upload()
        .expect("workspace hub last upload bound");
    assert!(
        hub_upload.one_line().contains("uploaded")
            && hub_upload.one_line().contains("artifacts/bundle.tar"),
        "expected multi-endpoint last uploaded upload: {}",
        hub_upload.one_line()
    );
    let hub_recover = app
        .workspace_hub_last_recover()
        .expect("workspace hub last recover bound");
    assert!(
        hub_recover.one_line().contains("recovered")
            && hub_recover.one_line().contains("hub-session-9"),
        "expected multi-endpoint last recovered recover: {}",
        hub_recover.one_line()
    );
    let hub_avail = app
        .workspace_hub_availability()
        .expect("workspace hub availability bound");
    assert!(hub_avail.is_available());
    assert!(hub_avail.one_line().contains("available"));
    let oidc = app
        .browser_oidc_outcome_summary()
        .expect("browser oidc outcome summary bound");
    assert_eq!(oidc.total, 2);
    assert_eq!(oidc.start_unavailable, 0);
    assert_eq!(oidc.complete_unavailable, 0);
    assert!(!oidc.all_unavailable());
    let oidc_start = app
        .browser_oidc_last_start()
        .expect("browser oidc last start bound");
    assert!(
        oidc_start.one_line().contains("started")
            && oidc_start.one_line().contains("issuer.example"),
        "expected multi-endpoint last OIDC start: {}",
        oidc_start.one_line()
    );
    let oidc_complete = app
        .browser_oidc_last_complete()
        .expect("browser oidc last complete bound");
    assert!(
        oidc_complete.one_line().contains("completed"),
        "expected completed multi-endpoint OIDC complete: {}",
        oidc_complete.one_line()
    );
    assert!(!oidc_complete.one_line().contains("probe-device"));
    let oidc_avail = app
        .browser_oidc_availability()
        .expect("browser oidc availability bound");
    assert!(oidc_avail.is_available());
    assert!(oidc_avail.one_line().contains("available"));
    let mcp = app
        .mcp_oauth_outcome_summary()
        .expect("mcp oauth outcome summary bound");
    assert_eq!(mcp.total, 3);
    assert_eq!(mcp.begin_unavailable, 0);
    assert_eq!(mcp.exchange_unavailable, 0);
    assert_eq!(mcp.open_unavailable, 0);
    assert!(!mcp.all_unavailable());
    let mcp_begin = app
        .mcp_oauth_last_begin()
        .expect("mcp oauth last begin bound");
    assert!(
        mcp_begin.one_line().contains("begun") && mcp_begin.one_line().contains("docs-server"),
        "expected multi-endpoint last MCP OAuth begin: {}",
        mcp_begin.one_line()
    );
    let mcp_exchange = app
        .mcp_oauth_last_exchange()
        .expect("mcp oauth last exchange bound");
    assert!(
        mcp_exchange.one_line().contains("exchanged")
            && mcp_exchange.one_line().contains("docs-server"),
        "expected multi-endpoint last MCP OAuth exchange: {}",
        mcp_exchange.one_line()
    );
    assert!(!mcp_exchange.one_line().contains("probe-device"));
    let mcp_open = app
        .mcp_oauth_last_open()
        .expect("mcp oauth last open bound");
    assert!(
        mcp_open.one_line().contains("opened") && mcp_open.one_line().contains("docs-server"),
        "expected multi-endpoint last MCP open: {}",
        mcp_open.one_line()
    );
    let mcp_avail = app
        .mcp_oauth_remote_availability()
        .expect("mcp oauth remote availability bound");
    assert!(mcp_avail.is_available());
    assert!(mcp_avail.one_line().contains("available"));
    let sleep = app
        .sleep_wake_observation_summary()
        .expect("sleep/wake observation summary bound");
    assert!(
        sleep.total >= 8 && sleep.recorded >= 8,
        "expected dual-cycle sleep/wake observations: {sleep:?}"
    );
    assert_eq!(sleep.recorded_noop, 0);
    assert!(
        app.sleep_wake_observation_log().len() >= 8,
        "expected dual-cycle observation log: {}",
        app.sleep_wake_observation_log().len()
    );
    let sleep_last = app
        .sleep_wake_last_observation()
        .expect("sleep/wake last observation bound");
    assert!(
        sleep_last.one_line().contains("suspend") || sleep_last.one_line().contains("recorded"),
        "expected last multi-event observation (suspend): {}",
        sleep_last.one_line()
    );
    assert!(sleep_last.is_recorded());
    assert!(!sleep_last.is_recorded_noop());
    let sleep_decision = app
        .sleep_wake_last_decision()
        .expect("sleep/wake last decision bound");
    assert!(sleep_decision.is_skip());
    assert!(!sleep_decision.claims_refresh());
    assert!(
        sleep_decision.one_line().contains("skip refresh")
            && sleep_decision.one_line().contains("suspend"),
        "expected skip decision for last suspend: {}",
        sleep_decision.one_line()
    );
    let sleep_policy = app
        .sleep_wake_credential_policy()
        .expect("sleep/wake credential policy bound");
    assert!(sleep_policy.is_active());
    assert!(
        sleep_policy.one_line().contains("active (strategy=hook)"),
        "expected active hook policy: {}",
        sleep_policy.one_line()
    );
    let sleep_avail = app
        .sleep_wake_availability()
        .expect("sleep/wake availability bound");
    assert!(
        sleep_avail.one_line().contains("active"),
        "expected active availability: {}",
        sleep_avail.one_line()
    );

    // When: apply one more host event through product API (not seed-only; no expiry)
    let decision =
        app.apply_sleep_wake_host_event(harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake);
    // Then: decision + summary advance; still skip without expiry snapshot
    assert!(decision.is_skip());
    assert!(!decision.claims_refresh());
    assert!(decision.one_line().contains("wake"));
    let sleep_after = app
        .sleep_wake_observation_summary()
        .expect("sleep/wake summary after apply");
    assert!(sleep_after.total >= 9);
    assert!(sleep_after.recorded >= 9);
    assert!(
        app.sleep_wake_last_decision()
            .is_some_and(|d| d.one_line().contains("wake")),
        "expected last decision for wake"
    );

    // When: wake with credentials near expiry
    let now = 1_700_000_000_000i64;
    let near_expiry = harness_core::sleep_wake_auth::CredentialExpirySnapshot {
        expires_at_unix_ms: Some(now + 30_000),
        now_unix_ms: now,
        leeway_ms: harness_core::sleep_wake_auth::DEFAULT_CREDENTIAL_EXPIRY_LEEWAY_MS,
    };
    let refresh_decision = app.apply_sleep_wake_host_event_with_expiry(
        harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake,
        Some(&near_expiry),
    );
    // Then: policy recommends refresh; infrastructure is Active hook strategy
    assert!(refresh_decision.is_refresh());
    assert!(refresh_decision.claims_refresh());
    assert!(
        refresh_decision.one_line().contains("refresh recommended")
            && refresh_decision.one_line().contains("remaining_ms=30000"),
        "expected near-expiry refresh recommendation: {}",
        refresh_decision.one_line()
    );
    let policy_after = app
        .sleep_wake_credential_policy()
        .expect("sleep/wake policy after near-expiry wake");
    assert!(policy_after.is_active());
    assert!(policy_after.one_line().contains("strategy=hook"));

    // Then: jujutsu probe is bound with .jj marker (repo workspace; CLI may be available or not)
    let probe = app.jujutsu_probe().expect("jujutsu probe bound");
    assert!(
        probe.one_line().contains("ready="),
        "expected ready flag in jujutsu one_line: {}",
        probe.one_line()
    );
    assert!(
        probe.workspace.is_repo(),
        "expected .jj marker repo workspace: {}",
        probe.one_line()
    );
    assert!(
        !probe.is_ready() || probe.cli.is_available(),
        "ready only when CLI available: {}",
        probe.one_line()
    );
    let jj_cli = app.jujutsu_cli().expect("jujutsu cli bound");
    assert!(
        jj_cli.one_line().contains("jujutsu") || jj_cli.one_line().contains("jj"),
        "expected jujutsu cli one_line: {}",
        jj_cli.one_line()
    );
    let jj_ws = app.jujutsu_workspace().expect("jujutsu workspace bound");
    assert!(
        jj_ws.is_repo(),
        "expected jujutsu workspace repo: {}",
        jj_ws.one_line()
    );
    // Then: multi-command walk ends on jj status (ok or unavailable; structured)
    let jj_cmd = app
        .jujutsu_last_command()
        .expect("jujutsu last command bound");
    assert!(
        jj_cmd.one_line().contains("jujutsu command:") && jj_cmd.one_line().contains("status"),
        "expected last jujutsu command status: {}",
        jj_cmd.one_line()
    );
    assert!(
        jj_cmd.is_ok() || jj_cmd.is_unavailable(),
        "expected structured ok|unavailable: {}",
        jj_cmd.one_line()
    );

    // Then: COW worktree availability is probed for the workspace root
    let cow = app
        .cow_worktree_availability()
        .expect("cow worktree availability bound");
    assert!(
        cow.one_line().contains("COW worktree fastpath:"),
        "expected COW fastpath one_line: {}",
        cow.one_line()
    );
    // Then: multi-path COW clone batch is bound (src + missing + dest-exists)
    let cow_last = app
        .cow_clone_last_result()
        .expect("cow clone last result bound");
    assert!(
        cow_last.one_line().contains("COW clone:"),
        "expected COW clone last one_line: {}",
        cow_last.one_line()
    );
    assert!(
        cow_last.is_unavailable(),
        "expected last clone dest-exists unavailable: {}",
        cow_last.one_line()
    );
    assert!(
        cow_last.one_line().contains("dst-exists.bin")
            || cow_last.one_line().contains("already exists"),
        "expected dest-exists last path: {}",
        cow_last.one_line()
    );
    let cow_summary = app
        .cow_clone_outcome_summary()
        .expect("cow clone outcome summary bound");
    assert!(
        cow_summary.total >= 5 && cow_summary.unavailable >= 3,
        "expected multi-path COW clone batch: {cow_summary:?}"
    );
    assert_eq!(
        cow_summary.cloned + cow_summary.unavailable,
        cow_summary.total
    );
    assert!(cow_summary.one_line().contains("total"));

    // Then: persistent graph product builds simple index + multi-kind batch
    let graph = app
        .persistent_graph_availability()
        .expect("persistent graph availability bound");
    assert!(
        graph.is_available() || graph.one_line().contains("persistent graph: unavailable"),
        "expected structured persistent graph one_line: {}",
        graph.one_line()
    );
    let batch = app
        .graph_query_batch_summary()
        .expect("multi-kind graph batch summary bound");
    // Multi-symbol multi-kind probe batch (3 symbols × 4 kinds)
    assert!(
        batch.total >= 8,
        "expected multi-symbol multi-kind graph batch: {batch:?}"
    );
    // symbol_def can hit; callers/callees/references stay unavailable on simple index
    assert!(
        batch.unavailable >= 1 || batch.hit_results >= 1,
        "expected structured batch counts: {batch:?}"
    );
    let batch_first = app
        .graph_query_batch_first_line()
        .expect("graph batch first line bound");
    assert!(
        batch_first.contains("symbol_def") && batch_first.contains("(probe)"),
        "expected first batch one_line: {batch_first}"
    );
    let graph_last = app
        .graph_query_last_result()
        .expect("graph query last result bound");
    assert!(
        graph_last.one_line().contains("references")
            || graph_last.one_line().contains("graph query unavailable")
            || graph_last.one_line().contains("graph query hit"),
        "expected last query one_line: {}",
        graph_last.one_line()
    );
    assert!(
        batch.one_line().contains("graph batch:")
            && (batch.one_line().contains("total")
                || batch.one_line().contains("unavailable")
                || batch.one_line().contains("hit")),
        "expected multi-symbol graph batch one_line: {}",
        batch.one_line()
    );

    // Then: Landlock host support is probed (presence ≠ confinement)
    let landlock = app.landlock_support().expect("landlock support bound");
    assert!(
        landlock.one_line().contains("Landlock:"),
        "expected Landlock one_line: {}",
        landlock.one_line()
    );

    // Then: sandbox FS plan is bound after multi-policy walk (last non-Off = Strict; plan-only, not enforcement)
    let sandbox = app
        .sandbox_fs_plan_summary()
        .expect("sandbox fs plan summary bound");
    let os_profiles = app
        .os_sandbox_profiles_summary()
        .expect("os sandbox profiles summary bound");
    assert_eq!(
        os_profiles.total,
        harness_core::sandbox::OS_SANDBOX_POLICIES.len()
    );
    assert_eq!(
        os_profiles.available + os_profiles.unavailable,
        os_profiles.total
    );
    assert!(os_profiles.total >= 4, "expected full OS policy inventory");
    let os_first = app
        .os_sandbox_first_profile_line()
        .expect("os sandbox first profile bound");
    assert!(
        os_first.contains("policy=off") || os_first.contains("OS sandbox profile"),
        "expected first profile (off): {os_first}"
    );
    let sandbox_prep = app
        .sandbox_last_prepare()
        .expect("sandbox last prepare bound");
    assert!(
        sandbox_prep.one_line().contains("sandbox prepare")
            && sandbox_prep.one_line().contains("strict"),
        "expected multi-policy last prepare (strict): {}",
        sandbox_prep.one_line()
    );
    assert_eq!(sandbox.policy, harness_core::sandbox::SandboxPolicy::Strict);
    assert!(sandbox.read_root_count >= 1);
    assert!(sandbox.write_root_count >= 1);
    assert!(
        sandbox.one_line().contains("strict") || sandbox.one_line().contains("read_roots="),
        "expected Strict multi-policy last plan one_line: {}",
        sandbox.one_line()
    );
}
