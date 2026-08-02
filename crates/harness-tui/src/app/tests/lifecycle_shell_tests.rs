use super::*;
use crate::UnwrapOrAbort;

pub(super) fn seed_operator_host_probes_sets_binary_update_and_jujutsu() {
    // Given: live app with no operator host probes bound yet
    let mut app = AppState::new_live(None, false, None);
    assert!(app.binary_update_summary().is_none());
    assert!(app.jujutsu_probe().is_none());

    // When: seed with an explicit workspace root (no PATH dependence on jj)
    let root = std::env::temp_dir().join(format!(
        "harness-tui-seed-probes-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create seed workspace");
    app.seed_operator_host_probes(Some(root.as_path()));
    let bin_ver = app
        .binary_version_info()
        .expect("binary version info bound");
    assert!(
        bin_ver.one_line().contains("harness") || bin_ver.one_line().contains("binary:"),
        "expected binary version: {}",
        bin_ver.one_line()
    );

    // Then: binary update multi-policy offline checks are bound honestly
    let binary = app
        .binary_update_summary()
        .expect("binary update summary bound");
    assert!(
        binary.total >= 5 && binary.checks_unavailable >= 5,
        "expected multi-channel binary update checks: {binary:?}"
    );
    assert!(!binary.update_available);
    assert!(binary.all_unavailable());
    assert!(binary.one_line().contains("update_available=false"));
    let binary_policy = app
        .binary_update_policy()
        .expect("binary update policy bound");
    assert_eq!(
        binary_policy.channel.as_deref(),
        Some("offline"),
        "expected offline channel policy: {binary_policy:?}"
    );
    let binary_check = app
        .binary_update_check()
        .expect("binary update last check bound");
    assert!(binary_check.is_unavailable());
    assert!(
        binary_check.one_line().contains("unavailable")
            || binary_check.one_line().contains("offline")
            || binary_check.one_line().contains("not"),
        "expected unavailable last check: {}",
        binary_check.one_line()
    );

    let attr = app
        .edit_attribution_summary()
        .expect("edit attribution summary bound");
    assert!(
        attr.total >= 3 && attr.agent_tool >= 1 && attr.external >= 1 && attr.drift >= 1,
        "expected multi-path attribution with agent+external+drift: {attr:?}"
    );
    assert!(attr.has_agent_tool());
    assert!(attr.has_external());
    assert!(attr.one_line().contains("agent-tool"));
    assert!(attr.one_line().contains("external"));
    let attr_first = app
        .edit_attribution_first_line()
        .expect("edit attribution first line bound");
    assert!(
        attr_first.contains("source=agent_tool") && attr_first.contains("agent.rs"),
        "expected agent-tool first line: {attr_first}"
    );
    let attr_last = app
        .edit_attribution_last_line()
        .expect("edit attribution last line bound");
    assert!(
        attr_last.contains("source=external")
            && (attr_last.contains("external.rs") || attr_last.contains("drift.rs")),
        "expected external last line: {attr_last}"
    );

    let settings = app.settings_editor_summary();
    assert!(
        settings.bound,
        "expected project config bound: {settings:?}"
    );
    assert_eq!(settings.writable_paths, 6);
    assert_eq!(settings.editable, 6);
    assert!(settings.with_effective_value >= 6);
    assert!(settings.total >= 38);
    assert!(settings.one_line().contains("bound=true"));
    assert!(settings.one_line().contains("writable_paths=6"));
    assert!(
        app.settings_project_config_path()
            .is_some_and(|path| path.ends_with("harness.json")),
        "expected harness.json project config path"
    );
    assert!(app.settings_hashline_edit());
    assert!(app.settings_compaction_enabled());
    assert!(app.settings_compaction_auto_retry_overflow());
    assert!(app.settings_compaction_structured_summary_contract());
    assert!(app.settings_compaction_estimated_token_triggers());
    assert!(!app.settings_deterministic_enabled());
    let settings_path = app
        .settings_project_config_path()
        .expect("settings path bound")
        .to_path_buf();
    assert_eq!(
        harness_core::config::read_effective_hashline_edit(&settings_path).expect("hashline"),
        app.settings_hashline_edit()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_enabled(&settings_path)
            .expect("compaction"),
        app.settings_compaction_enabled()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_auto_retry_overflow(&settings_path)
            .expect("auto_retry"),
        app.settings_compaction_auto_retry_overflow()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_structured_summary_contract(&settings_path)
            .expect("structured_summary"),
        app.settings_compaction_structured_summary_contract()
    );
    assert_eq!(
        harness_core::config::read_effective_compaction_estimated_token_triggers(&settings_path)
            .expect("estimated_token"),
        app.settings_compaction_estimated_token_triggers()
    );
    assert_eq!(
        harness_core::config::read_effective_deterministic_enabled(&settings_path)
            .expect("deterministic"),
        app.settings_deterministic_enabled()
    );

    // Then: write→reset→write product path leaves final effective values bound,
    // and registry definitions/merge strategies are resolvable for all 6 writable paths.
    let registry_json =
        harness_core::config::settings_registry_json().expect("settings registry json");
    assert!(
        registry_json.contains("hashline_edit")
            && registry_json.contains("runtime.compaction.enabled")
            && registry_json.contains("runtime.deterministic.enabled"),
        "expected writable setting ids in registry json"
    );
    let writable_ids = [
        "hashline_edit",
        "runtime.compaction.enabled",
        "runtime.compaction.auto_retry_overflow",
        "runtime.compaction.structured_summary_contract",
        "runtime.compaction.estimated_token_triggers",
        "runtime.deterministic.enabled",
    ];
    let mut editable_defs = 0usize;
    let mut replace_merge = 0usize;
    for setting_id in writable_ids {
        let def = harness_core::config::setting_definition(setting_id)
            .unwrap_or_else(|| panic!("missing setting definition for {setting_id}"));
        assert!(
            def.is_editable(),
            "expected editable writable setting {setting_id}"
        );
        editable_defs += 1;
        if matches!(
            def.merge_strategy,
            harness_core::config::SettingMergeStrategy::Replace
        ) {
            replace_merge += 1;
        }
    }
    assert_eq!(editable_defs, 6);
    assert_eq!(
        replace_merge, 6,
        "expected Replace merge strategy for scalar writable settings"
    );

    // Then: worktree product defaults are metadata-only ReadOnly registry stubs
    for setting_id in ["worktree.relative_base", "worktree.branch_prefix"] {
        assert!(
            harness_core::config::is_metadata_only_setting(setting_id),
            "expected metadata-only worktree setting {setting_id}"
        );
        let def = harness_core::config::setting_definition(setting_id)
            .unwrap_or_else(|| panic!("missing worktree setting definition for {setting_id}"));
        assert!(
            !def.is_editable(),
            "expected read-only metadata worktree setting {setting_id}"
        );
        assert!(
            def.has_default(),
            "expected default for metadata worktree setting {setting_id}"
        );
        assert!(
            matches!(
                def.merge_strategy,
                harness_core::config::SettingMergeStrategy::Replace
            ),
            "expected Replace merge for {setting_id}"
        );
    }
    assert!(
        registry_json.contains("worktree.relative_base")
            && registry_json.contains("worktree.branch_prefix"),
        "expected worktree metadata ids in registry json"
    );

    let settings_registry = app
        .settings_registry_summary()
        .expect("settings registry summary bound");
    assert!(settings_registry.total >= 38);
    assert!(settings_registry.runtime > 0);
    assert!(settings_registry.tui > 0);
    assert!(settings_registry.editable > 0);
    assert!(settings_registry.read_only > 0);
    assert!(settings_registry.secret > 0);
    assert!(settings_registry.with_default > 0);
    assert!(
        settings_registry.metadata_only >= 2,
        "expected worktree metadata-only stubs in registry: {settings_registry:?}"
    );
    assert_eq!(
        settings_registry.editable + settings_registry.read_only,
        settings_registry.total
    );
    assert!(settings_registry
        .one_line()
        .starts_with("settings registry: "));
    assert!(settings_registry.one_line().contains("runtime="));
    assert!(settings_registry.one_line().contains("tui="));

    let plan_summary = app.plan_view_summary();
    assert!(
        plan_summary.total >= 5,
        "expected multi-plan seed with active-run plan: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.existing >= 5,
        "expected existing plan files: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.active >= 1,
        "expected active-run plan binding: {:?}",
        plan_summary
    );
    assert!(
        plan_summary.total_bytes > 0,
        "expected plan bytes: {:?}",
        plan_summary
    );
    assert!(plan_summary.one_line().starts_with("plan view: "));
    assert!(plan_summary.one_line().contains("existing="));
    assert!(plan_summary.one_line().contains("active="));
    assert_eq!(app.run_id(), Some("harness-probe-run"));
    let plan_rows = app.plan_view_rows();
    assert!(
        plan_rows.len() >= 5,
        "expected multi-plan rows: {}",
        plan_rows.len()
    );
    assert!(
        plan_rows
            .iter()
            .any(|row| { row.slug.contains("harness-probe-plan") && row.exists }),
        "expected probe plan row: {:?}",
        plan_rows
            .iter()
            .map(|row| row.one_line())
            .collect::<Vec<_>>()
    );
    assert!(
        plan_rows
            .iter()
            .any(|row| row.slug.contains("harness-probe-run") && row.exists && row.is_active),
        "expected active-run plan row: {:?}",
        plan_rows
            .iter()
            .map(|row| row.one_line())
            .collect::<Vec<_>>()
    );

    // Then: multi-report crash scan is seeded under workspace/.harness-sessions-probe
    let crash = app
        .crash_recovery_scan_summary()
        .expect("crash recovery scan summary bound");
    assert!(
        crash.scanned >= 5,
        "expected multi-report crash probe fixtures: {crash:?}"
    );
    assert!(
        crash.previous_crash >= 1 && crash.clean >= 1,
        "expected previous-crash + clean mix: {crash:?}"
    );
    assert!(crash.one_line().contains("previous-crash"));
    let crash_first = app
        .crash_recovery_first_report()
        .expect("crash recovery first report bound");
    assert!(crash_first.previous_crash_detected);
    assert!(!crash_first.events_log_present);
    let crash_action = app
        .crash_recovery_resolved_action()
        .expect("crash recovery resolved action bound");
    assert_eq!(crash_action.as_str(), "reopen_session");

    // Then: offline mock ACP connect+bind success path is seeded honestly
    let acp = app
        .acp_connection_summary()
        .expect("acp connection summary bound");
    let acp_connect = app.acp_last_connect().expect("acp last connect bound");
    assert!(
        acp_connect.one_line().contains("ok") || acp_connect.is_connected(),
        "expected mock ACP connect ok: {}",
        acp_connect.one_line()
    );
    let acp_bind = app.acp_last_bind().expect("acp last bind bound");
    assert!(
        acp_bind.one_line().contains("ok") && acp_bind.one_line().contains("harness.probe.agent"),
        "expected mock ACP bind ok: {}",
        acp_bind.one_line()
    );
    assert!(acp.is_bound());
    assert!(
        acp.one_line().contains("harness.probe.agent")
            || acp.agent_name.as_deref() == Some("harness.probe.agent"),
        "expected bound ACP agent: {}",
        acp.one_line()
    );
    let acp_session = app.acp_session_info().expect("acp session info bound");
    assert_eq!(acp_session.agent_name, "harness.probe.agent");
    assert!(!acp_session.session_id.is_empty());
    let fallback = app
        .auto_fallback_summary()
        .expect("auto fallback summary bound");
    // Full chain walk: primary → fb1 → fb2 → fb3 → fb4 → Exhausted (remaining=0).
    assert_eq!(fallback.remaining, 0);
    assert!(
        fallback.chain_len >= 5,
        "expected longer multi-fallback chain: {fallback:?}"
    );
    assert!(fallback.exhausted);
    let fallback_outcome = app
        .auto_fallback_last_outcome()
        .expect("auto fallback last outcome bound");
    assert!(
        fallback_outcome.is_exhausted(),
        "expected Exhausted after full chain walk: {}",
        harness_core::auto_fallback::describe_auto_fallback_outcome(&fallback_outcome)
    );
    let banner = app
        .auto_fallback_last_banner()
        .expect("auto fallback last banner bound");
    assert!(
        banner.contains("exhausted") && banner.contains("(probe):fb2"),
        "expected exhausted banner after fb2: {banner}"
    );
    let models = app
        .auto_fallback_chain_label()
        .expect("auto fallback chain label bound");
    assert!(
        models.contains("(probe):primary")
            && models.contains("(probe):fb1")
            && models.contains("(probe):fb2"),
        "expected full probe chain label: {models}"
    );
    let plugins = app
        .plugin_lifecycle_summary()
        .expect("plugin lifecycle summary bound");
    assert!(
        plugins.installed >= 2 && plugins.enabled >= 1 && plugins.disabled >= 1,
        "expected multi-plugin lifecycle installed/enabled/disabled: {plugins:?}"
    );
    let plugin_install = app
        .plugin_last_install()
        .expect("plugin last install bound");
    assert!(
        plugin_install.one_line().contains("plugin install: ok"),
        "expected successful probe install: {}",
        plugin_install.one_line()
    );
    assert!(
        plugin_install.one_line().contains("harness.probe.plugin"),
        "expected probe plugin id (primary or secondary): {}",
        plugin_install.one_line()
    );
    let plugin_activate = app
        .plugin_last_activate()
        .expect("plugin last activate bound");
    assert!(
        plugin_activate.one_line().contains("plugin activate: ok"),
        "expected successful probe activate: {}",
        plugin_activate.one_line()
    );
    let plugin_deactivate = app
        .plugin_last_deactivate()
        .expect("plugin last deactivate bound");
    assert!(
        plugin_deactivate
            .one_line()
            .contains("plugin deactivate: ok"),
        "expected successful probe deactivate: {}",
        plugin_deactivate.one_line()
    );
    let plugin_remove = app.plugin_last_remove().expect("plugin last remove bound");
    assert!(
        plugin_remove.one_line().contains("plugin remove: failed"),
        "missing-remove-probe should fail closed: {}",
        plugin_remove.one_line()
    );
    let plugin_first = app.plugin_first_line().expect("plugin first line bound");
    assert!(
        plugin_first.contains("harness.probe.plugin"),
        "expected first plugin line: {plugin_first}"
    );
    assert!(
        plugin_first.contains("enablement=enabled") || plugin_first.contains("enablement=disabled"),
        "expected enablement state on multi-plugin first line: {plugin_first}"
    );
    assert!(
        plugin_deactivate
            .one_line()
            .contains("harness.probe.plugin.secondary")
            || plugin_deactivate.one_line().contains("secondary"),
        "expected secondary deactivate last: {}",
        plugin_deactivate.one_line()
    );

    // Then: multi-descriptor discover + primary probe loaded (descriptor-only; no code load)
    let discover = app
        .extension_discover_summary()
        .expect("extension discover summary bound");
    assert!(
        discover.discovered >= 3,
        "expected multi-descriptor discover (primary+alt+tools[+plugin]): discovered={}",
        discover.discovered
    );
    assert!(!discover.loads_external_code);
    let summary = app
        .extension_manifest_summary()
        .expect("extension manifest summary bound");
    assert_eq!(summary.extension_id, "harness.probe.extension");
    assert!(
        summary.one_line().contains("harness.probe.extension")
            || summary.one_line().contains("extension descriptor:"),
        "expected probe descriptor one_line: {}",
        summary.one_line()
    );
    assert!(
        summary.capabilities >= 1 && summary.enabled_capabilities >= 1,
        "expected primary probe capability counts: caps={} enabled={}",
        summary.capabilities,
        summary.enabled_capabilities
    );
    assert!(
        summary.tools >= 1,
        "expected primary probe tool count: tools={}",
        summary.tools
    );
    assert!(!summary.loads_external_code);
    let load = app
        .extension_last_load()
        .expect("extension last load bound");
    assert!(
        load.one_line().contains("ok") && load.one_line().contains("harness.probe.extension"),
        "expected Loaded primary probe load: {}",
        load.one_line()
    );

    super::lifecycle_shell_part3_test::seed_operator_host_probes_sets_binary_update_and_jujutsu_continuation(&mut app);

    let _ = std::fs::remove_dir_all(&root);
}

pub(super) fn seed_operator_host_probes_binds_crash_scan_and_foreign_discover() {
    // Given: isolated sessions root with one clean run and one previous-crash run
    let root = std::env::temp_dir().join(format!(
        "harness-tui-seed-session-probes-{}-{}",
        std::process::id(),
        "sessions"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create sessions root");

    let clean = root.join("run_clean");
    std::fs::create_dir_all(&clean).expect("create clean run");
    std::fs::write(clean.join("events.jsonl"), b"").expect("events");

    let crashed = root.join("run_crashed");
    std::fs::create_dir_all(&crashed).expect("create crashed run");
    std::fs::write(crashed.join(".writer.lock.recovering"), b"").expect("recovery marker");

    // Given: foreign scan root with one importable events.jsonl candidate
    let foreign_root = std::env::temp_dir().join(format!(
        "harness-tui-seed-foreign-{}-{}",
        std::process::id(),
        "scan"
    ));
    let _ = std::fs::remove_dir_all(&foreign_root);
    let foreign_session = foreign_root.join("foreign_events");
    std::fs::create_dir_all(&foreign_session).expect("create foreign session");
    std::fs::write(
        foreign_session.join("events.jsonl"),
        br#"{"schema_version":1,"event_id":"evt_foreign_1","seq":1,"run_id":"run_foreign","mono_ms":1,"actor":{"kind":"system"},"payload":{"event_type":"run_finished","data":{"summary":"imported"}}}
"#,
    )
    .expect("foreign events marker");

    let mut app = AppState::new_live(None, false, None);
    assert!(app.crash_recovery_scan_summary().is_none());
    assert!(app.foreign_discover_summary().is_none());

    // When: seed with explicit sessions + foreign roots
    app.seed_operator_host_probes_with_roots(
        Some(root.as_path()),
        Some(root.as_path()),
        Some(foreign_root.as_path()),
    );

    // Then: crash scan summary reflects multi-report root (test fixtures + probe fixtures)
    let crash = app
        .crash_recovery_scan_summary()
        .expect("crash recovery scan summary bound");
    assert!(
        crash.scanned >= 3,
        "expected multi-report scan (clean+crashed+stale[+test fixtures]): {crash:?}"
    );
    assert!(
        crash.previous_crash >= 1,
        "expected previous-crash count: {crash:?}"
    );
    assert!(crash.clean >= 1, "expected clean count: {crash:?}");
    assert!(crash.one_line().contains("previous-crash"));
    let crash_action = app
        .crash_recovery_resolved_action()
        .expect("crash recovery resolved action bound from first report");
    assert_eq!(
        crash_action.as_str(),
        "reopen_session",
        "crashed fixture has no events.jsonl → not resumable"
    );
    let run_id = crash_action.operator_hint("run_crashed");
    assert!(
        run_id.contains("run_crashed"),
        "operator hint should carry run id: {run_id}"
    );
    let crash_first = app
        .crash_recovery_first_report()
        .expect("crash recovery first report bound");
    assert!(crash_first.previous_crash_detected);
    assert!(
        crash_first.recovery_marker_present || crash_first.stale_writer_lock,
        "expected recovery marker or stale lock: {crash_first:?}"
    );
    assert!(!crash_first.events_log_present);
    assert!(
        crash_first
            .run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name == "run_crashed"
                    || name == "harness_probe_crashed"
                    || name == "harness_probe_stale"
            }),
        "first crash report should be a previous-crash fixture: {:?}",
        crash_first.run_dir
    );

    // Then: foreign discover summary sees multi-source importable markers
    let foreign = app
        .foreign_discover_summary()
        .expect("foreign discover summary bound");
    assert!(
        foreign.total >= 3,
        "expected multi-source foreign discover: {foreign:?}"
    );
    assert!(
        foreign.discoverable >= 3 && foreign.importable >= 3,
        "expected multi-source importable: {foreign:?}"
    );
    assert!(foreign.has_importable());
    assert!(foreign.one_line().contains("importable"));
    // Then: first importable candidate is imported into probe dest (Imported outcome)
    let first = app
        .foreign_import_first_candidate()
        .expect("foreign import first candidate bound");
    assert!(first.is_importable(), "expected importable first candidate");
    let import_last = app
        .foreign_import_last_outcome()
        .expect("foreign import last outcome bound");
    assert!(
        import_last.one_line().contains("foreign import:")
            && (import_last.one_line().contains("ok")
                || import_last.one_line().contains("imported")
                || import_last.one_line().contains("run=")),
        "expected successful import one_line: {}",
        import_last.one_line()
    );

    // Then: binary + jujutsu + sandbox still seeded
    assert!(app.binary_update_summary().is_some());
    assert!(app.jujutsu_probe().is_some());
    let sandbox = app
        .sandbox_fs_plan_summary()
        .expect("sandbox fs plan summary bound");
    assert_eq!(
        sandbox.policy,
        harness_core::sandbox::SandboxPolicy::Strict,
        "multi-policy FS plan walk binds last non-Off plan (Strict)"
    );
    assert!(sandbox.one_line().contains("read_roots="));
    assert!(sandbox.one_line().contains("write_roots="));

    let _ = std::fs::remove_dir_all(&root);
}
