use super::{DiscoveryContext, RealFormatterDiscovery};
use crate::coord::formatter::FormatterDiscovery;
use crate::UnwrapOrAbort;

#[tokio::test]
async fn ruff_discovery_requires_config_file() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    std::fs::write(bin_dir.join("ruff"), "#!/bin/sh\n").unwrap_or_abort();
    super::tests::make_executable(&bin_dir.join("ruff"));
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    assert!(
        discovery.resolve("ruff", &context).await.is_none(),
        "ruff absent without config or fallback"
    );
    std::fs::write(workspace.join("ruff.toml"), "[tool.ruff]\n").unwrap();
    let command = discovery.resolve("ruff", &context).await.unwrap_or_abort();
    assert_eq!(command, vec!["ruff", "format", "$FILE"]);

    std::fs::remove_file(workspace.join("ruff.toml")).unwrap();
    std::fs::write(workspace.join("requirements.txt"), "ruff\n").unwrap();
    assert!(
        discovery.resolve("ruff", &context).await.is_some(),
        "requirements fallback enables ruff"
    );
}

#[tokio::test]
async fn ruff_falls_back_when_nearest_pyproject_lacks_ruff_section() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let target = workspace.join("package");
    std::fs::create_dir_all(&target).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    std::fs::write(bin_dir.join("ruff"), "#!/bin/sh\n").unwrap_or_abort();
    super::tests::make_executable(&bin_dir.join("ruff"));
    let _path_guard = prepend_path(&bin_dir);

    std::fs::write(target.join("pyproject.toml"), "[tool.other]\n").unwrap();
    std::fs::write(workspace.join("ruff.toml"), "[tool.ruff]\n").unwrap();

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: target.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    let command = discovery.resolve("ruff", &context).await.unwrap_or_abort();
    assert_eq!(command, vec!["ruff", "format", "$FILE"]);
}

#[tokio::test]
async fn uv_skips_when_ruff_enabled() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    for name in ["ruff", "uv"] {
        let path = bin_dir.join(name);
        std::fs::write(&path, "#!/bin/sh\n").unwrap_or_abort();
        super::tests::make_executable(&path);
    }
    std::fs::write(workspace.join("ruff.toml"), "[tool.ruff]\n").unwrap();
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    assert!(
        discovery.resolve("uv", &context).await.is_none(),
        "uv skips while ruff config exists"
    );
    std::fs::remove_file(workspace.join("ruff.toml")).unwrap();
    let command = discovery.resolve("uv", &context).await.unwrap_or_abort();
    let uv_path = bin_dir.join("uv").to_string_lossy().to_string();
    assert_eq!(
        command,
        vec![uv_path, "format".into(), "--".into(), "$FILE".into()]
    );
}

#[tokio::test]
async fn clang_format_discovery_requires_dot_clang_format() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    std::fs::write(bin_dir.join("clang-format"), "#!/bin/sh\n").unwrap_or_abort();
    super::tests::make_executable(&bin_dir.join("clang-format"));
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    assert!(
        discovery.resolve("clang-format", &context).await.is_none(),
        "clang-format absent without marker"
    );
    std::fs::write(workspace.join(".clang-format"), "\n").unwrap();
    let command = discovery
        .resolve("clang-format", &context)
        .await
        .unwrap_or_abort();
    assert_eq!(command[1..], ["-i", "$FILE"]);
}

#[tokio::test]
async fn ocamlformat_discovery_requires_dot_ocamlformat() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    std::fs::write(bin_dir.join("ocamlformat"), "#!/bin/sh\n").unwrap_or_abort();
    super::tests::make_executable(&bin_dir.join("ocamlformat"));
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    assert!(
        discovery.resolve("ocamlformat", &context).await.is_none(),
        "ocamlformat absent without marker"
    );
    std::fs::write(workspace.join(".ocamlformat"), "\n").unwrap();
    assert!(
        discovery.resolve("ocamlformat", &context).await.is_some(),
        "ocamlformat discovered with marker"
    );
}

#[tokio::test]
async fn air_discovery_requires_help_output() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    let valid = bin_dir.join("air");
    std::fs::write(
        &valid,
        "#!/bin/sh\necho 'air: format R language formatter'\n",
    )
    .unwrap_or_abort();
    super::tests::make_executable(&valid);
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    let command = discovery.resolve("air", &context).await.unwrap_or_abort();
    assert_eq!(
        command,
        vec![
            valid.to_string_lossy().to_string(),
            "format".to_string(),
            "$FILE".to_string(),
        ]
    );

    std::fs::write(&valid, "#!/bin/sh\necho 'air: format language formatter'\n").unwrap();
    assert!(
        discovery.resolve("air", &context).await.is_none(),
        "air absent when help lacks R language"
    );
}

#[tokio::test]
async fn which_only_formatter_resolves_to_path_binary() {
    // arrange
    // act
    // assert
    let _guard = PATH_LOCK.lock().await;
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap_or_abort();
    std::fs::write(bin_dir.join("gofmt"), "#!/bin/sh\n").unwrap_or_abort();
    super::tests::make_executable(&bin_dir.join("gofmt"));
    let _path_guard = prepend_path(&bin_dir);

    let context = DiscoveryContext {
        workspace_root: workspace.clone(),
        target_dir: workspace.clone(),
        experimental_oxfmt: false,
    };
    let discovery = RealFormatterDiscovery;

    let command = discovery.resolve("gofmt", &context).await.unwrap_or_abort();
    assert_eq!(command[1..], ["-w", "$FILE"]);
    assert!(
        command[0].ends_with("gofmt"),
        "gofmt should be resolved to the PATH binary, got {}",
        command[0]
    );
}

struct PathPrefixGuard {
    original: String,
}

impl Drop for PathPrefixGuard {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.original);
    }
}

fn prepend_path(dir: &std::path::Path) -> PathPrefixGuard {
    let original = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", dir.display(), original));
    PathPrefixGuard { original }
}

static PATH_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));
