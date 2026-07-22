//! Product CLI for the plugin runtime lifecycle (`harness plugin`).
//!
//! Surfaces install/activate/deactivate/remove/list over a durable
//! [`harness_core::integrations::PluginLifecycleRegistry`] persisted at
//! `<workspace>/.agent-harness/plugins.json`, so a plugin installed in one
//! invocation is visible to the next. `activate` is an explicit operator action:
//! invoking the command is the permission grant (the registry never auto-grants).
//! No `.so`/wasm execution, marketplace, or remote install is performed.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::integrations::{
    InstalledPlugin, PluginActivationPermission, PluginLifecycleError, PluginLifecycleRegistry,
    PluginLifecycleSummary,
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
    /// List installed plugins with a lifecycle summary (JSON result).
    List(ListArgs),
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
        PluginSubcommand::List(args) => run_list(args, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn open(root: &std::path::Path, io: &mut CliIo<'_>) -> Result<PluginLifecycleRegistry, i32> {
    PluginLifecycleRegistry::open(root).map_err(|err| map_plugin_error(io, err))
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

fn single_view(
    root: &std::path::Path,
    registry: &PluginLifecycleRegistry,
    plugin: &InstalledPlugin,
) -> SingleJson {
    SingleJson {
        workspace_root: root.display().to_string(),
        registry_path: registry_path_string(registry),
        plugin: PluginView::from(plugin),
    }
}

fn registry_path_string(registry: &PluginLifecycleRegistry) -> Option<String> {
    registry
        .registry_path()
        .map(|path| path.display().to_string())
}

fn map_plugin_error(io: &mut CliIo<'_>, err: PluginLifecycleError) -> i32 {
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
struct ListJson {
    workspace_root: String,
    registry_path: Option<String>,
    summary: PluginLifecycleSummary,
    count: usize,
    plugins: Vec<PluginView>,
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
        let package = workspace.join(dir_name);
        fs::create_dir_all(&package).expect("create package dir");
        let manifest = serde_json::json!({
            "schemaVersion": EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            "id": id,
            "displayName": "CLI test plugin",
            "version": "0.1.0",
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
}
