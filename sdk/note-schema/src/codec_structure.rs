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
//! - the component declares no start function;
//! - each core module stays under its own counts, and its code section keeps a plausible
//!   average function size;
//! - the component tree stays under its budgets for core modules, nesting depth, core
//!   instantiations, component instantiations, defined tables, and defined memories;
//! - each component-level section stays under its width cap.
//!
//! Nothing else is checked here. The producer checks the exported codec interface, and a
//! consumer bounds the run time of a codec call with fuel and store limits.

use wasmparser::{
    Encoding, FuncValidatorAllocations, Parser, Payload, TypeRef, ValidPayload, Validator,
    WasmFeatures,
};

use crate::{Error, Result};

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

/// Maximum tables in one core module, imported and defined.
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

/// Maximum entries in one component-level section.
const MAX_COMPONENT_SECTION_ITEMS: usize = 256;

/// Maximum core instantiations in the whole component tree.
///
/// The consumer store admits the same number of instances. The budget is tree-wide because a
/// nested component is expanded once per instantiation of its parent, so per-level budgets
/// multiply.
pub(crate) const MAX_CORE_INSTANCES: usize = 32;

/// Maximum component instantiations in the whole component tree.
///
/// A `wasm32-wasip2` codec instantiates one component instance per exported interface.
pub(crate) const MAX_COMPONENT_INSTANCES: usize = 8;

/// Maximum tables defined in the whole component tree.
///
/// The consumer store admits the same number of tables.
pub(crate) const MAX_DEFINED_TABLES: usize = 32;

/// Maximum linear memories defined in the whole component tree.
///
/// The consumer store admits the same number of memories.
pub(crate) const MAX_DEFINED_MEMORIES: usize = 1;

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
            return Err(Error::new(
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
    let message =
        format!("note codec component is {byte_len} bytes; the pre-compilation limit is {limit}");
    // A consumer classifies the byte cap like every other cap it applies to a codec. Without the
    // consumer adapter there is no failure class to report.
    #[cfg(feature = "codec-component")]
    let error = Error::codec(crate::CodecFailure::LimitExceeded, message);
    #[cfg(not(feature = "codec-component"))]
    let error = Error::new(message);
    Err(error)
}

/// The state carried while the parser walks one component.
#[derive(Default)]
struct StructureWalk {
    /// One frame per core module or component the walk entered, innermost last.
    frames: Vec<Frame>,
    /// Core modules seen anywhere in the component.
    core_modules: usize,
    /// Core instantiations declared anywhere in the component.
    core_instances: usize,
    /// Component instantiations declared anywhere in the component.
    component_instances: usize,
    /// Tables defined anywhere in the component, excluding imported tables.
    defined_tables: usize,
    /// Linear memories defined anywhere in the component, excluding imported memories.
    defined_memories: usize,
}

/// One nesting level of the walk.
enum Frame {
    /// A core module, with the counters checked when the module ends.
    Module(ModuleCounts),
    /// A component, counted only for the nesting depth.
    Component,
}

/// Counters collected for one core module.
#[derive(Default)]
struct ModuleCounts {
    functions: usize,
    globals: usize,
    tables: usize,
    memories: usize,
    element_segments: usize,
    data_segments: usize,
    imports: usize,
    exports: usize,
}

impl ModuleCounts {
    /// Checks every per-module cap once the module ends.
    fn check(&self) -> Result<()> {
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
                self.module_counts()?.tables += count;
                self.defined_tables += count;
                ensure_tree_cap("defined tables", self.defined_tables, MAX_DEFINED_TABLES)?;
            }
            Payload::MemorySection(reader) => {
                let count = reader.count() as usize;
                self.module_counts()?.memories += count;
                self.defined_memories += count;
                ensure_tree_cap("defined memories", self.defined_memories, MAX_DEFINED_MEMORIES)?;
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
                self.core_instances += reader.count() as usize;
                ensure_tree_cap("core instantiations", self.core_instances, MAX_CORE_INSTANCES)?;
            }
            Payload::ComponentInstanceSection(reader) => {
                self.component_instances += reader.count() as usize;
                ensure_tree_cap(
                    "component instantiations",
                    self.component_instances,
                    MAX_COMPONENT_INSTANCES,
                )?;
            }
            Payload::CoreTypeSection(reader) => {
                ensure_component_section_cap("core types", reader.count() as usize)?;
            }
            Payload::ComponentTypeSection(reader) => {
                ensure_component_section_cap("types", reader.count() as usize)?;
            }
            Payload::ComponentAliasSection(reader) => {
                ensure_component_section_cap("aliases", reader.count() as usize)?;
            }
            Payload::ComponentCanonicalSection(reader) => {
                ensure_component_section_cap("canonical functions", reader.count() as usize)?;
            }
            Payload::ComponentImportSection(reader) => {
                ensure_component_section_cap("imports", reader.count() as usize)?;
            }
            Payload::ComponentExportSection(reader) => {
                ensure_component_section_cap("exports", reader.count() as usize)?;
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
                    return Err(Error::new(format!(
                        "note codec component has {} core modules; the limit is {MAX_CORE_MODULES}",
                        self.core_modules
                    )));
                }
                self.frames.push(Frame::Module(ModuleCounts::default()));
            }
            Encoding::Component => {
                self.frames.push(Frame::Component);
                let depth =
                    self.frames.iter().filter(|frame| matches!(frame, Frame::Component)).count();
                if depth > MAX_COMPONENT_DEPTH {
                    return Err(Error::new(format!(
                        "note codec component nests components {depth} deep; the limit is \
                         {MAX_COMPONENT_DEPTH}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Closes one nesting level and checks the counters of a core module.
    fn leave(&mut self) -> Result<()> {
        match self.frames.pop() {
            Some(Frame::Module(counts)) => counts.check(),
            // A component frame carries no counters, and an unbalanced end cannot happen:
            // the parser reports one `End` for every header it accepted.
            _ => Ok(()),
        }
    }

    /// Returns the counters of the core module the walk is inside.
    ///
    /// Core sections appear only inside a core module. A core section anywhere else is a
    /// malformed layout, and the walk fails closed instead of guessing a frame for it.
    fn module_counts(&mut self) -> Result<&mut ModuleCounts> {
        match self.frames.last_mut() {
            Some(Frame::Module(counts)) => Ok(counts),
            _ => Err(Error::new(
                "note codec component is malformed: a core section appears outside a core module",
            )),
        }
    }
}

/// Reports a core module that is over one of its caps.
fn ensure_module_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(Error::new(format!(
            "note codec component has a core module with {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a component tree that is over one of its whole-tree budgets.
fn ensure_tree_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(Error::new(format!(
            "note codec component has {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a core function type that is over its parameter or result cap.
fn ensure_signature_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(Error::new(format!(
            "note codec component has a function type with {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a component-level section that is over its width cap.
fn ensure_component_section_cap(kind: &str, observed: usize) -> Result<()> {
    if observed > MAX_COMPONENT_SECTION_ITEMS {
        return Err(Error::new(format!(
            "note codec component has a section with {observed} component {kind}; the limit is \
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
        return Err(Error::new(format!(
            "note codec component has a core module with {count} functions in {size} bytes of \
             code, an average of {average} bytes; the limit is {MIN_AVERAGE_FUNCTION_BYTES} bytes \
             per function"
        )));
    }
    Ok(())
}

/// Reports bytes that do not parse as a component.
fn malformed(error: wasmparser::BinaryReaderError) -> Error {
    Error::new(format!("note codec component is malformed: {error}"))
}

/// Reports a component that does not validate under [`NOTE_CODEC_WASM_FEATURES`].
fn rejected_feature(error: wasmparser::BinaryReaderError) -> Error {
    Error::new(format!("note codec component uses a Wasm feature the policy rejects: {error}"))
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
    fn nested_component_instantiations_are_rejected() {
        // Three nesting levels that each instantiate the level below 300 times. A per-level cap
        // would still admit 300^3 expansions, so the budget counts the whole tree.
        let instantiate = |name: &str| format!("(instance (instantiate {name}))").repeat(300);
        let text = format!(
            "(component
                (component $outer
                    (component $middle
                        (component $inner (core module))
                        {inner_uses})
                    {middle_uses})
                {outer_uses})",
            inner_uses = instantiate("$inner"),
            middle_uses = instantiate("$middle"),
            outer_uses = instantiate("$outer"),
        );
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(error.contains("component instantiations"), "unexpected error: {error}");
        assert!(
            error.contains(&format!("the limit is {MAX_COMPONENT_INSTANCES}")),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn defined_memories_are_counted_across_core_modules() {
        let text = "(component (core module (memory 1)) (core module (memory 1)))";
        let error = validate(&wat::parse_str(text).unwrap()).unwrap_err().to_string();

        assert!(error.contains("2 defined memories"), "unexpected error: {error}");
        assert!(
            error.contains(&format!("the limit is {MAX_DEFINED_MEMORIES}")),
            "unexpected error: {error}"
        );
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
