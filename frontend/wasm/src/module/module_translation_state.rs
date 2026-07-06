use cranelift_entity::packed_option::ReservedValue;
use midenc_hir::{
    CallConv, FunctionType, FxHashMap, Ident, SourceSpan, SymbolNameComponent, SymbolPath,
    Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{
        FunctionRef, FunctionTableRef, ModuleBuilder, WorldBuilder, attributes::Signature,
    },
    interner::Symbol,
    smallvec,
};
use midenc_session::diagnostics::{DiagnosticsHandler, Severity};

use super::{
    DefinedTableIndex, FuncIndex, Module, TableIndex, TableInitialValue,
    instance::ModuleArgument,
    ir_func_type,
    types::{ModuleTypesBuilder, WasmRefType},
};
use crate::{
    callable::CallableFunction,
    component::lower_imports::generate_import_lowering_function,
    error::WasmResult,
    intrinsics::{Intrinsic, IntrinsicsConversionResult, attach_effects_to_function},
    translation_utils::sig_from_func_type,
    unsupported_diag,
};

/// A practical bound on the number of slots in a lowered function table.
///
/// Each slot occupies one word of linear memory, and each initialized slot materializes IR and
/// startup code, so absurdly-sized tables (which no real program produces) are rejected up front
/// rather than exhausting memory or overflowing the memory layout.
const MAX_FUNCTION_TABLE_SLOTS: u32 = 1 << 20;

pub struct ModuleTranslationState<'a> {
    /// Imported and local functions
    functions: FxHashMap<FuncIndex, CallableFunction>,
    /// Lowered function tables, keyed by Wasm table index
    tables: FxHashMap<TableIndex, FunctionTableRef>,
    pub module_builder: &'a mut ModuleBuilder,
    pub world_builder: &'a mut WorldBuilder,
}

impl<'a> ModuleTranslationState<'a> {
    /// Create a new `ModuleTranslationState` for the core Wasm module translation
    ///
    /// Parameters:
    /// `module` - the core Wasm module
    /// `module_builder` - the Miden IR Module builder
    /// `world_builder` - the Miden IR World builder
    /// `mod_types` - the Miden IR module types builder
    /// `module_args` - the module instantiation arguments, i.e. entities to "fill" module imports
    pub fn new(
        module: &Module,
        module_builder: &'a mut ModuleBuilder,
        world_builder: &'a mut WorldBuilder,
        mod_types: &ModuleTypesBuilder,
        module_args: FxHashMap<SymbolPath, ModuleArgument>,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Self> {
        let mut functions = FxHashMap::default();
        for (index, func_type) in &module.functions {
            let wasm_func_type = mod_types[func_type.signature].clone();
            let ir_func_type = ir_func_type(&wasm_func_type, diagnostics)?;
            let func_name = module.func_name(index);
            let path = SymbolPath {
                path: smallvec![
                    SymbolNameComponent::Root,
                    SymbolNameComponent::Component(module.name().as_symbol()),
                    SymbolNameComponent::Leaf(func_name)
                ],
            };
            let visibility = if module.is_exported(index.into()) {
                Visibility::Public
            } else {
                Visibility::Private
            };
            let sig = sig_from_func_type(&ir_func_type, CallConv::C);
            if module.is_imported_function(index) {
                assert!((index.as_u32() as usize) < module.num_imported_funcs);
                let import = &module.imports[index.as_u32() as usize];
                let func = process_import(
                    module_builder,
                    world_builder,
                    &module_args,
                    path,
                    sig,
                    import,
                    diagnostics,
                )?;
                functions.insert(index, func);
            } else {
                let function_ref = module_builder
                    .define_function(path.name().into(), visibility, sig.clone())
                    .map_err(|e| {
                        diagnostics
                            .diagnostic(Severity::Error)
                            .with_message(format!(
                                "Failed to add new function '{}' to module: {e:?}",
                                path.name()
                            ))
                            .into_report()
                    })?;
                let defined_function = CallableFunction::Function {
                    wasm_id: path,
                    function_ref,
                    signature: sig.clone(),
                };
                functions.insert(index, defined_function);
            };
        }
        Ok(Self {
            functions,
            tables: FxHashMap::default(),
            module_builder,
            world_builder,
        })
    }

    /// Get the `CallableFunction` that should be used to make a direct call to function `index`.
    pub(crate) fn get_direct_func(&mut self, index: FuncIndex) -> WasmResult<CallableFunction> {
        let defined_func = self.functions[&index].clone();
        Ok(defined_func)
    }

    /// Get the lowered function table for the Wasm table `table_index`, building it on first use.
    ///
    /// Tables are lowered lazily, only when a `call_indirect` actually dispatches through them:
    /// wit-bindgen and rustc routinely emit `funcref` tables (e.g. holding only `cabi_realloc`,
    /// or entirely empty) in modules that never perform an indirect call, and lowering those
    /// would burden every compiled program with a useless table and its initialization code.
    ///
    /// Only locally-defined `funcref` tables with statically-known (constant-offset) element
    /// segments are supported; anything else produces a compile-time error.
    pub(crate) fn get_or_build_table(
        &mut self,
        table_index: TableIndex,
        module: &Module,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<FunctionTableRef> {
        if let Some(table_ref) = self.tables.get(&table_index) {
            return Ok(*table_ref);
        }

        let Some(defined_idx) = module.defined_table_index(table_index) else {
            unsupported_diag!(
                diagnostics,
                "unsupported `call_indirect`: imported tables are not supported yet"
            );
        };
        let table = &module.tables[table_index];
        if table.wasm_ty != WasmRefType::FUNCREF {
            unsupported_diag!(
                diagnostics,
                "unsupported table type '{}': only 'funcref' tables are supported",
                table.wasm_ty
            );
        }
        if table.minimum > MAX_FUNCTION_TABLE_SLOTS {
            unsupported_diag!(
                diagnostics,
                "unsupported `call_indirect`: table has {} slots, which exceeds the supported \
                 maximum of {MAX_FUNCTION_TABLE_SLOTS}",
                table.minimum
            );
        }

        // An all-`None` image is fine: every dispatch through such a table fails at runtime on
        // the zero MAST root of a null slot, matching Wasm's uninitialized-element trap
        let image = collect_table_image(table_index, defined_idx, module, diagnostics)?;

        // The table symbol is internal to the compiler, so use a hygienic generated name; a Wasm
        // export name is an arbitrary string that could collide with other module symbols
        let name = format!("__indirect_function_table_{}", table_index.as_u32());
        let table_ref = self
            .module_builder
            .define_function_table(Ident::from(name.as_str()), Visibility::Private, table.minimum)
            .map_err(|e| {
                diagnostics
                    .diagnostic(Severity::Error)
                    .with_message(format!("Failed to add function table '{name}' to module: {e:?}"))
                    .into_report()
            })?;
        let span = SourceSpan::default();
        for (slot, func_index) in image.into_iter().enumerate() {
            let Some(func_index) = func_index else {
                continue;
            };
            self.add_table_entry(table_ref, slot as u32, func_index, module, span, diagnostics)?;
        }
        self.tables.insert(table_index, table_ref);
        Ok(table_ref)
    }

    /// Record that slot `index` of `table` is initialized with the function `func_index`.
    fn add_table_entry(
        &mut self,
        table: FunctionTableRef,
        index: u32,
        func_index: FuncIndex,
        module: &Module,
        span: SourceSpan,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<()> {
        match self.get_direct_func(func_index)? {
            CallableFunction::Function { function_ref, .. }
            | CallableFunction::Intrinsic { function_ref, .. } => self
                .module_builder
                .append_function_table_entry(table, index, function_ref, span),
            CallableFunction::Instruction { .. } => {
                unsupported_diag!(
                    diagnostics,
                    "unsupported function table element: '{}' is an inlined intrinsic without a \
                     procedure body",
                    module.func_name(func_index)
                );
            }
        }
    }

    /// Register a linker stub function as an intrinsic so that calls to it will be inlined.
    ///
    /// This updates the function's entry in the functions map from `CallableFunction::Function`
    /// to either `CallableFunction::Instruction` (for inline operations) or
    /// `CallableFunction::Intrinsic` (for MASM function calls).
    ///
    /// Returns the FunctionRef if the stub was registered (so it can be removed from the module),
    /// or None if the function wasn't found or isn't a valid intrinsic stub.
    pub(crate) fn register_linker_stub(
        &mut self,
        func_index: FuncIndex,
        intrinsic: Intrinsic,
    ) -> WasmResult<Option<FunctionRef>> {
        let Some(callable) = self.functions.get(&func_index) else {
            return Ok(None);
        };

        let CallableFunction::Function {
            function_ref,
            signature,
            ..
        } = callable
        else {
            return Ok(None);
        };

        let function_ref = *function_ref;
        let signature = signature.clone();

        // Determine if this intrinsic is inlined as an op or needs a function call
        let Some(conv) = intrinsic.conversion_result() else {
            return Ok(None);
        };

        match conv {
            IntrinsicsConversionResult::FunctionType { ty, effects } => {
                // Create import function reference for the intrinsic
                let import_path = intrinsic.into_symbol_path();
                let import_ft: FunctionType = ty;
                let context = self.world_builder.context_rc();
                let import_sig = Signature::new(&context, import_ft.params, import_ft.results);

                let import_module_ref = self
                    .world_builder
                    .declare_module_tree(&import_path.without_leaf())
                    .wrap_err("failed to create module for intrinsic imports")?;
                let mut import_module_builder = ModuleBuilder::new(import_module_ref);
                let mut intrinsic_func_ref = import_module_builder
                    .define_function(import_path.name().into(), Visibility::Public, import_sig)
                    .wrap_err("failed to create intrinsic function ref")?;

                {
                    let mut intrinsic_func = intrinsic_func_ref.borrow_mut();
                    attach_effects_to_function(&mut intrinsic_func, effects.iter());
                }

                self.functions.insert(
                    func_index,
                    CallableFunction::Intrinsic {
                        intrinsic,
                        function_ref: intrinsic_func_ref,
                        signature,
                    },
                );
            }
            IntrinsicsConversionResult::MidenVmOp => {
                // Inline as an operation
                self.functions.insert(
                    func_index,
                    CallableFunction::Instruction {
                        intrinsic,
                        signature,
                    },
                );
            }
            // Module-context stubs keep their defined function: the body is synthesized during
            // translation, so calls to them remain ordinary calls
            IntrinsicsConversionResult::ModuleContextStub => return Ok(None),
        }

        Ok(Some(function_ref))
    }
}

/// Compute the final compile-time image of a table: for each slot, the function whose MAST root
/// it holds at startup, or `None` for a null slot.
///
/// The table's initial value and its active element segments are applied in order, with later
/// writes — including explicit `ref.null` entries — replacing earlier ones, matching Wasm
/// table-initialization semantics.
fn collect_table_image(
    table_index: TableIndex,
    defined_idx: DefinedTableIndex,
    module: &Module,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<Vec<Option<FuncIndex>>> {
    let table = &module.tables[table_index];
    let mut image: Vec<Option<FuncIndex>> = vec![None; table.minimum as usize];
    match &module.table_initialization.initial_values[defined_idx] {
        TableInitialValue::Null { precomputed } => {
            // NOTE: this parser never populates `precomputed` (element segments are kept as-is),
            // but handle it as a full image for robustness; null slots are encoded there as the
            // reserved function index
            for (slot, func_index) in precomputed.iter().enumerate().take(image.len()) {
                image[slot] = (!func_index.is_reserved_value()).then_some(*func_index);
            }
        }
        TableInitialValue::FuncRef(func_index) => {
            image.fill(Some(*func_index));
        }
    }
    for segment in module
        .table_initialization
        .segments
        .iter()
        .filter(|segment| segment.table_index == table_index)
    {
        if segment.base.is_some() {
            unsupported_diag!(
                diagnostics,
                "unsupported element segment: global-relative offsets are not supported"
            );
        }
        let end = segment.offset as u64 + segment.elements.len() as u64;
        if end > image.len() as u64 {
            unsupported_diag!(
                diagnostics,
                "invalid element segment: initializes slots {}..{end} of a table with {} slots",
                segment.offset,
                image.len()
            );
        }
        for (i, func_index) in segment.elements.iter().enumerate() {
            // An explicit `ref.null` entry clears the slot
            image[segment.offset as usize + i] =
                (!func_index.is_reserved_value()).then_some(*func_index);
        }
    }
    Ok(image)
}

/// Returns [`CallableFunction`] translated from the core Wasm module import
fn process_import(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    module_args: &FxHashMap<SymbolPath, ModuleArgument>,
    core_func_id: SymbolPath,
    core_func_sig: Signature,
    import: &super::ModuleImport,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<CallableFunction> {
    let import_path = SymbolPath {
        path: smallvec![
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(Symbol::intern(&import.module)),
            SymbolNameComponent::Leaf(Symbol::intern(&import.field))
        ],
    };
    let Some(module_arg) = module_args.get(&import_path) else {
        crate::unsupported_diag!(diagnostics, "unexpected import '{import_path:?}'");
    };
    process_module_arg(
        module_builder,
        world_builder,
        core_func_id,
        core_func_sig,
        import_path,
        module_arg,
        diagnostics,
    )
}

fn process_module_arg(
    module_builder: &mut ModuleBuilder,
    world_builder: &mut WorldBuilder,
    path: SymbolPath,
    sig: Signature,
    wasm_import_path: SymbolPath,
    module_arg: &ModuleArgument,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<CallableFunction> {
    Ok(match module_arg {
        ModuleArgument::Function(_) => {
            // Support would generate an internal function which calls the import argument
            crate::unsupported_diag!(
                diagnostics,
                "core Wasm function imports are not supported yet"
            );
        }
        ModuleArgument::ComponentImport(signature) => generate_import_lowering_function(
            world_builder,
            module_builder,
            wasm_import_path,
            signature,
            path,
            sig,
        )?,
        ModuleArgument::Table => {
            crate::unsupported_diag!(diagnostics, "imported tables are not supported yet");
        }
    })
}
