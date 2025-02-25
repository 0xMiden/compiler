//! Data structures for representing parsed Wasm modules.

use std::{borrow::Cow, collections::BTreeMap, ops::Range};

use cranelift_entity::{packed_option::ReservedValue, EntityRef, PrimaryMap};
use indexmap::IndexMap;
use midenc_hir2::{interner::Symbol, FxHashMap, Ident};
use midenc_session::DiagnosticsHandler;

use self::types::*;
use crate::{component::SignatureIndex, error::WasmResult, unsupported_diag};

pub mod build_ir;
pub mod func_translation_state;
pub mod func_translator;
pub mod function_builder_ext;
pub mod instance;
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
    name_section: NameSection,

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

    /// Returns the name of the given function
    pub fn func_name(&self, index: FuncIndex) -> Symbol {
        self.name_section
            .func_names
            .get(&index)
            .cloned()
            .unwrap_or(Symbol::intern(format!("func{}", index.as_u32())))
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
cranelift_entity::entity_impl!(FuncRefIndex);

#[derive(Debug, Default)]
pub struct NameSection {
    pub module_name: Option<Ident>,
    pub func_names: FxHashMap<FuncIndex, Symbol>,
    pub locals_names: FxHashMap<FuncIndex, FxHashMap<u32, Symbol>>,
    pub globals_names: FxHashMap<GlobalIndex, Symbol>,
    pub data_segment_names: FxHashMap<DataSegmentIndex, Symbol>,
}
