use super::{
    modules::{Modules, Resolver},
    protocol::{harness_eval, Protocol},
};
use deno_core::{v8, ModuleSpecifier, PollEventLoopOptions};
use deno_resolver::npm::{
    ByonmInNpmPackageChecker, ByonmNpmResolver, ByonmNpmResolverCreateOptions,
    DenoInNpmPackageChecker,
};
use deno_runtime::{
    deno_fs::RealFs,
    deno_node::NodeExtInitServices,
    deno_permissions::{PermissionsContainer, RuntimePermissionDescriptorParser},
    worker::{MainWorker, WorkerOptions, WorkerServiceOptions},
    BootstrapOptions, WorkerExecutionMode,
};
use node_resolver::{
    cache::NodeResolutionSys, DenoIsBuiltInNodeModuleChecker, PackageJsonResolver,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, rc::Rc, sync::Arc, time::Instant};
use sys_traits::impls::RealSys;

mod snapshot_sources {
    include!(concat!(env!("OUT_DIR"), "/eval-sources.rs"));
}

deno_core::extension!(eval_warmup, js = [dir "src/javascript", "warmup.js"]);

pub(super) async fn run() -> Result<(), String> {
    let (protocol, mut input) = Protocol::start();
    let options = input
        .recv()
        .await
        .ok_or("eval worker requires initialization")?;
    if options["type"] != "init" {
        return Err("first eval worker message must be init".into());
    }
    let directory = std::env::current_dir().map_err(|e| e.to_string())?;
    let main = ModuleSpecifier::from_file_path(directory.join(".harness-eval-cell.js"))
        .map_err(|()| "invalid JavaScript working directory".to_owned())?;
    let packages = Arc::new(PackageJsonResolver::new(RealSys, None));
    let npm = ByonmNpmResolver::new(ByonmNpmResolverCreateOptions {
        root_node_modules_dir: Some(directory.join("node_modules")),
        search_stop_dir: None,
        sys: NodeResolutionSys::new(RealSys, None),
        pkg_json_resolver: Arc::clone(&packages),
    });
    let resolver = Arc::new(Resolver::new(
        DenoInNpmPackageChecker::Byonm(ByonmInNpmPackageChecker),
        DenoIsBuiltInNodeModuleChecker,
        npm.clone(),
        Arc::clone(&packages),
        NodeResolutionSys::new(RealSys, None),
        Default::default(),
    ));
    let modules = Rc::new(Modules::new(
        Arc::clone(&resolver),
        main.clone(),
        npm,
        Arc::clone(&packages),
    ));
    let services = WorkerServiceOptions {
        blob_store: Arc::new(deno_runtime::deno_web::BlobStore::default()),
        broadcast_channel: Default::default(),
        deno_rt_native_addon_loader: None,
        feature_checker: Arc::default(),
        fs: Arc::new(RealFs),
        module_loader: Rc::clone(&modules) as Rc<dyn deno_core::ModuleLoader>,
        node_services: Some(NodeExtInitServices {
            node_require_loader: modules,
            node_resolver: resolver,
            pkg_json_resolver: packages,
            sys: RealSys,
        }),
        npm_process_state_provider: None,
        permissions: PermissionsContainer::allow_all(Arc::new(
            RuntimePermissionDescriptorParser::new(RealSys),
        )),
        root_cert_store_provider: None,
        fetch_dns_resolver: Default::default(),
        shared_array_buffer_store: None,
        compiled_wasm_module_store: None,
        v8_code_cache: None,
        bundle_provider: None,
    };
    let mut worker = MainWorker::bootstrap_from_options(
        &main,
        services,
        WorkerOptions {
            startup_snapshot: Some(include_bytes!(concat!(env!("OUT_DIR"), "/eval-v8.bin"))),
            residual_lazy_js_sources: snapshot_sources::JS,
            residual_lazy_esm_sources: snapshot_sources::ESM,
            extensions: vec![
                eval_warmup::init(),
                harness_eval::init(Arc::clone(&protocol)),
            ],
            // V8's embedder default is below our 2 GiB recycle threshold. Allow
            // the normal Node-sized heap, without allocating it up front.
            create_params: Some(v8::CreateParams::default().heap_limits(0, 4 * 1024 * 1024 * 1024)),
            stdio: protocol
                .stdio(std::path::Path::new(
                    options["captureRoot"]
                        .as_str()
                        .ok_or("captureRoot is required")?,
                ))
                .map_err(|error| error.to_string())?,
            bootstrap: BootstrapOptions {
                mode: WorkerExecutionMode::Eval,
                deno_version: "harness".into(),
                user_agent: "Harness eval".into(),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    worker
        .js_runtime
        .execute_script("harness-eval-tools.js", include_str!("tools.js"))
        .map_err(|error| error.to_string())?;
    let prelude = worker
        .js_runtime
        .execute_script("harness-eval-prelude.js", include_str!("prelude.js"))
        .map_err(|error| error.to_string())?;
    let prelude = worker.js_runtime.resolve(prelude);
    worker
        .js_runtime
        .with_event_loop_promise(prelude, PollEventLoopOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    let protected = worker
        .js_runtime
        .execute_script(
            "harness-eval-globals.js",
            "Object.getOwnPropertyNames(globalThis)",
        )
        .map_err(|error| error.to_string())?;
    let protected: BTreeSet<String> = {
        deno_core::scope!(scope, &mut worker.js_runtime);
        let value = v8::Local::new(scope, protected);
        deno_core::serde_v8::from_v8(scope, value).map_err(|error| error.to_string())?
    };
    protocol
        .emit(json!({"type":"ready","runtime":{"name":"V8","version":v8::V8::get_version()}}))
        .map_err(|error| error.to_string())?;
    let settings =
        serde_json::from_value(options["memory"].clone()).map_err(|error| error.to_string())?;
    let mut memory = super::memory::Memory::new(settings);
    loop {
        let idle = memory.idle_delay();
        let request = worker.js_runtime.with_event_loop_promise(Box::pin(async {
            if let Some(delay) = idle {
                tokio::select! {
                    request = input.recv() => Ok::<_, deno_core::error::CoreError>(Some(request)),
                    () = tokio::time::sleep(delay) => Ok(None),
                }
            } else { Ok(Some(input.recv().await)) }
        }), PollEventLoopOptions::default()).await.map_err(|error| error.to_string())?;
        let Some(request) = request else {
            memory.collect(&mut worker.js_runtime);
            continue;
        };
        let Some(request) = request else {
            break;
        };
        let id = request["id"].as_str().ok_or("cell id is required")?;
        protocol.begin(id).map_err(|error| error.to_string())?;
        let started = Instant::now();
        let source = request["code"]
            .as_str()
            .ok_or("code is required")?
            .to_owned();
        let result = async {
            let mut setup = options.clone();
            setup["cellId"] = id.into();
            setup["preludes"] = request["preludes"].clone();
            setup["tools"] = request["tools"].clone();
            worker.js_runtime.execute_script("harness-eval-begin.js", format!("__harness_begin({setup})"))
                .map_err(|error| error.to_string())?;
            let source = super::transform::cell(&source, &main, &protected)?;
            let source = format!("__harness_run(() => ({source}).then(async (value) => {{ await __harness_drain(); return JSON.stringify(value); }}))");
            let value = worker.js_runtime.execute_script(main.to_string(), source).map_err(|e| e.to_string())?;
            let future = worker.js_runtime.resolve(value);
            let value = {
                let future = worker.js_runtime.with_event_loop_promise(future, PollEventLoopOptions::default());
                tokio::pin!(future);
                let mut flush = tokio::time::interval(std::time::Duration::from_millis(50));
                loop {
                    tokio::select! {
                        result = &mut future => break result.map_err(|error| error.to_string())?,
                        _ = flush.tick() => protocol.flush_native().map_err(|error| error.to_string())?,
                    }
                }
            };
            deno_core::scope!(scope, &mut worker.js_runtime);
            let value = v8::Local::new(scope, value);
            deno_core::serde_v8::from_v8::<Option<String>>(scope, value).map_err(|e| e.to_string())
        }.await;
        worker
            .js_runtime
            .execute_script("harness-eval-end.js", "__harness_end()")
            .map_err(|error| error.to_string())?;
        let mut result = match result {
            Ok(value) => {
                json!({"type":"result","ok":true,"valueRepr":value,"durationMs":started.elapsed().as_millis()})
            }
            Err(error) => {
                json!({"type":"result","ok":false,"error":{"message":error},"durationMs":started.elapsed().as_millis()})
            }
        };
        result["memory"] = memory.after_cell(&mut worker.js_runtime);
        protocol.emit(result).map_err(|error| error.to_string())?;
        protocol.end().map_err(|error| error.to_string())?;
    }
    Ok(())
}
