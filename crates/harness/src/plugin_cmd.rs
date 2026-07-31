//! Product CLI for the plugin runtime lifecycle (`harness plugin`).
//!
//! Surfaces install/activate/deactivate/remove/upgrade/list over a durable
//! [`harness_core::integrations::PluginLifecycleRegistry`] persisted at
//! `<workspace>/.agent-harness/plugins.json`, so a plugin installed in one
//! invocation is visible to the next. `activate` is an explicit operator action:
//! invoking the command is the permission grant (the registry never auto-grants).
//! `discover` scans the workspace for extension manifests and registers them in
//! the durable [`harness_core::extension_registry::ExtensionDescriptorRegistry`]
//! at `<workspace>/.agent-harness/extension-registry.json`.
//! No `.so`/wasm execution, marketplace, or remote install is performed.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::extension_registry::ExtensionDescriptorRegistry;
use harness_core::integrations::{
    InstalledPlugin, PluginActivationPermission, PluginLifecycleSummary, PluginRuntimeContract,
};
use serde::Serialize;

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct PluginCommand {
    #[command(subcommand)]
    command: PluginSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum PluginSubcommand {
    /// Install a plugin package by validating its descriptor under the workspace (JSON result).
    Install(InstallArgs),
    /// Activate an installed plugin (operator permission grant; loads package entries) (JSON result).
    Activate(IdArgs),
    /// Deactivate an enabled plugin without removing its registration (JSON result).
    Deactivate(IdArgs),
    /// Remove a disabled plugin registration (JSON result).
    Remove(IdArgs),
    /// Upgrade an installed plugin by re-validating a replacement package root (JSON result).
    Upgrade(UpgradeArgs),
    /// List installed plugins with a lifecycle summary (JSON result).
    List(ListArgs),
    /// Discover extension manifests under the workspace and register them in the descriptor registry (JSON result).
    Discover(ListArgs),
}

#[derive(Debug, Args, Clone)]
struct InstallArgs {
    /// Plugin package root (directory containing extension.manifest.json).
    package_root: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct IdArgs {
    /// Installed plugin id.
    id: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct UpgradeArgs {
    /// Installed plugin id to upgrade.
    id: String,
    /// Replacement package root (directory containing extension.manifest.json).
    package_root: String,
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct ListArgs {
    /// Workspace root (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

pub(crate) fn execute_with_io(command: PluginCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        PluginSubcommand::Install(args) => run_install(args, io, deps),
        PluginSubcommand::Activate(args) => run_activate(args, io, deps),
        PluginSubcommand::Deactivate(args) => run_deactivate(args, io, deps),
        PluginSubcommand::Remove(args) => run_remove(args, io, deps),
        PluginSubcommand::Upgrade(args) => run_upgrade(args, io, deps),
        PluginSubcommand::List(args) => run_list(args, io, deps),
        PluginSubcommand::Discover(args) => run_discover(args, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn open(root: &std::path::Path, io: &mut CliIo<'_>) -> Result<PluginRuntimeContract, i32> {
    PluginRuntimeContract::open(root).map_err(|err| map_plugin_error(io, err))
}

fn run_install(args: InstallArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    match registry.install_from_package_root(args.package_root) {
        Ok(plugin) => {
            let plugin = plugin.clone();
            write_json(io, &single_view(&root, &registry, &plugin))
        }
        Err(err) => map_plugin_error(io, err),
    }
}

fn run_activate(args: IdArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    match registry.activate(&args.id, PluginActivationPermission::Granted) {
        Ok(plugin) => {
            let plugin = plugin.clone();
            write_json(io, &single_view(&root, &registry, &plugin))
        }
        Err(err) => map_plugin_error(io, err),
    }
}

fn run_deactivate(args: IdArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    match registry.deactivate(&args.id) {
        Ok(plugin) => {
            let plugin = plugin.clone();
            write_json(io, &single_view(&root, &registry, &plugin))
        }
        Err(err) => map_plugin_error(io, err),
    }
}

fn run_remove(args: IdArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    match registry.remove(&args.id) {
        Ok(removed) => write_json(
            io,
            &RemoveJson {
                workspace_root: root.display().to_string(),
                registry_path: registry_path_string(&registry),
                removed: PluginView::from(&removed),
            },
        ),
        Err(err) => map_plugin_error(io, err),
    }
}

fn run_upgrade(args: UpgradeArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    let previous = registry.get(&args.id).map(|plugin| {
        (
            plugin.manifest.version.clone(),
            plugin.package_root.display().to_string(),
        )
    });
    match registry.upgrade_plugin(
        &args.id,
        &args.package_root,
        PluginActivationPermission::Granted,
    ) {
        Ok(plugin) => {
            let plugin = plugin.clone();
            let (previous_version, previous_package_root) = previous.unzip();
            write_json(
                io,
                &UpgradeJson {
                    workspace_root: root.display().to_string(),
                    registry_path: registry_path_string(&registry),
                    previous_version: previous_version.flatten(),
                    previous_package_root,
                    version: plugin.manifest.version.clone(),
                    plugin: PluginView::from(&plugin),
                },
            )
        }
        Err(err) => map_plugin_error(io, err),
    }
}

fn run_list(args: ListArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let registry = match open(&root, io) {
        Ok(registry) => registry,
        Err(code) => return code,
    };
    let summary = registry.summary();
    let plugins: Vec<PluginView> = registry.list().map(PluginView::from).collect();
    let count = plugins.len();
    write_json(
        io,
        &ListJson {
            workspace_root: root.display().to_string(),
            registry_path: registry_path_string(&registry),
            summary,
            count,
            plugins,
        },
    )
}

fn run_discover(args: ListArgs, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&args.workspace, deps);
    let mut descriptor_registry = match ExtensionDescriptorRegistry::open(&root) {
        Ok(registry) => registry,
        Err(err) => {
            let _ = writeln!(io.stderr, "plugin discover: {err}");
            return 1;
        }
    };
    let discover = match descriptor_registry.discover_and_register(&root) {
        Ok(summary) => summary,
        Err(err) => {
            let _ = writeln!(io.stderr, "plugin discover: {err}");
            return 1;
        }
    };
    let registry_summary = descriptor_registry.summary();
    let entries: Vec<DescriptorView> = descriptor_registry
        .list()
        .into_iter()
        .map(DescriptorView::from)
        .collect();
    let count = entries.len();
    write_json(
        io,
        &DiscoverJson {
            workspace_root: root.display().to_string(),
            registry_path: descriptor_registry.registry_path().display().to_string(),
            discovered: discover.discovered,
            loads_external_code: discover.loads_external_code,
            registered: registry_summary.registered,
            count,
            descriptors: entries,
        },
    )
}

fn single_view(
    root: &std::path::Path,
    contract: &PluginRuntimeContract,
    plugin: &InstalledPlugin,
) -> SingleJson {
    SingleJson {
        workspace_root: root.display().to_string(),
        registry_path: registry_path_string(contract),
        plugin: PluginView::from(plugin),
    }
}

fn registry_path_string(contract: &PluginRuntimeContract) -> Option<String> {
    contract
        .registry()
        .registry_path()
        .map(|path| path.display().to_string())
}

fn map_plugin_error(io: &mut CliIo<'_>, err: impl std::fmt::Display) -> i32 {
    let _ = writeln!(io.stderr, "plugin: {err}");
    1
}

#[derive(Debug, Serialize)]
struct PluginView {
    id: String,
    package_root: String,
    enablement: String,
    loads_code: bool,
}

impl From<&InstalledPlugin> for PluginView {
    fn from(plugin: &InstalledPlugin) -> Self {
        Self {
            id: plugin.id.clone(),
            package_root: plugin.package_root.display().to_string(),
            enablement: plugin.enablement.as_str().to_string(),
            loads_code: plugin.loads_code(),
        }
    }
}

#[derive(Debug, Serialize)]
struct SingleJson {
    workspace_root: String,
    registry_path: Option<String>,
    plugin: PluginView,
}

#[derive(Debug, Serialize)]
struct RemoveJson {
    workspace_root: String,
    registry_path: Option<String>,
    removed: PluginView,
}

#[derive(Debug, Serialize)]
struct UpgradeJson {
    workspace_root: String,
    registry_path: Option<String>,
    previous_version: Option<String>,
    previous_package_root: Option<String>,
    version: Option<String>,
    plugin: PluginView,
}

#[derive(Debug, Serialize)]
struct ListJson {
    workspace_root: String,
    registry_path: Option<String>,
    summary: PluginLifecycleSummary,
    count: usize,
    plugins: Vec<PluginView>,
}

#[derive(Debug, Serialize)]
struct DescriptorView {
    extension_id: String,
    manifest_path: String,
    capabilities: usize,
    enabled_capabilities: usize,
    tools: usize,
    hooks: usize,
    loads_external_code: bool,
}

impl From<&harness_core::extension_registry::ExtensionRegistryEntry> for DescriptorView {
    fn from(entry: &harness_core::extension_registry::ExtensionRegistryEntry) -> Self {
        Self {
            extension_id: entry.extension_id.clone(),
            manifest_path: entry.manifest_path.clone(),
            capabilities: entry.capabilities,
            enabled_capabilities: entry.enabled_capabilities,
            tools: entry.tools,
            hooks: entry.hooks,
            loads_external_code: entry.loads_external_code,
        }
    }
}

#[derive(Debug, Serialize)]
struct DiscoverJson {
    workspace_root: String,
    registry_path: String,
    discovered: usize,
    loads_external_code: bool,
    registered: usize,
    count: usize,
    descriptors: Vec<DescriptorView>,
}

fn write_json(io: &mut CliIo<'_>, value: &impl Serialize) -> i32 {
    match serde_json::to_string_pretty(value) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "plugin: failed to serialize JSON: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use harness_core::extension_manifest::EXTENSION_MANIFEST_V1_SCHEMA_VERSION;
    use harness_core::integrations::PLUGIN_MANIFEST_FILE_NAME;
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use tempfile::tempdir;

    fn run_cli(workspace: &Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(workspace.to_path_buf());
        let mut argv: Vec<String> = vec!["harness".to_string(), "plugin".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    fn seed_plugin_package(workspace: &Path, dir_name: &str, id: &str) -> PathBuf {
        seed_plugin_package_version(workspace, dir_name, id, "0.1.0")
    }

    fn seed_plugin_package_version(
        workspace: &Path,
        dir_name: &str,
        id: &str,
        version: &str,
    ) -> PathBuf {
        let package = workspace.join(dir_name);
        fs::create_dir_all(&package).expect("create package dir");
        let manifest = serde_json::json!({
            "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            "id": id,
            "displayName": "CLI test plugin",
            "version": version,
            "capabilities": [
                {"id": "cap.demo", "defaultEnabled": true}
            ]
        });
        fs::write(
            package.join(PLUGIN_MANIFEST_FILE_NAME),
            manifest.to_string(),
        )
        .expect("write manifest");
        package
    }

    #[test]
    fn full_lifecycle_across_separate_cli_invocations_persists_to_disk() {
        // arrange — a workspace owning a valid descriptor package and the durable journal
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let package = seed_plugin_package(ws, "plugins/demo", "demo.plugin");
        let journal = ws.join(".agent-harness/plugins.json");

        // act + assert — install persists a disabled registration
        let (code, stdout, stderr) = run_cli(ws, &["install", package.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"id\": \"demo.plugin\""),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"enablement\": \"disabled\""),
            "stdout: {stdout}"
        );
        assert!(
            journal.is_file(),
            "durable journal must persist after install"
        );

        // act + assert — a separate invocation activates the persisted plugin
        let (code, stdout, stderr) = run_cli(ws, &["activate", "demo.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"enablement\": \"enabled\""),
            "stdout: {stdout}"
        );

        // act + assert — list (another invocation) sees the enabled plugin durably
        let (code, stdout, stderr) = run_cli(ws, &["list"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
        assert!(stdout.contains("\"installed\": 1"), "stdout: {stdout}");
        assert!(stdout.contains("\"enabled\": 1"), "stdout: {stdout}");

        // act + assert — deactivate then remove across invocations
        let (code, stdout, stderr) = run_cli(ws, &["deactivate", "demo.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"enablement\": \"disabled\""),
            "stdout: {stdout}"
        );
        let (code, stdout, stderr) = run_cli(ws, &["remove", "demo.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"id\": \"demo.plugin\""),
            "stdout: {stdout}"
        );

        // assert — a final list confirms the removal persisted
        let (code, stdout, stderr) = run_cli(ws, &["list"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 0"), "stdout: {stdout}");
    }

    #[test]
    fn install_missing_manifest_fails_closed_without_journal_corruption() {
        // arrange — a package directory with no manifest
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let package = ws.join("plugins/empty");
        fs::create_dir_all(&package).unwrap();

        // act
        let (code, _, stderr) = run_cli(ws, &["install", package.to_str().unwrap()]);

        // assert — fail closed with a plugin error
        assert_eq!(code, 1);
        assert!(stderr.contains("plugin:"), "stderr: {stderr}");
    }

    #[test]
    fn remove_while_enabled_fails_closed() {
        // arrange — an activated plugin
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let package = seed_plugin_package(ws, "plugins/locked", "locked.plugin");
        let (code, _, stderr) = run_cli(ws, &["install", package.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let (code, _, stderr) = run_cli(ws, &["activate", "locked.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — attempt to remove while enabled
        let (code, _, stderr) = run_cli(ws, &["remove", "locked.plugin"]);

        // assert — fail closed; the plugin stays installed and enabled
        assert_eq!(code, 1);
        assert!(
            stderr.contains("deactivate before remove"),
            "stderr: {stderr}"
        );
        let (code, stdout, _) = run_cli(ws, &["list"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("\"enabled\": 1"), "stdout: {stdout}");
    }

    #[test]
    fn activate_unknown_id_fails_closed() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act — activate a plugin that was never installed
        let (code, _, stderr) = run_cli(ws, &["activate", "ghost.plugin"]);

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("not installed"), "stderr: {stderr}");
    }

    #[test]
    fn explicit_workspace_isolates_registry_location() {
        // arrange — an explicit workspace separate from the CLI cwd, with the
        // package rooted inside it (install validates paths under the workspace)
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let explicit = ws.join("explicit-ws");
        fs::create_dir_all(&explicit).unwrap();
        let package = seed_plugin_package(&explicit, "plugins/iso", "iso.plugin");

        // act — install into the explicit workspace
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "install",
                package.to_str().unwrap(),
                "--workspace",
                explicit.to_str().unwrap(),
            ],
        );

        // assert — journal lands in the explicit workspace, not the cwd
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"id\": \"iso.plugin\""),
            "stdout: {stdout}"
        );
        assert!(
            explicit.join(".agent-harness/plugins.json").is_file(),
            "journal must exist in explicit workspace"
        );
        assert!(
            !ws.join(".agent-harness/plugins.json").is_file(),
            "journal must not leak into unrelated cwd"
        );
    }

    #[test]
    fn discover_finds_extension_manifests_and_persists_descriptor_registry() {
        // arrange — a workspace with a valid extension manifest one level deep
        let dir = tempdir().unwrap();
        let ws = dir.path();
        seed_plugin_package(ws, "demo", "demo.discover");

        // act — discover scans the workspace and registers descriptors
        let (code, stdout, stderr) = run_cli(ws, &["discover"]);

        // assert — descriptor found, persisted, and reported
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"extension_id\": \"demo.discover\""),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("\"discovered\": 1"), "stdout: {stdout}");
        assert!(
            ws.join(".agent-harness/extension-registry.json").is_file(),
            "descriptor registry must persist after discover"
        );

        // act — a second discover is idempotent (upsert by id)
        let (code, stdout, stderr) = run_cli(ws, &["discover"]);

        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
    }

    #[test]
    fn discover_on_empty_workspace_reports_zero() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["discover"]);

        // assert
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"discovered\": 0"), "stdout: {stdout}");
        assert!(stdout.contains("\"count\": 0"), "stdout: {stdout}");
    }

    #[test]
    fn upgrade_replaces_package_and_persists_across_invocations() {
        // arrange — install + activate a v1 package in one workspace
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let v1 = seed_plugin_package_version(ws, "plugins/demo-v1", "demo.plugin", "0.1.0");
        let (code, _, stderr) = run_cli(ws, &["install", v1.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let (code, _, stderr) = run_cli(ws, &["activate", "demo.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — upgrade to a v2 package in a separate invocation
        let v2 = seed_plugin_package_version(ws, "plugins/demo-v2", "demo.plugin", "0.2.0");
        let (code, stdout, stderr) = run_cli(ws, &["upgrade", "demo.plugin", v2.to_str().unwrap()]);

        // assert — the upgrade reports the version transition and stays enabled
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(
            stdout.contains("\"previous_version\": \"0.1.0\""),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"version\": \"0.2.0\""),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("\"enablement\": \"enabled\""),
            "stdout: {stdout}"
        );
        assert!(stdout.contains(v2.to_str().unwrap()), "stdout: {stdout}");

        // assert — a further invocation durably sees the upgraded, enabled plugin
        let (code, stdout, stderr) = run_cli(ws, &["list"]);
        assert_eq!(code, 0, "stderr: {stderr}");
        assert!(stdout.contains("\"enabled\": 1"), "stdout: {stdout}");
        assert!(stdout.contains(v2.to_str().unwrap()), "stdout: {stdout}");
        assert!(
            ws.join(".agent-harness/plugins.json").is_file(),
            "durable journal must persist after upgrade"
        );
    }

    #[test]
    fn upgrade_with_mismatched_manifest_id_fails_closed_and_restores() {
        // arrange — an enabled plugin and a replacement package with a different id
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let original = seed_plugin_package(ws, "plugins/orig", "orig.plugin");
        let wrong = seed_plugin_package(ws, "plugins/wrong", "wrong.plugin");
        let (code, _, stderr) = run_cli(ws, &["install", original.to_str().unwrap()]);
        assert_eq!(code, 0, "stderr: {stderr}");
        let (code, _, stderr) = run_cli(ws, &["activate", "orig.plugin"]);
        assert_eq!(code, 0, "stderr: {stderr}");

        // act — upgrade with the mismatched package
        let (code, _, stderr) = run_cli(ws, &["upgrade", "orig.plugin", wrong.to_str().unwrap()]);

        // assert — fail closed: the original plugin survives, the wrong id is not registered
        assert_eq!(code, 1);
        assert!(
            stderr.contains("expected plugin `orig.plugin`"),
            "stderr: {stderr}"
        );
        let (code, stdout, _) = run_cli(ws, &["list"]);
        assert_eq!(code, 0);
        assert!(stdout.contains("\"count\": 1"), "stdout: {stdout}");
        assert!(
            stdout.contains("\"id\": \"orig.plugin\""),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("\"enabled\": 1"), "stdout: {stdout}");
        assert!(!stdout.contains("wrong.plugin"), "stdout: {stdout}");
    }

    #[test]
    fn upgrade_unknown_id_fails_closed() {
        // arrange
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let package = seed_plugin_package_version(ws, "plugins/ghost-v2", "ghost.plugin", "0.2.0");

        // act — upgrade a plugin that was never installed
        let (code, _, stderr) =
            run_cli(ws, &["upgrade", "ghost.plugin", package.to_str().unwrap()]);

        // assert
        assert_eq!(code, 1);
        assert!(stderr.contains("not installed"), "stderr: {stderr}");
    }
}
