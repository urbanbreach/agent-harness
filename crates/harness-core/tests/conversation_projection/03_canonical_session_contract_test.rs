use std::path::Path;

#[test]
fn canonical_session_type_contract() {
    // arrange
    let ids_source = include_str!("../../src/ids.rs");
    let lib_source = include_str!("../../src/lib.rs");

    // act
    let required_declarations = [
        "id_newtype!(SessionId);",
        "id_newtype!(EntryId);",
        "id_newtype!(TurnId);",
        "id_newtype!(RunId);",
        "id_newtype!(ProviderRequestId);",
        "id_newtype!(ToolCallId);",
    ];

    // assert
    for declaration in required_declarations {
        assert!(
            ids_source.contains(declaration),
            "missing distinct canonical identity declaration `{declaration}`"
        );
    }
    assert!(
        lib_source.contains("pub mod session;"),
        "missing public canonical session domain"
    );
}

#[test]
fn canonical_active_path_contract() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    // act
    let required_modules = [
        source_root.join("session.rs"),
        source_root.join("session/model.rs"),
        source_root.join("session/reducer.rs"),
        source_root.join("session/legacy.rs"),
    ];

    // assert
    for module in required_modules {
        assert!(
            module.is_file(),
            "missing canonical active-path module `{}`",
            module.display()
        );
    }
}

#[test]
fn canonical_projection_owns_core_durable_consumers() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let projection_path = source_root.join("session/projection.rs");
    let session_facade =
        std::fs::read_to_string(source_root.join("session.rs")).expect("read session facade");
    let committed_context = std::fs::read_to_string(
        source_root.join("coord/provider_context/committed.rs"),
    )
    .expect("read committed provider context");

    // act
    let projection_exists = projection_path.is_file();
    let projection_is_exported = session_facade.contains("mod projection;")
        && session_facade.contains("CanonicalSessionProjection");
    let committed_context_is_canonical =
        committed_context.contains("CanonicalSessionProjection")
            && !committed_context.contains("project_conversation");

    // assert
    assert!(
        projection_exists,
        "canonical durable projection must live at `{}`",
        projection_path.display()
    );
    assert!(
        projection_is_exported,
        "session facade must export CanonicalSessionProjection"
    );
    assert!(
        committed_context_is_canonical,
        "live provider context must consume CanonicalSessionProjection instead of project_conversation"
    );
}

#[test]
fn legacy_decoder_isolated_from_active_runtime_consumers() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let restore = std::fs::read_to_string(
        source_root.join("coord/provider_context/restore.rs"),
    )
    .expect("read provider restore");
    let compaction = std::fs::read_to_string(
        source_root.join("coord/session_compaction/typed_preparation.rs"),
    )
    .expect("read typed compaction preparation");
    let legacy_compaction =
        std::fs::read_to_string(source_root.join("session/legacy/compaction.rs"))
            .expect("read legacy compaction decoder");

    // act
    let active_runtime_imports_legacy_adapter =
        restore.contains("LegacyEventLogAdapter")
            || compaction.contains("LegacyEventLogAdapter");
    let legacy_checkpoint_runtime_remains = legacy_compaction.contains("load_checkpoint")
        || legacy_compaction.contains("discover_applied_checkpoints")
        || legacy_compaction.contains("checkpoint_artifact");

    // assert
    assert!(
        !active_runtime_imports_legacy_adapter,
        "active runtime consumers must enter through CanonicalSessionProjection"
    );
    assert!(
        !legacy_checkpoint_runtime_remains,
        "legacy compatibility may decode events but must not load checkpoint artifacts"
    );
}

#[test]
fn one_provider_context_builder_and_compaction_writer_remain() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let provider_context = std::fs::read_to_string(
        source_root.join("coord/provider_context.rs"),
    )
    .expect("read provider context owner");
    let committed = std::fs::read_to_string(
        source_root.join("coord/provider_context/committed.rs"),
    )
    .expect("read committed context adapter");
    let recovery = std::fs::read_to_string(
        source_root.join("coord/provider_context/restore/lower.rs"),
    )
    .expect("read recovery context adapter");
    let compaction = std::fs::read_to_string(
        source_root.join("coord/session_compaction/pipeline.rs"),
    )
    .expect("read compaction pipeline");

    // act
    let context_builder_count = [&provider_context, &committed, &recovery]
        .iter()
        .map(|source| source.matches("checkpoint: None,").count())
        .sum::<usize>();
    let compaction_writer_count = compaction
        .matches("EventV1::SessionCompaction(SessionCompactionEvent")
        .count();

    // assert
    assert_eq!(
        context_builder_count, 1,
        "provider context construction must have one production owner"
    );
    assert_eq!(
        compaction_writer_count, 1,
        "Compaction V2 must have exactly one success writer"
    );
}

#[test]
fn compaction_boundary_has_no_active_legacy_module() {
    // arrange
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let cut_point =
        std::fs::read_to_string(source_root.join("coord/compaction/cut_point.rs"))
            .expect("read compaction cut-point boundary");
    let session_projection =
        std::fs::read_to_string(source_root.join("session/projection.rs"))
            .expect("read canonical session projection");

    // act
    let active_legacy_module = cut_point.contains("mod legacy;");
    let session_depends_on_coordinator = session_projection.contains("crate::coord");

    // assert
    assert!(
        !active_legacy_module,
        "active Compaction V2 boundaries must not route through a legacy-named module"
    );
    assert!(
        !session_depends_on_coordinator,
        "pure session projection must not depend on coordinator authority"
    );
}
