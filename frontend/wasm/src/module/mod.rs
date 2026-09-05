//! Data structures for representing parsed Wasm modules.

use alloc::{borrow::Cow, collections::BTreeMap};
use core::{fmt, ops::Range, str::FromStr};

use cranelift_entity::{EntityRef, PrimaryMap, packed_option::ReservedValue};
use indexmap::IndexMap;
use midenc_hir::{FunctionIdent, FxHashMap, FxHashSet, Ident, SymbolPath, interner::Symbol};
use midenc_session::DiagnosticsHandler;

use self::types::*;
use crate::{
    component::SignatureIndex, error::WasmResult, intrinsics::Intrinsic,
    miden_abi::is_miden_abi_module, unsupported_diag,
};

pub mod build_ir;
pub mod debug_info;
pub mod func_translation_state;
pub mod func_translator;
pub mod function_builder_ext;
pub mod instance;
pub mod linker_stubs;
pub mod module_env;
pub mod module_translation_state;
pub mod types;

/// Table initialization data for all tables in the module.
#[derive(Debug, Default)]
pub struct TableInitialization {
    /// Initial values for tables defined within the module itself.
    ///
    /// This contains the initial values and initializers for tables defined
    /// within a wasm, so excluding imported tables. This initializer can
    /// represent null-initialized tables, element-initialized tables (e.g. with
    /// the function-references proposal), or precomputed images of table
    /// initialization. For example table initializers to a table that are all
    /// in-bounds will get removed from `segment` and moved into
    /// `initial_values` here.
    pub initial_values: PrimaryMap<DefinedTableIndex, TableInitialValue>,

    /// Element segments present in the initial wasm module which are executed
    /// at instantiation time.
    ///
    /// These element segments are iterated over during instantiation to apply
    /// any segments that weren't already moved into `initial_values` above.
    pub segments: Vec<TableSegment>,
}

/// Initial value for all elements in a table.
#[derive(Clone, Debug)]
pub enum TableInitialValue {
    /// Initialize each table element to null, optionally setting some elements
    /// to non-null given the precomputed image.
    Null {
        /// A precomputed image of table initializers for this table.
        precomputed: Vec<FuncIndex>,
    },

    /// Initialize each table element to the function reference given
    /// by the `FuncIndex`.
    FuncRef(FuncIndex),
}

/// A WebAssembly table initializer segment.
#[derive(Clone, Debug)]
pub struct TableSegment {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: u32,
    /// The values to write into the table elements.
    pub elements: Box<[FuncIndex]>,
}

/// Different types that can appear in a module.
///
/// Note that each of these variants are intended to index further into a
/// separate table.
#[derive(Debug, Copy, Clone)]
pub enum ModuleType {
    Function(SignatureIndex),
}

impl ModuleType {
    /// Asserts this is a `ModuleType::Function`, returning the underlying
    /// `SignatureIndex`.
    pub fn unwrap_function(&self) -> SignatureIndex {
        match self {
            ModuleType::Function(f) => *f,
        }
    }
}

/// A translated WebAssembly module, excluding the function bodies
#[derive(Default, Debug)]
pub struct Module {
    /// All import records, in the order they are declared in the module.
    pub imports: Vec<ModuleImport>,

    /// Exported entities.
    pub exports: IndexMap<String, EntityIndex>,

    /// The module "start" function, if present.
    pub start_func: Option<FuncIndex>,

    /// WebAssembly table initialization data, per table.
    pub table_initialization: TableInitialization,

    /// WebAssembly passive elements.
    pub passive_elements: Vec<Box<[FuncIndex]>>,

    /// The map from passive element index (element segment index space) to index in
    /// `passive_elements`.
    pub passive_elements_map: BTreeMap<ElemIndex, usize>,

    /// The map from passive data index (data segment index space) to index in `passive_data`.
    pub passive_data_map: BTreeMap<DataIndex, Range<u32>>,

    /// Types declared in the wasm module.
    pub types: PrimaryMap<TypeIndex, ModuleType>,

    /// Number of imported or aliased functions in the module.
    pub num_imported_funcs: usize,

    /// Number of imported or aliased tables in the module.
    pub num_imported_tables: usize,

    /// Number of imported or aliased globals in the module.
    pub num_imported_globals: usize,

    /// Number of functions that "escape" from this module
    ///
    /// This is also the number of functions in the `functions` array below with
    /// an `func_ref` index (and is the maximum func_ref index).
    pub num_escaped_funcs: usize,

    /// Types of functions, imported and local.
    pub functions: PrimaryMap<FuncIndex, FunctionTypeInfo>,

    /// WebAssembly tables.
    pub tables: PrimaryMap<TableIndex, Table>,

    /// WebAssembly global variables.
    pub globals: PrimaryMap<GlobalIndex, Global>,

    /// WebAssembly global initializers for locally-defined globals.
    pub global_initializers: PrimaryMap<DefinedGlobalIndex, GlobalInit>,

    /// WebAssembly module memories.
    pub memories: PrimaryMap<MemoryIndex, Memory>,

    /// Parsed names section.
    ///
    /// Wasm's [name section] may contain duplicate names. Therefore it is recommended to
    /// call [`Self::resolve_func_symbols`] after parsing and then get the unique function
    /// symbol with [`Self::func_name`].
    ///
    /// [name section]: https://webassembly.github.io/spec/core/appendix/custom.html#name-section
    name_section: NameSection,

    /// Linkage name per function. Linkage names are unique.
    ///
    /// Built by [`Self::resolve_func_symbols`].
    func_linkages: PrimaryMap<FuncIndex, Symbol>,

    /// Names in the name section that are shared by more than one function.
    duplicate_source_names: FxHashSet<Symbol>,

    /// The fallback name of this module, used if there is no module name in the name section,
    /// and there is no override specified
    name_fallback: Option<Ident>,

    /// If specified, overrides the name of the module regardless of what is in the name section
    name_override: Option<Ident>,
}

/// Module imports
#[derive(Debug, Clone)]
pub struct ModuleImport {
    /// Name of this import
    pub module: String,
    /// The field name projection of this import
    pub field: String,
    /// Where this import will be placed, which also has type information
    /// about the import.
    pub index: EntityIndex,
}

impl Module {
    /// Convert a `DefinedFuncIndex` into a `FuncIndex`.
    #[inline]
    pub fn func_index(&self, defined_func: DefinedFuncIndex) -> FuncIndex {
        FuncIndex::new(self.num_imported_funcs + defined_func.index())
    }

    /// Convert a `FuncIndex` into a `DefinedFuncIndex`. Returns None if the
    /// index is an imported function.
    #[inline]
    pub fn defined_func_index(&self, func: FuncIndex) -> Option<DefinedFuncIndex> {
        if func.index() < self.num_imported_funcs {
            None
        } else {
            Some(DefinedFuncIndex::new(func.index() - self.num_imported_funcs))
        }
    }

    /// Test whether the given function index is for an imported function.
    #[inline]
    pub fn is_imported_function(&self, index: FuncIndex) -> bool {
        index.index() < self.num_imported_funcs
    }

    pub fn is_exported(&self, entity: EntityIndex) -> bool {
        self.exports.values().any(|export| *export == entity)
    }

    /// Convert a `DefinedTableIndex` into a `TableIndex`.
    #[inline]
    pub fn table_index(&self, defined_table: DefinedTableIndex) -> TableIndex {
        TableIndex::new(self.num_imported_tables + defined_table.index())
    }

    /// Convert a `TableIndex` into a `DefinedTableIndex`. Returns None if the
    /// index is an imported table.
    #[inline]
    pub fn defined_table_index(&self, table: TableIndex) -> Option<DefinedTableIndex> {
        if table.index() < self.num_imported_tables {
            None
        } else {
            Some(DefinedTableIndex::new(table.index() - self.num_imported_tables))
        }
    }

    /// Test whether the given table index is for an imported table.
    #[inline]
    pub fn is_imported_table(&self, index: TableIndex) -> bool {
        index.index() < self.num_imported_tables
    }

    /// Test whether the given memory index is for an imported memory.
    #[inline]
    pub fn is_imported_memory(&self, index: MemoryIndex) -> bool {
        self.memories[index].imported
    }

    /// Convert a `DefinedGlobalIndex` into a `GlobalIndex`.
    #[inline]
    pub fn global_index(&self, defined_global: DefinedGlobalIndex) -> GlobalIndex {
        GlobalIndex::new(self.num_imported_globals + defined_global.index())
    }

    /// Convert a `GlobalIndex` into a `DefinedGlobalIndex`. Returns None if the
    /// index is an imported global.
    #[inline]
    pub fn defined_global_index(&self, global: GlobalIndex) -> Option<DefinedGlobalIndex> {
        if global.index() < self.num_imported_globals {
            None
        } else {
            Some(DefinedGlobalIndex::new(global.index() - self.num_imported_globals))
        }
    }

    /// Test whether the given global index is for an imported global.
    #[inline]
    pub fn is_imported_global(&self, index: GlobalIndex) -> bool {
        index.index() < self.num_imported_globals
    }

    pub fn global_name(&self, index: GlobalIndex) -> Symbol {
        self.name_section
            .globals_names
            .get(&index)
            .cloned()
            .unwrap_or(Symbol::intern(format!("global{}", index.as_u32()).as_str()))
    }

    /// Returns the type of an item based on its index
    pub fn type_of(&self, index: EntityIndex) -> EntityType {
        match index {
            EntityIndex::Global(i) => EntityType::Global(self.globals[i].clone()),
            EntityIndex::Table(i) => EntityType::Table(self.tables[i]),
            EntityIndex::Memory(i) => EntityType::Memory(self.memories[i]),
            EntityIndex::Function(i) => EntityType::Function(self.functions[i].signature),
        }
    }

    /// Appends a new function to this module with the given type information,
    /// used for functions that either don't escape or aren't certain whether
    /// they escape yet.
    pub fn push_function(&mut self, signature: SignatureIndex) -> FuncIndex {
        self.functions.push(FunctionTypeInfo {
            signature,
            func_ref: FuncRefIndex::reserved_value(),
        })
    }

    /// Appends a new function to this module with the given type information.
    pub fn push_escaped_function(
        &mut self,
        signature: SignatureIndex,
        func_ref: FuncRefIndex,
    ) -> FuncIndex {
        self.functions.push(FunctionTypeInfo {
            signature,
            func_ref,
        })
    }

    /// Returns the global initializer for the given index, or `Unsupported` error if the global is
    /// imported.
    pub fn try_global_initializer(
        &self,
        index: GlobalIndex,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<&GlobalInit> {
        if let Some(defined_index) = self.defined_global_index(index) {
            Ok(&self.global_initializers[defined_index])
        } else {
            unsupported_diag!(diagnostics, "Imported globals are not supported yet");
        }
    }

    /// Returns the name of this module
    pub fn name(&self) -> Ident {
        self.name_override
            .or(self.name_section.module_name)
            .or(self.name_fallback)
            .expect("No module name in the name section and no fallback name is set")
    }

    /// Returns the unique name of the given function
    pub fn func_name(&self, index: FuncIndex) -> Symbol {
        if let Some(sym) = self.func_linkages.get(index).copied() {
            return sym;
        }
        // Fallback for unnamed functions
        self.source_func_name(index)
    }

    /// Returns the name according to the name section.
    ///
    /// Use this when referring to the original source code, e.g. in diagnostics or debug info.
    ///
    /// The returned name might not be unique, see `Self::name_section`.
    pub fn source_func_name(&self, index: FuncIndex) -> Symbol {
        self.name_section
            .func_names
            .get(&index)
            .copied()
            .unwrap_or_else(|| Self::fallback_func_name(index))
    }

    /// Synthesized name for functions without a name-section entry (e.g. stripped binaries).
    // TODO check if there are more places that could use this
    fn fallback_func_name(index: FuncIndex) -> Symbol {
        Symbol::intern(format!("func{}", index.as_u32()))
    }

    /// Returns true if the function has an explicit entry in the name section.
    pub fn has_explicit_source_func_name(&self, index: FuncIndex) -> bool {
        self.name_section.func_names.contains_key(&index)
    }

    /// Returns true if the function's name-section name is shared with at least one other function.
    ///
    /// Requires [`Self::resolve_func_symbols`] to have run, which the Wasm frontend does during
    /// parsing.
    pub fn is_duplicate_source_func_name(&self, index: FuncIndex) -> bool {
        self.name_section
            .func_names
            .get(&index)
            .is_some_and(|name| self.duplicate_source_names.contains(name))
    }

    /// Resolves unique HIR linkage names for all functions in the module.
    ///
    /// WebAssembly function export names define the public interface and take precedence as
    /// the primary HIR linkage symbol. Unexported functions use their name-section name (or
    /// `func{index}` fallback if absent), disambiguated via `{name}_func{index}` (with `_` appended
    /// to resolve collisions) if they conflict with an export name, a global variable name, or
    /// another function with the same source name.
    ///
    /// Intrinsics and Miden ABI linker stubs are identified by name (see
    /// [`maybe_lower_linker_stub`]) and considered internal, so an export or duplicate name that
    /// identifies a known stub is an error.
    ///
    /// This method is idempotent.
    ///
    /// [name section]: https://webassembly.github.io/spec/core/appendix/custom.html#name-section
    /// [`maybe_lower_linker_stub`]: linker_stubs::maybe_lower_linker_stub
    pub fn resolve_func_symbols(&mut self, diagnostics: &DiagnosticsHandler) -> WasmResult<()> {
        self.func_linkages.clear();
        self.duplicate_source_names.clear();

        // Collect and validate function exports
        let mut exported_as: FxHashMap<FuncIndex, Symbol> = FxHashMap::default();
        let mut export_names: FxHashSet<Symbol> = FxHashSet::default();
        for (export_name, entity) in &self.exports {
            let EntityIndex::Function(func_idx) = entity else {
                continue;
            };
            let export_sym = Symbol::intern(export_name.as_str());

            if exported_as.insert(*func_idx, export_sym).is_some() {
                unsupported_diag!(
                    diagnostics,
                    "exporting a function under multiple names is not supported: function index \
                     `{}`, `{export_name}`)",
                    func_idx.as_u32()
                );
            }
            export_names.insert(export_sym);

            if let Ok(func_ident) = FunctionIdent::from_str(export_name.as_str()) {
                let path = SymbolPath::from_masm_function_id(func_ident);
                if Intrinsic::try_from(&path).is_ok() || is_miden_abi_module(&path) {
                    unsupported_diag!(
                        diagnostics,
                        "export name '{export_name}' identifies an intrinsic or Miden ABI linker \
                         stub, which cannot be exported"
                    );
                }
            }
        }

        // Reject collisions between export names and global variable names
        let mut global_names: FxHashSet<Symbol> = FxHashSet::default();
        for global_idx in self.globals.keys() {
            let name = self.global_name(global_idx);
            if export_names.contains(&name) {
                unsupported_diag!(
                    diagnostics,
                    "export name '{name}' conflicts with a global variable name"
                );
            }
            global_names.insert(name);
        }

        // Source names are explicit name-section names only; fallbacks (`func{index}`) are unique
        // by construction and handled below via `taken`. Counting explicit names ensures that
        // explicit names win over fallbacks.
        let mut counts: FxHashMap<Symbol, usize> = FxHashMap::default();
        for (func_idx, name) in &self.name_section.func_names {
            if func_idx.index() >= self.functions.len() {
                continue;
            }
            *counts.entry(*name).or_default() += 1;
        }
        for (name, count) in &counts {
            if *count > 1 {
                self.duplicate_source_names.insert(*name);
            }
        }

        // Collect source names that don't need to change. Only explicit (non-fallback) names
        // participate here. Fallbacks are handled below via `taken`.
        let mut keep_source_name: FxHashSet<Symbol> = FxHashSet::default();
        for (func_idx, name) in &self.name_section.func_names {
            if func_idx.index() >= self.functions.len() {
                continue;
            }
            if !exported_as.contains_key(func_idx)
                && counts.get(name) == Some(&1)
                && !export_names.contains(name)
                && !global_names.contains(name)
            {
                keep_source_name.insert(*name);
            }
        }

        // Exports take precedence and are never renamed.
        let mut taken: FxHashSet<Symbol> = export_names;
        taken.extend(global_names);
        taken.extend(keep_source_name.iter().copied());

        // Assign dense linkage names in deterministic `FuncIndex` order.
        let mut linkages: Vec<Option<Symbol>> = vec![None; self.functions.len()];
        for fidx in self.functions.keys() {
            let linkage = if let Some(export_name) = exported_as.get(&fidx) {
                // Export name must become linkage name for external calls to resolve.
                *export_name
            } else {
                // Not exported: linkage defaults to the source name. Fallbacks are unique among
                // themselves, but an explicit name may be `funcY` while `Y` is unnamed. Explicit
                // names win regardless of index order because `keep` is pre-seeded into `taken`, so
                // the fallback loses here and is renamed below.
                let (candidate, is_explicit) = match self.name_section.func_names.get(&fidx) {
                    Some(name) => (*name, true),
                    None => (Self::fallback_func_name(fidx), false),
                };
                let can_use_source_as_linkage = if is_explicit {
                    keep_source_name.contains(&candidate)
                } else if taken.contains(&candidate) {
                    false
                } else {
                    taken.insert(candidate);
                    true
                };

                if can_use_source_as_linkage {
                    candidate
                } else {
                    // Need to construct a unique linkage name.
                    let cand_str = candidate.as_str();
                    if let Ok(func_id) = FunctionIdent::from_str(cand_str) {
                        let path = SymbolPath::from_masm_function_id(func_id);
                        if Intrinsic::try_from(&path).is_ok() || is_miden_abi_module(&path) {
                            unsupported_diag!(
                                diagnostics,
                                "duplicated function name '{cand_str}' identifies an intrinsic or \
                                 Miden ABI linker stub, which midenc recognizes by name, so it \
                                 cannot be renamed"
                            );
                        }
                    }

                    // Interning in the loop is fine because either the string is already interned
                    // (if taken) or it is about to be used as new symbol.
                    let mut unique_str = format!("{cand_str}_func{}", fidx.as_u32());
                    while taken.contains(&Symbol::intern(unique_str.as_str())) {
                        unique_str.push('_');
                    }
                    let unique_sym = Symbol::intern(unique_str);
                    taken.insert(unique_sym);
                    unique_sym
                }
            };
            linkages[fidx.index()] = Some(linkage);
        }

        self.func_linkages.clear();
        self.func_linkages.reserve(linkages.len());
        for (idx, slot) in linkages.into_iter().enumerate() {
            let linkage = slot.expect("linkage assigned for every function");
            let key = self.func_linkages.push(linkage);
            debug_assert_eq!(key, FuncIndex::new(idx));
        }

        Ok(())
    }

    /// Returns the name of the given data segment.
    ///
    /// If the wasm name section does not include an entry for this segment
    /// (e.g. when the binary was built with `strip = true` or the producer
    /// did not emit a name subsection for data segments), falls back to a
    /// synthesized `data{index}` name.
    pub fn data_segment_name(&self, index: DataSegmentIndex) -> Symbol {
        self.name_section
            .data_segment_names
            .get(&index)
            .cloned()
            .unwrap_or_else(|| Symbol::intern(format!("data{}", index.as_u32())))
    }

    /// Returns the name of the given local (including parameters) if available in the name section.
    pub fn local_name(&self, func: FuncIndex, index: u32) -> Option<Symbol> {
        self.name_section
            .locals_names
            .get(&func)
            .and_then(|locals| locals.get(&index).copied())
    }

    /// Sets the fallback name of this module, used if there is no module name in the name section
    pub fn set_name_fallback(&mut self, name_fallback: Cow<'static, str>) {
        self.name_fallback = Some(Ident::from(name_fallback.as_ref()));
    }

    /// Sets the name of this module, discarding whatever is in the name section
    pub fn set_name_override(&mut self, name_override: Cow<'static, str>) {
        self.name_override = Some(Ident::from(name_override.as_ref()));
    }
}

impl fmt::Display for ModuleImport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}({:?})", self.module, self.field, self.index)
    }
}

/// Type information about functions in a wasm module.
#[derive(Debug, Clone, Copy)]
pub struct FunctionTypeInfo {
    /// The type of this function, indexed into the module-wide type tables for
    /// a module compilation.
    pub signature: SignatureIndex,
    /// The index into the funcref table, if present. Note that this is
    /// `reserved_value()` if the function does not escape from a module.
    pub func_ref: FuncRefIndex,
}

impl FunctionTypeInfo {
    /// Returns whether this function's type is one that "escapes" the current
    /// module, meaning that the function is exported, used in `ref.func`, used
    /// in a table, etc.
    pub fn is_escaping(&self) -> bool {
        !self.func_ref.is_reserved_value()
    }
}

/// Index into the funcref table
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct FuncRefIndex(u32);

impl cranelift_entity::EntityRef for FuncRefIndex {
    #[inline]
    fn new(index: usize) -> Self {
        debug_assert!(index < u32::MAX as usize);
        Self(index as u32)
    }

    #[inline]
    fn index(self) -> usize {
        self.0 as usize
    }
}

impl cranelift_entity::packed_option::ReservedValue for FuncRefIndex {
    #[inline]
    fn reserved_value() -> Self {
        Self(u32::MAX)
    }

    #[inline]
    fn is_reserved_value(&self) -> bool {
        self.0 == u32::MAX
    }
}

impl FuncRefIndex {
    /// Create a new instance from a `u32`.
    #[inline]
    pub fn from_u32(x: u32) -> Self {
        debug_assert!(x < u32::MAX);
        Self(x)
    }

    /// Return the underlying index value as a `u32`.
    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Return the raw bit encoding for this instance.
    ///
    /// __Warning__: the raw bit encoding is opaque and has no
    /// guaranteed correspondence to the entity's index. It encodes the
    /// entire state of this index value: either a valid index or an
    /// invalid-index sentinel. The value returned by this method should
    /// only be passed to `from_bits`.
    #[inline]
    pub fn as_bits(self) -> u32 {
        self.0
    }

    /// Create a new instance from the raw bit encoding.
    ///
    /// __Warning__: the raw bit encoding is opaque and has no
    /// guaranteed correspondence to the entity's index. It encodes the
    /// entire state of this index value: either a valid index or an
    /// invalid-index sentinel. The value returned by this method should
    /// only be given bits from `as_bits`.
    #[inline]
    pub fn from_bits(x: u32) -> Self {
        Self(x)
    }
}

/// Parsed names from the Wasm [name section].
///
/// [name section]: https://webassembly.github.io/spec/core/appendix/custom.html#name-section
#[derive(Debug, Default)]
pub struct NameSection {
    pub module_name: Option<Ident>,
    pub func_names: FxHashMap<FuncIndex, Symbol>,
    pub locals_names: FxHashMap<FuncIndex, FxHashMap<u32, Symbol>>,
    pub globals_names: FxHashMap<GlobalIndex, Symbol>,
    pub data_segment_names: FxHashMap<DataSegmentIndex, Symbol>,
}
