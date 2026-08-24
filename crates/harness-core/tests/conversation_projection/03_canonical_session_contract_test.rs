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
