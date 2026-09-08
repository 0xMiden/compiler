//! Structural limits for note codec components.
//!
//! The limits bound the work that parsing, compilation, and instantiation cost. That work
//! happens before any fuel budget applies, so a size cap alone does not bound it. A small
//! binary can still declare thousands of tiny functions, thousands of globals, or a deep
//! component tree.
//!
//! The rules are fixed policy. The producer applies them when it attaches a codec, and every
//! consumer applies them when it loads one. The numbers follow the strict compilation limits
//! that the Miden VM Wasm event handler runner enforces.

use wasmparser::{Encoding, Parser, Payload, TypeRef};

use crate::{Error, Result};

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

/// Maximum entries in one component import or export section.
const MAX_COMPONENT_SECTION_ITEMS: usize = 256;

/// Rejects a note codec component whose structure would make compilation or instantiation
/// expensive before any fuel applies. The rules are fixed policy, not host configuration:
/// the producer applies them at build time and every consumer applies them at load time.
pub fn validate_note_codec_structure(component: &[u8]) -> Result<()> {
    let mut walk = StructureWalk::default();
    for payload in Parser::new(0).parse_all(component) {
        walk.visit(payload.map_err(malformed)?)?;
    }
    Ok(())
}

/// The state carried while the parser walks one component.
#[derive(Default)]
struct StructureWalk {
    /// One frame per core module or component the walk entered, innermost last.
    frames: Vec<Frame>,
    /// Core modules seen anywhere in the component.
    core_modules: usize,
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
                for ty in reader.into_iter_err_on_gc_types() {
                    let ty = ty.map_err(malformed)?;
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
                self.module_counts()?.tables += reader.count() as usize;
            }
            Payload::MemorySection(reader) => {
                self.module_counts()?.memories += reader.count() as usize;
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

/// Reports a core function type that is over its parameter or result cap.
fn ensure_signature_cap(kind: &str, observed: usize, limit: usize) -> Result<()> {
    if observed > limit {
        return Err(Error::new(format!(
            "note codec component has a function type with {observed} {kind}; the limit is {limit}"
        )));
    }
    Ok(())
}

/// Reports a component import or export section that is over its cap.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps core module text in a component and assembles it.
    fn component(core_module: &str) -> Vec<u8> {
        wat::parse_str(format!("(component (core module {core_module}))")).unwrap()
    }

    #[test]
    fn minimal_component_passes() {
        validate_note_codec_structure(&component("(func)")).unwrap();
    }

    #[test]
    fn too_many_globals_are_rejected() {
        let globals = "(global i32 (i32.const 0))".repeat(MAX_MODULE_GLOBALS + 1);
        let error = validate_note_codec_structure(&component(&globals)).unwrap_err().to_string();

        assert!(error.contains("1001 globals"), "unexpected error: {error}");
        assert!(error.contains("the limit is 1000"), "unexpected error: {error}");
    }

    #[test]
    fn oversized_function_signatures_are_rejected() {
        let params = "i32 ".repeat(MAX_FUNCTION_PARAMS + 1);
        let module = format!("(type (func (param {params})))");
        let error = validate_note_codec_structure(&component(&module)).unwrap_err().to_string();

        assert!(error.contains("33 parameters"), "unexpected error: {error}");
        assert!(error.contains("the limit is 32"), "unexpected error: {error}");
    }

    #[test]
    fn many_tiny_functions_are_rejected() {
        let error = validate_note_codec_structure(&component(&"(func)".repeat(600)))
            .unwrap_err()
            .to_string();

        assert!(error.contains("600 functions"), "unexpected error: {error}");
        assert!(error.contains("bytes per function"), "unexpected error: {error}");
    }

    #[test]
    fn malformed_bytes_are_rejected() {
        let error = validate_note_codec_structure(b"not a component").unwrap_err().to_string();

        assert!(error.contains("note codec component is malformed"), "unexpected error: {error}");
    }

    #[cfg(feature = "codec-component")]
    #[test]
    fn fixture_component_passes() {
        if !midenc_integration_test_support::wasm_target_is_installed() {
            eprintln!("skipping the structural limit fixture test: wasm32-wasip2 is not installed");
            return;
        }

        validate_note_codec_structure(&crate::codec_component::tests::build_fixture_component())
            .unwrap();
    }
}
