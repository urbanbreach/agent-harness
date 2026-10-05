use deno_ast::MediaType;
use deno_core::{
    error::ModuleLoaderError, FastString, ModuleLoadOptions, ModuleLoadReferrer,
    ModuleLoadResponse, ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType,
    ResolutionKind,
};
use deno_error::JsErrorBox;
use deno_resolver::{
    cjs::{
        analyzer::{DenoAstModuleExportAnalyzer, DenoCjsCodeAnalyzer, NullNodeAnalysisCache},
        CjsTracker, IsCjsResolutionMode,
    },
    npm::{ByonmInNpmPackageChecker, ByonmNpmResolver, DenoInNpmPackageChecker},
};
use deno_runtime::{deno_node, deno_permissions::PermissionsContainer};
use node_resolver::{NodeResolutionKind, ResolutionMode};
use std::{borrow::Cow, path::Path, sync::Arc};
use sys_traits::impls::RealSys;

pub(super) type Resolver = node_resolver::NodeResolver<
    DenoInNpmPackageChecker,
    node_resolver::DenoIsBuiltInNodeModuleChecker,
    ByonmNpmResolver<RealSys>,
    RealSys,
>;

pub(super) struct Modules {
    pub resolver: Arc<Resolver>,
    pub base: ModuleSpecifier,
    tracker: Arc<CjsTracker<DenoInNpmPackageChecker, RealSys>>,
    translator: Arc<
        node_resolver::analyze::NodeCodeTranslator<
            DenoCjsCodeAnalyzer<RealSys>,
            DenoInNpmPackageChecker,
            node_resolver::DenoIsBuiltInNodeModuleChecker,
            ByonmNpmResolver<RealSys>,
            RealSys,
        >,
    >,
}

impl Modules {
    pub fn new(
        resolver: Arc<Resolver>,
        base: ModuleSpecifier,
        npm: ByonmNpmResolver<RealSys>,
        packages: Arc<node_resolver::PackageJsonResolver<RealSys>>,
    ) -> Self {
        let checker = DenoInNpmPackageChecker::Byonm(ByonmInNpmPackageChecker);
        let tracker = Arc::new(CjsTracker::new(
            checker.clone(),
            Arc::clone(&packages),
            IsCjsResolutionMode::ImplicitTypeCommonJs,
            Vec::new(),
        ));
        let analyzer = DenoCjsCodeAnalyzer::new(
            Arc::new(NullNodeAnalysisCache),
            Arc::clone(&tracker),
            Arc::new(DenoAstModuleExportAnalyzer::new(Arc::default())),
        );
        let analyzer = Arc::new(node_resolver::analyze::CjsModuleExportAnalyzer::new(
            analyzer,
            checker,
            Arc::clone(&resolver),
            npm,
            packages,
            RealSys,
        ));
        Self {
            resolver,
            base,
            tracker,
            translator: Arc::new(node_resolver::analyze::NodeCodeTranslator::new(
                analyzer,
                Default::default(),
            )),
        }
    }
}

impl ModuleLoader for Modules {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, ModuleLoaderError> {
        let referrer = ModuleSpecifier::parse(referrer).unwrap_or_else(|_| self.base.clone());
        self.resolver
            .resolve(
                specifier,
                &referrer,
                ResolutionMode::Import,
                NodeResolutionKind::Execution,
            )
            .and_then(node_resolver::NodeResolution::into_url)
            .map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let translator = Arc::clone(&self.translator);
        let tracker = Arc::clone(&self.tracker);
        let specifier = specifier.clone();
        ModuleLoadResponse::Async(Box::pin(async move {
            let path = specifier
                .to_file_path()
                .map_err(|()| JsErrorBox::generic(format!("cannot load module {specifier}")))?;
            let mut source = std::fs::read_to_string(&path).map_err(JsErrorBox::from_err)?;
            let kind = if path.extension().is_some_and(|ext| ext == "json") {
                ModuleType::Json
            } else {
                ModuleType::JavaScript
            };
            if kind == ModuleType::JavaScript {
                let media_type = MediaType::from_specifier(&specifier);
                let parsed = deno_ast::parse_program(deno_ast::ParseParams {
                    specifier: specifier.clone(),
                    text: source.clone().into(),
                    media_type,
                    capture_tokens: false,
                    scope_analysis: false,
                    maybe_syntax: None,
                })
                .map_err(JsErrorBox::from_err)?;
                let is_cjs = tracker
                    .is_cjs_with_known_is_script(&specifier, media_type, parsed.compute_is_script())
                    .map_err(JsErrorBox::from_err)?;
                if is_cjs {
                    source = translator
                        .translate_cjs_to_esm(&specifier, Some(Cow::Owned(source)))
                        .await
                        .map_err(JsErrorBox::from_err)?
                        .into_owned();
                } else if !matches!(
                    media_type,
                    MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs
                ) {
                    source = parsed
                        .transpile(
                            &Default::default(),
                            &Default::default(),
                            &Default::default(),
                        )
                        .map_err(JsErrorBox::from_err)?
                        .into_source()
                        .text;
                }
            }
            Ok(ModuleSource::new(
                kind,
                ModuleSourceCode::String(source.into()),
                &specifier,
                None,
            ))
        }))
    }
}

impl deno_node::NodeRequireLoader for Modules {
    fn ensure_read_permission<'a>(
        &self,
        _permissions: &mut PermissionsContainer,
        path: Cow<'a, Path>,
    ) -> Result<Cow<'a, Path>, JsErrorBox> {
        // The coordinator has approved local code execution. Like the other
        // kernels, this interpreter is not an additional filesystem sandbox.
        Ok(path)
    }

    fn load_text_file_lossy(&self, path: &Path) -> Result<FastString, JsErrorBox> {
        std::fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned().into())
            .map_err(JsErrorBox::from_err)
    }

    fn is_maybe_cjs(
        &self,
        specifier: &ModuleSpecifier,
    ) -> Result<bool, node_resolver::errors::PackageJsonLoadError> {
        self.tracker
            .is_maybe_cjs(specifier, MediaType::from_specifier(specifier))
    }

    fn is_maybe_cjs_from_require(
        &self,
        specifier: &ModuleSpecifier,
    ) -> Result<bool, node_resolver::errors::PackageJsonLoadError> {
        self.tracker
            .is_maybe_cjs_from_require(specifier, MediaType::from_specifier(specifier))
    }
}
