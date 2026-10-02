//! Wasm feature and structural policy for note codec components.
//!
//! Parsing, compilation, and instantiation cost work before any fuel budget applies, so a byte
//! cap alone does not bound that work. A small binary can declare thousands of tiny functions,
//! thousands of globals, or a component tree whose instantiations expand exponentially.
//!
//! The rules are fixed policy. The producer applies them when it attaches a codec, and every
//! consumer applies them when it loads one. [`validate_note_codec_component`] runs the whole
//! policy in one place. It checks that:
//!
//! - the component fits in the caller's byte budget;
//! - the component validates under [`NOTE_CODEC_WASM_FEATURES`];
//! - the component declares no component-level start function;
//! - each core module stays under its own counts, each of its function types stays under the
//!   parameter and result caps, and its code section keeps a plausible average function size;
//! - the component tree stays under its budgets for core modules and nesting depth;
//! - one instantiation of any component in the tree stays under its budgets for core
//!   instantiations, component instantiations, linear memories, and tables;
//! - each kind of component-level entry stays under its cap for the whole tree.
//!
//! The walk counts what an instantiation creates, not what the tree declares. A core module that
//! is instantiated twice counts twice, and a nested component contributes what one instantiation
//! of it creates, once per instantiation. A consumer store admits the same counts, so a component
//! that passes the walk instantiates inside the store limits.
//!
//! The walk rejects only a component-level start function. A core module keeps its own start
//! function, which runs at instantiation under the consumer fuel budget and store limits.
//!
//! Nothing else is checked here. The producer checks the exported codec interface, and a
//! consumer bounds the run time of a codec call with fuel and store limits.

use wasmparser::{
    ComponentAlias, ComponentExternalKind, ComponentInstance, ComponentOuterAliasKind,
    ComponentTypeRef, Encoding, FuncValidatorAllocations, Instance, Parser, Payload, TypeRef,
    ValidPayload, Validator, WasmFeatures,
};

use crate::{CodecFailure, Error, Result};

/// Wasm proposals a note codec component may use.
///
/// The producer and every consumer validate against this constant, so the accepted set does not
/// depend on the Cargo features a host compiled its engine with. The set holds the proposals the
/// `wasm32-wasip2` target emits, the component model, and floating point. Every other proposal is
/// off, including SIMD, threads, garbage collection, typed function references, and the
/// asynchronous component-model additions.
///
/// The value is written bit by bit instead of subtracting from `WasmFeatures::default()` or
/// `WasmFeatures::all()`, so a wasmparser upgrade cannot widen the policy.
pub const NOTE_CODEC_WASM_FEATURES: WasmFeatures = WasmFeatures::COMPONENT_MODEL
    .union(WasmFeatures::FLOATS)
    .union(WasmFeatures::MUTABLE_GLOBAL)
    .union(WasmFeatures::SATURATING_FLOAT_TO_INT)
    .union(WasmFeatures::SIGN_EXTENSION)
    .union(WasmFeatures::REFERENCE_TYPES)
    .union(WasmFeatures::MULTI_VALUE)
    .union(WasmFeatures::BULK_MEMORY);

/// Maximum functions in one core module, imported and defined.
const MAX_MODULE_FUNCTIONS: usize = 10_000;

/// Maximum globals in one core module, imported and defined.
const MAX_MODULE_GLOBALS: usize = 1_000;

/// Maximum types in one core module.
const MAX_MODULE_TYPES: usize = 1_000;

/// Maximum tables in one core module, imported and defined.
///
/// A module that is instantiated reaches the tighter tree-wide instantiated-table budget first.
/// This cap constrains imported tables, and the tables of a module that is never instantiated.
const MAX_MODULE_TABLES: usize = 100;

/// Maximum linear memories in one core module, imported and defined.
const MAX_MODULE_MEMORIES: usize = 1;

/// Maximum element segments in one core module.
const MAX_MODULE_ELEMENT_SEGMENTS: usize = 1_000;

/// Maximum data segments in one core module.
const MAX_MODULE_DATA_SEGMENTS: usize = 1_000;

/// Maximum imports in one core module.
const MAX_MODULE_IMPORTS: usize = 1_024;

/// Maximum exports in one core module.
const MAX_MODULE_EXPORTS: usize = 1_024;

/// Maximum parameters in one core function type.
const MAX_FUNCTION_PARAMS: usize = 32;

/// Maximum results in one core function type.
const MAX_FUNCTION_RESULTS: usize = 32;

/// Code section size, in bytes, above which the average function size applies.
///
/// Small modules stay below it, so a short helper module is never rejected for its shape.
const MIN_METERED_CODE_SECTION_BYTES: u32 = 1_000;

/// Minimum average bytes per function body in a metered code section.
///
/// Compiled Wasm averages far above this value. A module below it is a compilation bomb:
/// many tiny functions that each cost a fixed amount of host work.
const MIN_AVERAGE_FUNCTION_BYTES: u32 = 40;

/// Maximum core modules in one component, at any nesting level.
const MAX_CORE_MODULES: usize = 16;

/// Maximum component nesting depth.
const MAX_COMPONENT_DEPTH: usize = 4;

/// Maximum entries of one kind of component-level item in the whole component tree.
///
/// A component may split one kind over many sections, so the cap counts each kind over the
/// whole tree: core types, component types, aliases, canonical functions, imports, and exports.
const MAX_COMPONENT_SECTION_ITEMS: usize = 256;

/// Maximum core instances one instantiation of a component creates.
///
/// The consumer store admits the same number of instances.
pub(crate) const MAX_CORE_INSTANCES: usize = 32;

/// Maximum nested component instances one instantiation of a component creates.
///
/// A `wasm32-wasip2` codec builds one nested component instance per exported interface. The
/// budget has no consumer store counterpart: a component instance holds no core instance,
/// memory, or table of its own, and what it creates is counted through those budgets.
const MAX_COMPONENT_INSTANCES: usize = 8;

/// Maximum tables one instantiation of a component creates.
///
/// The consumer store admits the same number of tables.
pub(crate) const MAX_INSTANTIATED_TABLES: usize = 32;

/// Maximum linear memories one instantiation of a component creates.
///
/// The consumer store admits the same number of memories.
pub(crate) const MAX_INSTANTIATED_MEMORIES: usize = 1;

/// Applies the whole note codec component policy: the byte cap, the Wasm feature set, and the
/// structural limits.
///
/// `max_bytes` is the caller's byte budget. The producer passes
/// [`MAX_NOTE_CODEC_COMPONENT_BYTES`](crate::MAX_NOTE_CODEC_COMPONENT_BYTES), and a consumer
/// passes the cap in its own limits.
pub fn validate_note_codec_component(component: &[u8], max_bytes: usize) -> Result<()> {
    ensure_component_byte_limit(component.len(), max_bytes)?;
    validate_note_codec_structure(component)
}

/// Rejects a note codec component whose Wasm features or structure fall outside the policy.
///
/// [`validate_note_codec_component`] is the entry point both sides call. This function is public
/// for a caller that bounds the byte length itself.
pub fn validate_note_codec_structure(component: &[u8]) -> Result<()> {
    let mut validator = Validator::new_with_features(NOTE_CODEC_WASM_FEATURES);
    // Function bodies are validated with one reusable allocation, not one per function.
    let mut allocations = FuncValidatorAllocations::default();
    let mut walk = StructureWalk::default();
    for payload in Parser::new(0).parse_all(component) {
        let payload = payload.map_err(malformed)?;
        // Reject a start function ahead of the validator. A start function runs guest code when
        // the component is instantiated, before the export call the limits are built around, and
        // the validator reports only that component values are disabled.
        if matches!(payload, Payload::ComponentStartSection { .. }) {
            return Err(policy_rejection(
                "note codec component declares a start function; the policy rejects a component \
                 that runs code when it is instantiated",
            ));
        }
        if let ValidPayload::Func(function, body) =
            validator.payload(&payload).map_err(rejected_feature)?
        {
            let mut function = function.into_validator(allocations);
            function.validate(&body).map_err(rejected_feature)?;
            allocations = function.into_allocations();
        }
        walk.visit(payload)?;
    }
    Ok(())
}

/// Rejects component bytes over the caller's budget, before any parsing or compilation.
fn ensure_component_byte_limit(byte_len: usize, limit: usize) -> Result<()> {
    if byte_len <= limit {
        return Ok(());
    }
    Err(policy_rejection(format!(
        "note codec component is {byte_len} bytes; the pre-compilation limit is {limit}"
    )))
}

/// The state carried while the parser walks one component.
#[derive(Default)]
struct StructureWalk {
    /// One frame per core module or component the walk entered, innermost last.
    frames: Vec<Frame>,
    /// Core modules seen anywhere in the component.
    core_modules: usize,
    /// Component-level items declared anywhere in the component, one count per kind.
    component_items: ComponentItemCounts,
}

/// Component-level items counted over the whole component tree.
#[derive(Default)]
struct ComponentItemCounts {
    core_types: usize,
    types: usize,
    aliases: usize,
    canonical_functions: usize,
    imports: usize,
    exports: usize,
}

/// One nesting level of the walk.
enum Frame {
    /// A core module, with the counters checked when the module ends.
    Module(ModuleCounts),
    /// A component, with its index spaces and what one instantiation of it creates.
    Component(ComponentFrame),
}

/// One component nesting level.
///
/// An index-space entry is `None` for a core module or a component the walk cannot read, such as
/// an imported or an aliased one. An instantiation of such an entry is rejected.
#[derive(Default)]
struct ComponentFrame {
    /// The core module index space of this component.
    core_modules: Vec<Option<ModuleRuntimeCounts>>,
    /// The component index space of this component.
    components: Vec<Option<CreatedCounts>>,
    /// What one instantiation of this component creates.
    created: CreatedCounts,
}

/// What one core module creates every time it is instantiated.
#[derive(Clone, Copy, Default)]
struct ModuleRuntimeCounts {
    memories: usize,
    tables: usize,
}

/// What one instantiation of a component creates.
#[derive(Clone, Copy, Default)]
struct CreatedCounts {
    core_instances: usize,
    component_instances: usize,
    memories: usize,
    tables: usize,
}

impl CreatedCounts {
    /// Adds what one nested instantiation creates.
    fn add(&mut self, other: Self) {
        self.core_instances = self.core_instances.saturating_add(other.core_instances);
        self.component_instances =
            self.component_instances.saturating_add(other.component_instances);
        self.memories = self.memories.saturating_add(other.memories);
        self.tables = self.tables.saturating_add(other.tables);
    }

    /// Checks every budget that bounds one instantiation.
    fn check(&self) -> Result<()> {
        ensure_created_cap("core instances", self.core_instances, MAX_CORE_INSTANCES)?;
        ensure_created_cap(
            "component instances",
            self.component_instances,
            MAX_COMPONENT_INSTANCES,
        )?;
        ensure_created_cap("linear memories", self.memories, MAX_INSTANTIATED_MEMORIES)?;
        ensure_created_cap("tables", self.tables, MAX_INSTANTIATED_TABLES)
    }
}

/// Counters collected for one core module.
#[derive(Default)]
struct ModuleCounts {
    types: usize,
    functions: usize,
    globals: usize,
    tables: usize,
    memories: usize,
    element_segments: usize,
    data_segments: usize,
    imports: usize,
    exports: usize,
    /// The memories and tables one instantiation of this module creates.
    runtime: ModuleRuntimeCounts,
}

impl ModuleCounts {
    /// Checks every per-module cap once the module ends.
    fn check(&self) -> Result<()> {
        ensure_module_cap("types", self.types, MAX_MODULE_TYPES)?;
        ensure_module_cap("functions", self.functions, MAX_MODULE_FUNCTIONS)?;
        ensure_module_cap("globals", self.globals, MAX_MODULE_GLOBALS)?;
        ensure_module_cap("tables", self.tables, MAX_MODULE_TABLES)?;
        ensure_module_cap("memories", self.memories, MAX_MODULE_MEMORIES)?;
        ensure_module_cap("element segments", self.element_segments, MAX_MODULE_ELEMENT_SEGMENTS)?;
        ensure_module_cap("data segments", self.data_segments, MAX_MODULE_DATA_SEGMENTS)?;
        ensure_module_cap("imports", self.imports, MAX_MODULE_IMPORTS)?;
        ensure_module_cap("exports", self.exports, MAX_MODULE_EXPORTS)
    }
}

impl StructureWalk {
    /// Applies one parser payload to the current nesting level.
    fn visit(&mut self, payload: Payload<'_>) -> Result<()> {
        match payload {
            Payload::Version { encoding, .. } => self.enter(encoding)?,
            Payload::End(_) => self.leave()?,
            Payload::TypeSection(reader) => {
                self.module_counts()?.types += reader.count() as usize;
                // The feature validator rejects a GC type before the walk sees the section, so
                // only a core function type reaches this loop.
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(rejected_feature)?;
                    ensure_signature_cap("parameters", ty.params().len(), MAX_FUNCTION_PARAMS)?;
                    ensure_signature_cap("results", ty.results().len(), MAX_FUNCTION_RESULTS)?;
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.map_err(malformed)?;
                    let counts = self.module_counts()?;
                    counts.imports += 1;
                    match import.ty {
                        TypeRef::Func(_) | TypeRef::FuncExact(_) => counts.functions += 1,
                        TypeRef::Global(_) => counts.globals += 1,
                        TypeRef::Table(_) => counts.tables += 1,
                        TypeRef::Memory(_) => counts.memories += 1,
                        TypeRef::Tag(_) => {}
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                self.module_counts()?.functions += reader.count() as usize;
            }
            Payload::GlobalSection(reader) => {
                self.module_counts()?.globals += reader.count() as usize;
            }
            Payload::TableSection(reader) => {
                let count = reader.count() as usize;
                let counts = self.module_counts()?;
                counts.tables += count;
                counts.runtime.tables += count;
            }
            Payload::MemorySection(reader) => {
                let count = reader.count() as usize;
                let counts = self.module_counts()?;
                counts.memories += count;
                counts.runtime.memories += count;
            }
            Payload::ElementSection(reader) => {
                self.module_counts()?.element_segments += reader.count() as usize;
            }
            Payload::DataSection(reader) => {
                self.module_counts()?.data_segments += reader.count() as usize;
            }
            Payload::ExportSection(reader) => {
                self.module_counts()?.exports += reader.count() as usize;
            }
            Payload::CodeSectionStart { count, size, .. } => {
                ensure_average_function_size(count, size)?;
            }
            Payload::InstanceSection(reader) => {
                for instance in reader {
                    self.visit_core_instance(instance.map_err(malformed)?)?;
                }
            }
            Payload::ComponentInstanceSection(reader) => {
                for instance in reader {
                    self.visit_component_instance(instance.map_err(malformed)?)?;
                }
            }
            Payload::CoreTypeSection(reader) => {
                self.component_items.core_types += reader.count() as usize;
                ensure_component_item_cap("core types", self.component_items.core_types)?;
            }
            Payload::ComponentTypeSection(reader) => {
                self.component_items.types += reader.count() as usize;
                ensure_component_item_cap("types", self.component_items.types)?;
            }
            Payload::ComponentAliasSection(reader) => {
                for alias in reader {
                    let alias = alias.map_err(malformed)?;
                    self.component_items.aliases += 1;
                    ensure_component_item_cap("aliases", self.component_items.aliases)?;
                    self.declare_aliased_item(&alias)?;
                }
            }
            Payload::ComponentCanonicalSection(reader) => {
                self.component_items.canonical_functions += reader.count() as usize;
                ensure_component_item_cap(
                    "canonical functions",
                    self.component_items.canonical_functions,
                )?;
            }
            Payload::ComponentImportSection(reader) => {
                for import in reader {
                    let import = import.map_err(malformed)?;
                    self.component_items.imports += 1;
                    ensure_component_item_cap("imports", self.component_items.imports)?;
                    match import.ty {
                        ComponentTypeRef::Module(_) => self.declare_core_module(None)?,
                        ComponentTypeRef::Component(_) => self.declare_component(None)?,
                        _ => {}
                    }
                }
            }
            Payload::ComponentExportSection(reader) => {
                self.component_items.exports += reader.count() as usize;
                ensure_component_item_cap("exports", self.component_items.exports)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Opens one nesting level and checks the component-wide caps.
    fn enter(&mut self, encoding: Encoding) -> Result<()> {
        match encoding {
            Encoding::Module => {
                self.core_modules += 1;
                if self.core_modules > MAX_CORE_MODULES {
                    return Err(policy_rejection(format!(
                        "note codec component has {} core modules; the limit is {MAX_CORE_MODULES}",
                        self.core_modules
                    )));
                }
                self.frames.push(Frame::Module(ModuleCounts::default()));
            }
            Encoding::Component => {
                self.frames.push(Frame::Component(ComponentFrame::default()));
                let depth =
                    self.frames.iter().filter(|frame| matches!(frame, Frame::Component(_))).count();
                if depth > MAX_COMPONENT_DEPTH {
                    return Err(policy_rejection(format!(
                        "note codec component nests components {depth} deep; the limit is \
                         {MAX_COMPONENT_DEPTH}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Closes one nesting level and records what it creates in the component that declares it.
    fn leave(&mut self) -> Result<()> {
        match self.frames.pop() {
            Some(Frame::Module(counts)) => {
                counts.check()?;
                // The module now holds an index in the core module index space of the component
                // that declares it.
                self.declare_core_module(Some(counts.runtime))
            }
            Some(Frame::Component(frame)) => self.declare_component(Some(frame.created)),
            // An unbalanced end cannot happen: the parser reports one `End` for every header it
            // accepted.
            None => Ok(()),
        }
    }

    /// Counts one core instantiation and what it creates.
    fn visit_core_instance(&mut self, instance: Instance<'_>) -> Result<()> {
        // An instance built from exports names items other instances already created.
        let created = match instance {
            Instance::Instantiate { module_index, .. } => {
                let module = self.instantiated_core_module(module_index)?;
                CreatedCounts {
                    core_instances: 1,
                    component_instances: 0,
                    memories: module.memories,
                    tables: module.tables,
                }
            }
            Instance::FromExports(_) => CreatedCounts {
                core_instances: 1,
                ..CreatedCounts::default()
            },
        };
        self.record_created(created)
    }

    /// Counts one component instantiation and what it creates.
    fn visit_component_instance(&mut self, instance: ComponentInstance<'_>) -> Result<()> {
        // An instance built from exports names items other instances already created.
        let ComponentInstance::Instantiate {
            component_index, ..
        } = instance
        else {
            return Ok(());
        };
        let mut created = self.instantiated_component(component_index)?;
        created.component_instances = created.component_instances.saturating_add(1);
        self.record_created(created)
    }

    /// Adds what one instantiation creates to the component the walk is inside.
    fn record_created(&mut self, created: CreatedCounts) -> Result<()> {
        let frame = self.component_frame()?;
        frame.created.add(created);
        frame.created.check()
    }

    /// Returns what one instantiation of a core module of the current component creates.
    fn instantiated_core_module(&mut self, module_index: u32) -> Result<ModuleRuntimeCounts> {
        let frame = self.component_frame()?;
        frame.core_modules.get(module_index as usize).copied().flatten().ok_or_else(|| {
            policy_rejection(format!(
                "note codec component instantiates core module {module_index}, which the policy \
                 cannot read; a codec instantiates only the core modules it defines"
            ))
        })
    }

    /// Returns what one instantiation of a nested component of the current component creates.
    fn instantiated_component(&mut self, component_index: u32) -> Result<CreatedCounts> {
        let frame = self.component_frame()?;
        frame
            .components
            .get(component_index as usize)
            .copied()
            .flatten()
            .ok_or_else(|| {
                policy_rejection(format!(
                    "note codec component instantiates component {component_index}, which the \
                     policy cannot read; a codec instantiates only the components it defines"
                ))
            })
    }

    /// Adds one core module to the index space of the component the walk is inside.
    ///
    /// A core module at the top level belongs to no component index space, so it is dropped.
    fn declare_core_module(&mut self, created: Option<ModuleRuntimeCounts>) -> Result<()> {
        if let Some(Frame::Component(frame)) = self.frames.last_mut() {
            frame.core_modules.push(created);
        }
        Ok(())
    }

    /// Adds one component to the index space of the component the walk is inside.
    ///
    /// The root component belongs to no component index space, so it is dropped.
    fn declare_component(&mut self, created: Option<CreatedCounts>) -> Result<()> {
        if let Some(Frame::Component(frame)) = self.frames.last_mut() {
            frame.components.push(created);
        }
        Ok(())
    }

    /// Adds one aliased core module or component to the current index spaces.
    fn declare_aliased_item(&mut self, alias: &ComponentAlias<'_>) -> Result<()> {
        match aliased_item_kind(alias) {
            Some(AliasedItem::CoreModule) => self.declare_core_module(None),
            Some(AliasedItem::Component) => self.declare_component(None),
            None => Ok(()),
        }
    }

    /// Returns the frame of the component the walk is inside.
    ///
    /// Component sections appear only inside a component. A component section anywhere else is a
    /// malformed layout, and the walk fails closed instead of guessing a frame for it.
    fn component_frame(&mut self) -> Result<&mut ComponentFrame> {
        match self.frames.last_mut() {
            Some(Frame::Component(frame)) => Ok(frame),
            _ => Err(policy_rejection(
                "note codec component is malformed: a component section appears outside a \
                 component",
            )),
        }
    }

    /// Returns the counters of the core module the walk is inside.
    ///
    /// Core sections appear only inside a core module. A core section anywhere else is a
    /// malformed layout, and the walk fails closed instead of guessing a frame for it.
    fn module_counts(&mut self) -> Result<&mut ModuleCounts> {
        match self.frames.last_mut() {
            Some(Frame::Module(counts)) => Ok(counts),
            _ => Err(policy_rejection(
                "note codec component is malformed: a core section appears outside a core module",
            )),
        }
    }
}

/// An index space one alias adds an item to.
enum AliasedItem {
    CoreModule,
    Component,
}

/// Returns the index space one alias adds an item to, if the walk tracks that space.
fn aliased_item_kind(alias: &ComponentAlias<'_>) -> Option<AliasedItem> {
    match alias {
        ComponentAlias::InstanceExport {
            kind: ComponentExternalKind::Module,
            ..
        }
        | ComponentAlias::Outer {
            kind: ComponentOuterAliasKind::CoreModule,
            ..
        } => Some(AliasedItem::CoreModule),
        ComponentAlias::InstanceExport {
            kind: ComponentExternalKind::Component,
            ..
        }
        | ComponentAlias::Outer {
            kind: ComponentOuterAliasKind::Component,
            ..
        } => Some(AliasedItem::Component),
        _ => None,
    }
}

/// Reports a core module that is over one of its caps.
fn ensure_module_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(policy_rejection(format!(
            "note codec component has a core module with {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a component that creates too much when it is instantiated.
fn ensure_created_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(policy_rejection(format!(
            "note codec component creates {observed} {kind} when it is instantiated; the limit is \
             {limit}"
        )));
    }
    Ok(())
}

/// Reports a core function type that is over its parameter or result cap.
fn ensure_signature_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(policy_rejection(format!(
            "note codec component has a function type with {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a component tree that is over the cap for one kind of component-level item.
fn ensure_component_item_cap(kind: &str, observed: usize) -> Result<()> {
    if observed > MAX_COMPONENT_SECTION_ITEMS {
        return Err(policy_rejection(format!(
            "note codec component has {observed} component {kind}; the limit is \
             {MAX_COMPONENT_SECTION_ITEMS}"
        )));
    }
    Ok(())
}

/// Reports a code section built from many tiny functions.
fn ensure_average_function_size(count: u32, size: u32) -> Result<()> {
    if size < MIN_METERED_CODE_SECTION_BYTES || count == 0 {
        return Ok(());
    }
    let average = size / count;
    if average < MIN_AVERAGE_FUNCTION_BYTES {
        return Err(policy_rejection(format!(
            "note codec component has a core module with {count} functions in {size} bytes of \
             code, an average of {average} bytes; the limit is {MIN_AVERAGE_FUNCTION_BYTES} bytes \
             per function"
        )));
    }
    Ok(())
}

/// Reports bytes that do not parse as a component.
fn malformed(error: wasmparser::BinaryReaderError) -> Error {
    policy_rejection(format!("note codec component is malformed: {error}"))
}

/// Reports a component that does not validate under [`NOTE_CODEC_WASM_FEATURES`].
fn rejected_feature(error: wasmparser::BinaryReaderError) -> Error {
    policy_rejection(format!(
        "note codec component uses a Wasm feature the policy rejects: {error}"
    ))
}

/// Creates an error for a component the structural load policy does not admit.
///
/// Every rejection carries one class, so a consumer reports a codec the policy refused the same
/// way it reports a codec that ran past a host limit.
fn policy_rejection(message: impl Into<String>) -> Error {
    Error::codec(CodecFailure::LimitExceeded, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps core module text in a component and assembles it.
    fn component(core_module: &str) -> Vec<u8> {
        wat::parse_str(format!("(component (core module {core_module}))")).unwrap()
    }

    /// Validates one component with a byte budget that never trips.
    fn validate(component: &[u8]) -> Result<()> {
        validate_note_codec_component(component, usize::MAX)
    }

    #[test]
    fn minimal_component_passes() {
        validate(&component("(func)")).unwrap();
    }

    #[test]
    fn the_wasm_feature_policy_is_exact() {
        // Destructuring is exhaustive on purpose: a wasmparser upgrade that adds a proposal
        // fails to compile here, so the policy cannot widen without a decision.
        let wasmparser::WasmFeaturesInflated {
            mutable_global,
            saturating_float_to_int,
            sign_extension,
            reference_types,
            multi_value,
            bulk_memory,
            simd,
            relaxed_simd,
            threads,
            shared_everything_threads,
            tail_call,
            floats,
            multi_memory,
            exceptions,
            memory64,
            extended_const,
            component_model,
            function_references,
            memory_control,
            gc,
            custom_page_sizes,
            legacy_exceptions,
            gc_types,
            stack_switching,
            wide_arithmetic,
            cm_values,
            cm_nested_names,
            cm_async,
            cm_async_stackful,
            cm_more_async_builtins,
            cm_threading,
            cm_error_context,
            cm_fixed_length_lists,
            cm_gc,
            call_indirect_overlong,
            bulk_memory_opt,
            custom_descriptors,
            compact_imports,
            cm_map,
            cm64,
        } = NOTE_CODEC_WASM_FEATURES.inflate();

        for (name, required) in [
            ("component model", component_model),
            ("floats", floats),
            ("mutable globals", mutable_global),
            ("saturating float to int", saturating_float_to_int),
            ("sign extension", sign_extension),
            ("reference types", reference_types),
            ("call-indirect overlong", call_indirect_overlong),
            ("multi value", multi_value),
            ("bulk memory", bulk_memory),
            ("bulk memory opt", bulk_memory_opt),
        ] {
            assert!(required, "the policy must accept {name}");
        }

        for (name, rejected) in [
            ("simd", simd),
            ("relaxed simd", relaxed_simd),
            ("threads", threads),
            ("shared everything threads", shared_everything_threads),
            ("tail call", tail_call),
            ("multi memory", multi_memory),
            ("exceptions", exceptions),
            ("legacy exceptions", legacy_exceptions),
            ("memory64", memory64),
            ("extended const", extended_const),
            ("function references", function_references),
            ("memory control", memory_control),
            ("gc", gc),
            ("gc types", gc_types),
            ("custom page sizes", custom_page_sizes),
            ("stack switching", stack_switching),
            ("wide arithmetic", wide_arithmetic),
            ("component model values", cm_values),
            ("component model nested names", cm_nested_names),
            ("component model async", cm_async),
            ("component model stackful async", cm_async_stackful),
            ("component model async builtins", cm_more_async_builtins),
            ("component model threading", cm_threading),
            ("component model error context", cm_error_context),
            ("component model fixed length lists", cm_fixed_length_lists),
            ("component model gc", cm_gc),
            ("component model maps", cm_map),
            ("component model 64-bit contexts", cm64),
            ("custom descriptors", custom_descriptors),
            ("compact imports", compact_imports),
        ] {
            assert!(!rejected, "the policy must reject {name}");
        }
    }

    #[test]
    fn oversized_components_are_rejected_before_parsing() {
        let error = validate_note_codec_component(&[0; 8], 4).unwrap_err().to_string();

        assert!(error.contains("is 8 bytes"), "unexpected error: {error}");
        assert!(error.contains("the pre-compilation limit is 4"), "unexpected error: {error}");
    }

    #[test]
    fn too_many_globals_are_rejected() {
        let globals = "(global i32 (i32.const 0))".repeat(MAX_MODULE_GLOBALS + 1);
        let error = validate(&component(&globals)).unwrap_err().to_string();

        assert!(error.contains("1001 globals"), "unexpected error: {error}");
        assert!(error.contains("the limit is 1000"), "unexpected error: {error}");
    }

    #[test]
    fn oversized_function_signatures_are_rejected() {
        let params = "i32 ".repeat(MAX_FUNCTION_PARAMS + 1);
        let module = format!("(type (func (param {params})))");
        let error = validate(&component(&module)).unwrap_err().to_string();

        assert!(error.contains("33 parameters"), "unexpected error: {error}");
        assert!(error.contains("the limit is 32"), "unexpected error: {error}");
    }

    #[test]
    fn many_tiny_functions_are_rejected() {
        let error = validate(&component(&"(func)".repeat(600))).unwrap_err().to_string();

        assert!(error.contains("600 functions"), "unexpected error: {error}");
        assert!(error.contains("bytes per function"), "unexpected error: {error}");
    }

    #[test]
    fn malformed_bytes_are_rejected() {
        let error = validate(b"not a component").unwrap_err().to_string();

        assert!(error.contains("note codec component is malformed"), "unexpected error: {error}");
    }

    #[test]
    fn what_a_nested_component_creates_is_counted_once_per_instantiation() {
        // One instantiation of the nested component creates one memory, so two instantiations
        // reach the memory budget. A nested component that is never instantiated creates
        // nothing.
        let text = "(component
                (component $inner
                    (core module $m (memory 1))
                    (core instance (instantiate $m)))
                (instance (instantiate $inner))
                (instance (instantiate $inner)))";
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(
            error.contains("creates 2 linear memories when it is instantiated"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_INSTANTIATED_MEMORIES}")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_component_that_is_never_instantiated_creates_nothing() {
        let text = "(component
                (component $unused
                    (core module $m (memory 1))
                    (core instance (instantiate $m)))
                (core module $m (memory 1))
                (core instance (instantiate $m)))";

        validate(&wat::parse_str(text).unwrap()).unwrap();
    }

    #[test]
    fn too_many_component_instances_are_rejected() {
        let instantiate = "(instance (instantiate $inner))".repeat(MAX_COMPONENT_INSTANCES + 1);
        let text = format!("(component (component $inner) {instantiate})");
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(
            error.contains(&format!("creates {} component instances", MAX_COMPONENT_INSTANCES + 1)),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn instantiated_memories_are_counted_across_core_modules() {
        let text = "(component
                (core module $a (memory 1))
                (core module $b (memory 1))
                (core instance (instantiate $a))
                (core instance (instantiate $b)))";
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(
            error.contains("creates 2 linear memories when it is instantiated"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_INSTANTIATED_MEMORIES}")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn one_module_instantiated_twice_is_counted_twice() {
        // The budgets count what instantiation creates, not what the tree declares, so one
        // memory-defining module reaches the memory budget when it is instantiated twice.
        let text = r#"(component
                (core module $m (memory 1) (func (export "f")))
                (core instance $a (instantiate $m))
                (core instance $b (instantiate $m)))"#;
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(
            error.contains("creates 2 linear memories when it is instantiated"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_core_module_the_walk_cannot_read_is_not_instantiable() {
        let text = r#"(component
                (import "m" (core module $m))
                (core instance (instantiate $m)))"#;
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(error.contains("which the policy cannot read"), "unexpected error: {error}");
    }

    #[test]
    fn component_items_of_one_kind_are_capped_across_sections() {
        // Component sections repeat, so the cap counts one kind over the whole tree. A core
        // module between the two type sections keeps them apart in the encoding.
        let types = |count: usize| "(type u8)".repeat(count);
        let text = format!(
            "(component {first} (core module) {second})",
            first = types(MAX_COMPONENT_SECTION_ITEMS),
            second = types(1),
        );
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(
            error.contains(&format!("{} component types", MAX_COMPONENT_SECTION_ITEMS + 1)),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(&format!("the limit is {MAX_COMPONENT_SECTION_ITEMS}")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn too_many_module_types_are_rejected() {
        let types = "(type (func (param i32)))".repeat(MAX_MODULE_TYPES + 1);
        let error = validate(&component(&types)).unwrap_err().to_string();

        assert!(
            error.contains(&format!("{} types", MAX_MODULE_TYPES + 1)),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn every_structural_rejection_carries_a_class() {
        let error = validate(b"not a component").unwrap_err();

        assert_eq!(error.codec_failure(), Some(crate::CodecFailure::LimitExceeded));
    }

    #[test]
    fn component_start_functions_are_rejected() {
        let text = r#"(component
            (core module $m (func (export "f")))
            (core instance $i (instantiate $m))
            (func $f (canon lift (core func $i "f")))
            (start $f))"#;
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(error.contains("declares a start function"), "unexpected error: {error}");
    }

    #[cfg(feature = "codec-component")]
    #[test]
    fn fixture_component_passes() {
        if !midenc_integration_test_support::wasm_target_is_installed() {
            eprintln!("skipping the structural limit fixture test: wasm32-wasip2 is not installed");
            return;
        }

        validate(&crate::codec_component::tests::build_fixture_component()).unwrap();
    }
}
