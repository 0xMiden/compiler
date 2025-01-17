use alloc::{collections::BTreeMap, sync::Arc};
use core::any::TypeId;

use midenc_hir_symbol::sync::{LazyLock, RwLock};
use midenc_session::diagnostics::DiagnosticsHandler;

use super::*;
use crate::Report;

static PASS_REGISTRY: LazyLock<PassRegistry> = LazyLock::new(PassRegistry::new);

/// A global, thread-safe pass and pass pipeline registry
///
/// You should generally _not_ need to work with this directly.
pub struct PassRegistry {
    passes: RwLock<BTreeMap<&'static str, PassRegistryEntry>>,
    pipelines: RwLock<BTreeMap<&'static str, PassRegistryEntry>>,
}
impl Default for PassRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl PassRegistry {
    /// Create a new [PassRegistry] instance.
    pub fn new() -> Self {
        let mut passes = BTreeMap::default();
        let mut pipelines = BTreeMap::default();
        for pass in inventory::iter::<PassInfo>() {
            passes.insert(
                pass.0.arg,
                PassRegistryEntry {
                    arg: pass.0.arg,
                    description: pass.0.description,
                    type_id: pass.0.type_id,
                    builder: Arc::clone(&pass.0.builder),
                },
            );
        }
        for pipeline in inventory::iter::<PassPipelineInfo>() {
            pipelines.insert(
                pipeline.0.arg,
                PassRegistryEntry {
                    arg: pipeline.0.arg,
                    description: pipeline.0.description,
                    type_id: pipeline.0.type_id,
                    builder: Arc::clone(&pipeline.0.builder),
                },
            );
        }

        Self {
            passes: RwLock::new(passes),
            pipelines: RwLock::new(pipelines),
        }
    }

    /// Get the pass information for the pass whose argument name is `name`
    pub fn get_pass(&self, name: &str) -> Option<PassInfo> {
        self.passes.read().get(name).cloned().map(PassInfo)
    }

    /// Get the pass pipeline information for the pipeline whose argument name is `name`
    pub fn get_pipeline(&self, name: &str) -> Option<PassPipelineInfo> {
        self.pipelines.read().get(name).cloned().map(PassPipelineInfo)
    }

    /// Register the given pass
    pub fn register_pass(&self, info: PassInfo) {
        use alloc::collections::btree_map::Entry;

        let mut passes = self.passes.write();
        match passes.entry(info.argument()) {
            Entry::Vacant(entry) => {
                entry.insert(info.0);
            }
            Entry::Occupied(entry) => {
                assert_eq!(
                    entry.get().type_id,
                    info.0.type_id,
                    "cannot register pass '{}': name already registered by a different type",
                    info.argument()
                );
            }
        }
    }

    /// Register the given pass pipeline
    pub fn register_pipeline(&self, info: PassPipelineInfo) {
        use alloc::collections::btree_map::Entry;

        let mut pipelines = self.pipelines.write();
        match pipelines.entry(info.argument()) {
            Entry::Vacant(entry) => {
                entry.insert(info.0);
            }
            Entry::Occupied(entry) => {
                assert_eq!(
                    entry.get().type_id,
                    info.0.type_id,
                    "cannot register pass pipeline '{}': name already registered by a different \
                     type",
                    info.argument()
                );
                assert!(Arc::ptr_eq(&entry.get().builder, &info.0.builder))
            }
        }
    }
}

inventory::collect!(PassInfo);
inventory::collect!(PassPipelineInfo);

/// A type alias for the closure type for registering a pass with a pass manager
pub type PassRegistryFunction = dyn Fn(&mut OpPassManager, &str, &DiagnosticsHandler) -> Result<(), Report>
    + Send
    + Sync
    + 'static;

/// A type alias for the closure type used for type-erased pass constructors
pub type PassAllocatorFunction = dyn Fn() -> Box<dyn OperationPass>;

/// A [RegistryEntry] is a registered pass or pass pipeline.
///
/// This trait provides the common functionality shared by both passes and pipelines.
pub trait RegistryEntry {
    /// Returns the command-line option that may be passed to `midenc` that will cause this pass
    /// or pass pipeline to run.
    fn argument(&self) -> &'static str;
    /// Return a description for the pass or pass pipeline.
    fn description(&self) -> &'static str;
    /// Adds this entry to the given pass manager.
    ///
    /// Note: `options` is an opaque string that will be parsed by the builder.
    ///
    /// Returns `Err` if an error occurred parsing the given options.
    fn add_to_pipeline(
        &self,
        pm: &mut OpPassManager,
        options: &str,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report>;
}

/// Information about a pass or pass pipeline in the pass registry
#[derive(Clone)]
struct PassRegistryEntry {
    /// The name of the compiler option for referencing on the command line
    arg: &'static str,
    /// A description of the pass or pass pipeline
    description: &'static str,
    /// The type id of the concrete pass type
    type_id: Option<TypeId>,
    /// Function that registers this entry with a pass manager pipeline
    builder: Arc<PassRegistryFunction>,
}
impl RegistryEntry for PassRegistryEntry {
    #[inline]
    fn add_to_pipeline(
        &self,
        pm: &mut OpPassManager,
        options: &str,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        (self.builder)(pm, options, diagnostics)
    }

    #[inline(always)]
    fn argument(&self) -> &'static str {
        self.arg
    }

    #[inline(always)]
    fn description(&self) -> &'static str {
        self.description
    }
}

/// Information about a registered pass pipeline
pub struct PassPipelineInfo(PassRegistryEntry);
impl PassPipelineInfo {
    pub fn new<B>(arg: &'static str, description: &'static str, builder: B) -> Self
    where
        B: Fn(&mut OpPassManager, &str, &DiagnosticsHandler) -> Result<(), Report>
            + Send
            + Sync
            + 'static,
    {
        Self(PassRegistryEntry {
            arg,
            description,
            type_id: None,
            builder: Arc::new(builder),
        })
    }

    /// Find the [PassInfo] for a registered pass pipeline named `name`
    pub fn lookup(name: &str) -> Option<PassPipelineInfo> {
        PASS_REGISTRY.get_pipeline(name)
    }
}
impl RegistryEntry for PassPipelineInfo {
    fn argument(&self) -> &'static str {
        self.0.argument()
    }

    fn description(&self) -> &'static str {
        self.0.description()
    }

    fn add_to_pipeline(
        &self,
        pm: &mut OpPassManager,
        options: &str,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        self.0.add_to_pipeline(pm, options, diagnostics)
    }
}

/// Information about a registered pass
pub struct PassInfo(PassRegistryEntry);
impl PassInfo {
    /// Create a new [PassInfo] from the given argument name and description, for a default-
    /// constructible pass type `P`.
    pub fn new<P: Pass + Default>(arg: &'static str, description: &'static str) -> Self {
        let type_id = TypeId::of::<P>();
        Self(PassRegistryEntry {
            arg,
            description,
            type_id: Some(type_id),
            builder: Arc::new(default_registration::<P>),
        })
    }

    /// Find the [PassInfo] for a registered pass named `name`
    pub fn lookup(name: &str) -> Option<PassInfo> {
        PASS_REGISTRY.get_pass(name)
    }
}
impl RegistryEntry for PassInfo {
    fn argument(&self) -> &'static str {
        self.0.argument()
    }

    fn description(&self) -> &'static str {
        self.0.description()
    }

    fn add_to_pipeline(
        &self,
        pm: &mut OpPassManager,
        options: &str,
        diagnostics: &DiagnosticsHandler,
    ) -> Result<(), Report> {
        self.0.add_to_pipeline(pm, options, diagnostics)
    }
}

/// Register a specific dialect pipeline registry function with the system.
///
/// # Example
///
/// If your pipeline implements the [Default] trait, you can just do:
///
/// ```text,ignore
/// register_pass_pipeline(
///     "my-pipeline",
///     "A simple test pipeline",
///     default_registration::<MyPipeline>(),
/// )
/// ```
///
/// Otherwise, you need to pass a factor function which will be used to construct fresh instances
/// of the pipeline:
///
/// ```text,ignore
/// register_pass_pipeline(
///     "my-pipeline",
///     "A simple test pipeline",
///     default_dyn_registration(|| MyPipeline::new(MyPipelineOptions::default())),
/// )
/// ```
///
/// NOTE: The functions/closures passed above are required to be `Send + Sync + 'static`, as they
/// are stored in the global registry for the lifetime of the program, and may be accessed from any
/// thread.
pub fn register_pass_pipeline<B, O>(arg: &'static str, description: &'static str, builder: B)
where
    B: Fn(&mut OpPassManager, &str, &DiagnosticsHandler) -> Result<(), Report>
        + Send
        + Sync
        + 'static,
{
    PASS_REGISTRY.register_pipeline(PassPipelineInfo(PassRegistryEntry {
        arg,
        description,
        type_id: None,
        builder: Arc::new(builder),
    }));
}

/// Register a specific dialect pass allocator function with the system.
///
/// # Example
///
/// ```text,ignore
/// register_pass(|| MyPass::default())
/// ```
///
/// NOTE: The allocator function provided is required to be `Send + Sync + 'static`, as it is
/// stored in the global registry for the lifetime of the program, and may be accessed from any
/// thread.
pub fn register_pass(ctor: impl Fn() -> Box<dyn OperationPass> + Send + Sync + 'static) {
    let pass = ctor();
    let type_id = pass.as_any().type_id();
    let arg = pass.argument();
    assert!(
        !arg.is_empty(),
        "attempted to register pass '{}' without specifying an argument name",
        pass.name()
    );
    let description = pass.description();
    PASS_REGISTRY.register_pass(PassInfo(PassRegistryEntry {
        arg,
        description,
        type_id: Some(type_id),
        builder: Arc::new(default_registration_factory(ctor)),
    }));
}

/// A default implementation of a pass pipeline registration function.
///
/// It expects that `P` (the type of the pass or pass pipeline), implements `Default`, so that an
/// instance is default-constructible. It then initializes the pass with the provided options,
/// validates that the pass/pipeline is valid for the parent pipeline, and adds it if so.
pub fn default_registration<P: Pass + Default>(
    pm: &mut OpPassManager,
    options: &str,
    diagnostics: &DiagnosticsHandler,
) -> Result<(), Report> {
    use midenc_session::diagnostics::Severity;

    let mut pass = Box::<P>::default() as Box<dyn OperationPass>;
    let result = pass.initialize_options(options);
    let pm_op_name = pm.name();
    let pass_op_name = pass.target_name(&pm.context());
    let pass_op_name = pass_op_name.as_ref();
    if matches!(pm.nesting(), Nesting::Explicit) && pm_op_name != pass_op_name {
        return Err(diagnostics
            .diagnostic(Severity::Error)
            .with_message(format!(
                "registration error for pass '{}': can't add pass restricted to '{}' on a pass \
                 manager intended to run on '{}', did you intend to nest?",
                pass.name(),
                crate::formatter::DisplayOptional(pass_op_name.as_ref()),
                crate::formatter::DisplayOptional(pm_op_name),
            ))
            .into_report());
    }
    pm.add_pass(pass);
    result
}

/// Like [default_registration], but takes an arbitrary constructor in the form of a zero-arity
/// closure, rather than relying on [Default]. Thus, this is actually a registration function
/// _factory_, rather than a registration function itself.
pub fn default_registration_factory<B: Fn() -> Box<dyn OperationPass> + Send + Sync + 'static>(
    builder: B,
) -> impl Fn(&mut OpPassManager, &str, &DiagnosticsHandler) -> Result<(), Report> + Send + Sync + 'static
{
    use midenc_session::diagnostics::Severity;
    move |pm: &mut OpPassManager,
          options: &str,
          diagnostics: &DiagnosticsHandler|
          -> Result<(), Report> {
        let mut pass = builder();
        let result = pass.initialize_options(options);
        let pm_op_name = pm.name();
        let pass_op_name = pass.target_name(&pm.context());
        let pass_op_name = pass_op_name.as_ref();
        if matches!(pm.nesting(), Nesting::Explicit) && pm_op_name != pass_op_name {
            return Err(diagnostics
                .diagnostic(Severity::Error)
                .with_message(format!(
                    "registration error for pass '{}': can't add pass restricted to '{}' on a \
                     pass manager intended to run on '{}', did you intend to nest?",
                    pass.name(),
                    crate::formatter::DisplayOptional(pass_op_name.as_ref()),
                    crate::formatter::DisplayOptional(pm_op_name),
                ))
                .into_report());
        }
        pm.add_pass(pass);
        result
    }
}
