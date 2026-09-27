use crate::{CliDeps, CliIo};
use harness_core::{
    extension_registry::ExtensionDescriptorRegistry,
    integrations::{InstalledPlugin, PluginActivationPermission, PluginRuntimeContract},
};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct PluginCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    Install { package_root: PathBuf },
    Activate { id: String },
    Deactivate { id: String },
    Remove { id: String },
    Upgrade { id: String, package_root: PathBuf },
    List,
    Discover,
}
pub(crate) fn execute(
    command: PluginCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = crate::workspace::workspace_root(command.workspace, deps)?;
    if matches!(command.action, Action::Discover) {
        let mut registry = ExtensionDescriptorRegistry::open(&root).map_err(|e| e.to_string())?;
        let found = registry
            .discover_and_register(&root)
            .map_err(|e| e.to_string())?;
        let descriptors: Vec<_> = registry
            .list()
            .into_iter()
            .map(|e| {
                json!({
                    "extension_id":e.extension_id,"manifest_path":e.manifest_path,
                    "capabilities":e.capabilities,"enabled_capabilities":e.enabled_capabilities,
                    "tools":e.tools,"hooks":e.hooks,"loads_external_code":e.loads_external_code
                })
            })
            .collect();
        return crate::inspect::print_json(
            io,
            &json!({
                "workspace_root":root,"registry_path":registry.registry_path(),
                "discovered":found.discovered,"loads_external_code":found.loads_external_code,
                "registered":registry.summary().registered,"count":descriptors.len(),"descriptors":descriptors
            }),
        );
    }
    let mut registry = PluginRuntimeContract::open(&root).map_err(|e| e.to_string())?;
    let mut report =
        json!({"workspace_root":root,"registry_path":registry.registry().registry_path()});
    // An explicit activate/upgrade command grants package validation and receipt creation.
    let permission = PluginActivationPermission::Granted;
    let result = match command.action {
        Action::Install { package_root } => {
            registry.install_from_package_root(package_root).map(view)
        }
        Action::Activate { id } => registry.activate(&id, permission).map(view),
        Action::Deactivate { id } => registry.deactivate(&id).map(view),
        Action::Upgrade { id, package_root } => {
            if let Some(previous) = registry.get(&id) {
                report["previous_version"] = json!(previous.manifest.version);
                report["previous_package_root"] = json!(previous.package_root);
            }
            registry
                .upgrade_plugin(&id, package_root, permission)
                .map(|plugin| {
                    report["version"] = json!(plugin.manifest.version);
                    view(plugin)
                })
        }
        Action::Remove { id } => registry.remove(&id).map(|plugin| {
            report["removed"] = view(&plugin);
            Value::Null
        }),
        Action::List | Action::Discover => {
            report["plugins"] = registry.list().map(view).collect();
            report["count"] = json!(registry.len());
            report["summary"] = json!(registry.summary());
            Ok(Value::Null)
        }
    }
    .map_err(|e| e.to_string())?;
    if !result.is_null() {
        report["plugin"] = result;
    }
    crate::inspect::print_json(io, &report)
}
fn view(plugin: &InstalledPlugin) -> Value {
    json!({"id":plugin.id,"package_root":plugin.package_root,
        "enablement":plugin.enablement,"loads_code":plugin.loads_code()})
}
