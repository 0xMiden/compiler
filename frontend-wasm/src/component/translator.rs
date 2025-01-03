use midenc_hir::{
    cranelift_entity::PrimaryMap, diagnostics::Severity, CanonAbiImport, ComponentBuilder,
    ComponentExport, FunctionIdent, FunctionType, FxHashMap, Ident, InterfaceFunctionIdent,
    InterfaceIdent, MidenAbiImport, Symbol,
};
use midenc_hir_type::Abi;
use midenc_session::{DiagnosticsHandler, Session};

use super::{
    interface_type_to_ir, CanonicalOptions, ComponentTypes, CoreDef, CoreExport, Export,
    ExportItem, GlobalInitializer, InstantiateModule, LinearComponent, LinearComponentTranslation,
    LoweredIndex, RuntimeImportIndex, RuntimeInstanceIndex, RuntimePostReturnIndex,
    RuntimeReallocIndex, StaticModuleIndex, Trampoline, TypeFuncIndex,
};
use crate::{
    component::StringEncoding,
    error::WasmResult,
    intrinsics::{
        intrinsics_conversion_result, is_miden_intrinsics_module, IntrinsicsConversionResult,
    },
    miden_abi::{is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id},
    module::{
        build_ir::build_ir_module,
        instance::ModuleArgument,
        module_env::ParsedModule,
        module_translation_state::ModuleTranslationState,
        types::{EntityIndex, FuncIndex},
        Module,
    },
    unsupported_diag, WasmTranslationConfig,
};

/// A translator from the linearized Wasm component model to the Miden IR component
pub struct ComponentTranslator<'a, 'data> {
    /// The Wasm component types
    component_types: ComponentTypes,
    /// The parsed static modules of the Wasm component
    parsed_modules: PrimaryMap<StaticModuleIndex, ParsedModule<'data>>,
    /// The translation configuration
    config: &'a WasmTranslationConfig,
    /// The runtime module instances index mapped to the static module index
    module_instances_source: PrimaryMap<RuntimeInstanceIndex, StaticModuleIndex>,
    /// The lower imports index mapped to the runtime import index
    lower_imports: FxHashMap<LoweredIndex, RuntimeImportIndex>,
    /// The realloc functions used in CanonicalOptions in this component
    reallocs: FxHashMap<RuntimeReallocIndex, FunctionIdent>,
    /// The post return functions used in CanonicalOptions in this component
    post_returns: FxHashMap<RuntimePostReturnIndex, FunctionIdent>,
    session: &'a Session,
}

impl<'a, 'data> ComponentTranslator<'a, 'data> {
    pub fn new(
        component_types: ComponentTypes,
        parsed_modules: PrimaryMap<StaticModuleIndex, ParsedModule<'data>>,
        config: &'a WasmTranslationConfig,
        session: &'a Session,
    ) -> Self {
        Self {
            component_types,
            parsed_modules,
            config,
            session,
            module_instances_source: PrimaryMap::new(),
            lower_imports: FxHashMap::default(),
            reallocs: FxHashMap::default(),
            post_returns: FxHashMap::default(),
        }
    }

    /// Translate the given linearized Wasm component to the Miden IR component
    pub fn translate(
        mut self,
        wasm_translation: LinearComponentTranslation,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<midenc_hir::Component> {
        let mut component_builder: midenc_hir::ComponentBuilder<'a> =
            midenc_hir::ComponentBuilder::new(&self.session.diagnostics);
        // dbg!(&wasm_translation.component.initializers);
        for initializer in &wasm_translation.component.initializers {
            match initializer {
                GlobalInitializer::InstantiateModule(instantiate_module) => {
                    self.translate_module_instance(
                        instantiate_module,
                        &mut component_builder,
                        &wasm_translation,
                        diagnostics,
                    )?;
                }
                GlobalInitializer::LowerImport {
                    index: init_lowered_idx,
                    import,
                } => {
                    self.lower_imports.insert(*init_lowered_idx, *import);
                }
                GlobalInitializer::ExtractMemory(mem) => {
                    if mem.index.as_u32() > 0 {
                        unsupported_diag!(
                            &self.session.diagnostics,
                            "only one memory is supported in the component"
                        );
                    }
                }
                GlobalInitializer::ExtractRealloc(realloc) => {
                    let func_id = self.func_id_from_core_def(&realloc.def)?;
                    self.reallocs.insert(realloc.index, func_id);
                }
                GlobalInitializer::ExtractPostReturn(post_return) => {
                    let func_id = self.func_id_from_core_def(&post_return.def)?;
                    self.post_returns.insert(post_return.index, func_id);
                }
                GlobalInitializer::Resource(_) => {
                    unsupported_diag!(
                        &self.session.diagnostics,
                        "resource global initializers are not yet supported"
                    );
                }
            }
        }
        for (name, export) in &wasm_translation.component.exports {
            self.build_export(export, name.clone(), None, &mut component_builder)?;
        }
        Ok(component_builder.build())
    }

    /// Translate the given Wasm core module instantiotion to the Miden IR component
    fn translate_module_instance(
        &mut self,
        instantiate_module: &InstantiateModule,
        component_builder: &mut ComponentBuilder<'_>,
        wasm_translation: &LinearComponentTranslation,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<()> {
        match instantiate_module {
            InstantiateModule::Static(static_module_idx, args) => {
                if self.module_instances_source.values().any(|idx| *idx == *static_module_idx) {
                    unsupported_diag!(
                        &self.session.diagnostics,
                        "A module with a static index {} is already instantiated. We don't \
                         support multiple instantiations of the same module.",
                        static_module_idx.as_u32()
                    );
                }
                self.module_instances_source.push(*static_module_idx);
                // TODO: create and init module instance tables
                // see https://github.com/0xPolygonMiden/compiler/issues/133
                let module = &self.parsed_modules[*static_module_idx].module;
                let mut module_args: Vec<ModuleArgument> = Vec::new();
                for (idx, arg) in args.iter().enumerate() {
                    match arg {
                        CoreDef::Export(export) => {
                            module_args.push(self.module_arg_from_export(export)?);
                        }
                        CoreDef::InstanceFlags(_) => {
                            unsupported_diag!(
                                &self.session.diagnostics,
                                "Wasm component instance flags are not supported"
                            );
                        }
                        CoreDef::Trampoline(trampoline_idx) => {
                            let trampoline = &wasm_translation.trampolines[*trampoline_idx];
                            if let Some(arg) = self.module_arg_from_trampoline(
                                trampoline,
                                module,
                                idx,
                                &wasm_translation.component,
                                component_builder,
                                diagnostics,
                            )? {
                                module_args.push(arg)
                            }
                        }
                    }
                }
                let module_types = self.component_types.module_types();
                let mut module_state = ModuleTranslationState::new(
                    module,
                    module_types,
                    module_args,
                    &self.session.diagnostics,
                );
                let ir_module = build_ir_module(
                    self.parsed_modules.get_mut(*static_module_idx).unwrap(),
                    module_types,
                    &mut module_state,
                    self.config,
                    self.session,
                )?;
                // Skip the shim and fixups core modules, they are a workaround for
                // specify the core instance memory and reallod function for the lowering
                if ir_module.name.as_str() != "wit-component:shim"
                    && ir_module.name.as_str() != "wit-component:fixups"
                {
                    component_builder
                        .add_module(ir_module.into())
                        .expect("module is already added");
                } else {
                    // dbg!(&ir_module.functions().collect::<Vec<_>>());
                }
            }
            InstantiateModule::Import(..) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Imported Wasm core module instantiation is not supported"
                );
            }
        };
        Ok(())
    }

    /// Build a Wasm core module argument from the given trampoline (component import)
    /// Returns `None` if the trampoline was an intrinsics that were converted
    fn module_arg_from_trampoline(
        &self,
        trampoline: &Trampoline,
        module: &Module,
        idx: usize,
        wasm_component: &LinearComponent,
        component_builder: &mut ComponentBuilder<'_>,
        _diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Option<ModuleArgument>> {
        match trampoline {
            Trampoline::LowerImport {
                index,
                lower_ty,
                options,
            } => {
                let module_import = module.imports.get(idx).expect("module import not found");
                let func_index = module_import.index.unwrap_func();
                let function_id = module.func_name(func_index).into();
                let function_id = FunctionIdent {
                    module: module.name(),
                    function: function_id,
                };

                let runtime_import_idx = self.lower_imports[index];
                // dbg!(&module_import);
                match self.translate_import(
                    runtime_import_idx,
                    *lower_ty,
                    options,
                    wasm_component,
                )? {
                    Some(component_import) => {
                        component_builder.add_import(function_id, component_import.clone());
                        Ok(Some(ModuleArgument::ComponentImport(component_import)))
                    }

                    None => Ok(None),
                }
            }
            _ => unsupported_diag!(
                &self.session.diagnostics,
                "Not yet implemented trampoline type {:?}",
                trampoline
            ),
        }
    }

    /// Build a module argument from the given module export
    fn module_arg_from_export(
        &self,
        export: &CoreExport<EntityIndex>,
    ) -> WasmResult<ModuleArgument> {
        match export.item {
            ExportItem::Index(entity_idx) => match entity_idx {
                EntityIndex::Function(func_idx) => {
                    let exporting_module_id = self.module_instances_source[export.instance];
                    let function_id = function_id_from_export(
                        &self.parsed_modules[exporting_module_id].module,
                        func_idx,
                    );
                    Ok(ModuleArgument::Function(function_id))
                }
                EntityIndex::Table(_idx) => {
                    // TODO: init the exported table with this module's table initialization values
                    // see https://github.com/0xPolygonMiden/compiler/issues/133
                    Ok(ModuleArgument::Table)
                }
                EntityIndex::Memory(_) => {
                    unreachable!("Attempt to export memory from a module instance. ")
                }
                EntityIndex::Global(_) => unsupported_diag!(
                    &self.session.diagnostics,
                    "Exporting of core module globals are not yet supported"
                ),
            },
            ExportItem::Name(_) => unsupported_diag!(
                &self.session.diagnostics,
                "Named core module exports are not yet supported"
            ),
        }
    }

    /// Translate the given runtime import to the Miden IR component import
    /// Returns `None` if the import was an intrinsics that were converted
    fn translate_import(
        &self,
        runtime_import_index: RuntimeImportIndex,
        high_func_ty: TypeFuncIndex,
        options: &CanonicalOptions,
        wasm_component: &LinearComponent,
    ) -> WasmResult<Option<midenc_hir::ComponentImport>> {
        let (import_idx, import_names) = &wasm_component.imports[runtime_import_index];
        if import_names.len() != 1 {
            unsupported_diag!(&self.session.diagnostics, "multi-name imports not supported");
        }
        let import_func_name = import_names.first().unwrap();
        let (full_interface_name, _) = wasm_component.import_types[*import_idx].clone();
        let function_id = recover_imported_masm_function_id(&full_interface_name, import_func_name);
        if is_miden_abi_module(function_id.module.as_symbol())
            || is_miden_intrinsics_module(function_id.module.as_symbol())
        {
            let function_ty = if is_miden_abi_module(function_id.module.as_symbol()) {
                miden_abi_function_type(
                    function_id.module.as_symbol(),
                    function_id.function.as_symbol(),
                )
            } else if is_miden_intrinsics_module(function_id.module.as_symbol()) {
                match intrinsics_conversion_result(&function_id) {
                    IntrinsicsConversionResult::FunctionType(function_ty) => function_ty,
                    IntrinsicsConversionResult::MidenVmOp => {
                        // Skip this import since it was converted to a Miden VM op(s)
                        return Ok(None);
                    }
                }
            } else {
                panic!("no support for importing function from module {}", function_id.module);
            };
            let component_import =
                midenc_hir::ComponentImport::MidenAbiImport(MidenAbiImport::new(function_ty));
            Ok(Some(component_import))
        } else {
            let interface_function = InterfaceFunctionIdent {
                interface: InterfaceIdent::from_full_ident(&full_interface_name),
                function: Symbol::intern(import_func_name),
            };
            let lifted_func_ty = convert_lifted_func_ty(&high_func_ty, &self.component_types);
            let component_import =
                midenc_hir::ComponentImport::CanonAbiImport(CanonAbiImport::new(
                    interface_function,
                    lifted_func_ty,
                    self.translate_canonical_options(options)?,
                ));
            Ok(Some(component_import))
        }
    }

    /// Build an IR Component export from the given Wasm component export
    fn build_export(
        &self,
        export: &Export,
        name: String,
        interface: Option<String>,
        component_builder: &mut ComponentBuilder,
    ) -> WasmResult<()> {
        match export {
            Export::LiftedFunction { ty, func, options } => {
                // dbg!(name);
                // The inline export does no have an interface name
                let interface = interface.unwrap_or_default();
                // dbg!(&interface);
                let export_name = InterfaceFunctionIdent::from_full(interface, name.clone());
                let export = self.build_export_lifted_function(func, ty, options)?;
                component_builder.add_export(export_name, export);
                Ok(())
            }
            Export::Instance(exports) => {
                let interface = Some(name.clone());
                // Flatten any(nested) interface instance exports into the IR `Component` exports
                for (export_name, export) in exports {
                    self.build_export(
                        export,
                        export_name.clone(),
                        interface.clone(),
                        component_builder,
                    )?;
                }
                Ok(())
            }
            Export::ModuleStatic(_) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Static module exports are not supported"
                );
            }
            Export::ModuleImport(_) => unsupported_diag!(
                &self.session.diagnostics,
                "Exporting of an imported module is not supported"
            ),
            Export::Type(_) => {
                // Besides the function exports the individual type are also exported from the
                // component We can ignore them for now
                Ok(())
            }
        }
    }

    /// Build an IR Component export from the given lifted Wasm core module function export
    fn build_export_lifted_function(
        &self,
        func: &CoreDef,
        ty: &TypeFuncIndex,
        options: &CanonicalOptions,
    ) -> WasmResult<ComponentExport> {
        let core_func_ident = self.func_id_from_core_def(func)?;
        let lifted_func_ty = convert_lifted_func_ty(ty, &self.component_types);
        let export = midenc_hir::ComponentExport {
            function: core_func_ident,
            function_ty: lifted_func_ty,
            options: self.translate_canonical_options(options)?,
        };
        Ok(export)
    }

    fn func_id_from_core_def(&self, func: &CoreDef) -> WasmResult<FunctionIdent> {
        Ok(match func {
            CoreDef::Export(core_export) => {
                let module =
                    &self.parsed_modules[self.module_instances_source[core_export.instance]].module;
                let from = Ident::from(module.name().as_str());
                let module_name = from;
                let func_name = match core_export.item {
                    ExportItem::Index(idx) => match idx {
                        EntityIndex::Function(func_idx) => module.func_name(func_idx),
                        EntityIndex::Table(_) | EntityIndex::Memory(_) | EntityIndex::Global(_) => {
                            unsupported_diag!(
                                &self.session.diagnostics,
                                "Exporting of non-function entity {:?} is not supported",
                                core_export
                            );
                        }
                    },
                    ExportItem::Name(_) => {
                        unsupported_diag!(
                            &self.session.diagnostics,
                            "Named exports are not yet supported"
                        );
                    }
                };

                midenc_hir::FunctionIdent {
                    module: module_name,
                    function: midenc_hir::Ident::with_empty_span(func_name),
                }
            }
            CoreDef::InstanceFlags(_) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Component instance flags exports are not supported"
                );
            }
            CoreDef::Trampoline(_) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Trampoline core module exports are not supported"
                );
            }
        })
    }

    fn translate_canonical_options(
        &self,
        options: &CanonicalOptions,
    ) -> WasmResult<midenc_hir::CanonicalOptions> {
        if options.string_encoding != StringEncoding::Utf8 {
            unsupported_diag!(
                &self.session.diagnostics,
                "UTF-8 is expected in CanonicalOptions, string transcoding is not yet supported"
            );
        }
        Ok(midenc_hir::CanonicalOptions {
            realloc: options.realloc.map(|idx| self.reallocs[&idx]),
            post_return: options.post_return.map(|idx| self.post_returns[&idx]),
        })
    }
}

/// Get the function id from the given Wasm func_idx in the given Wasm core exporting_module
fn function_id_from_export(exporting_module: &Module, func_idx: FuncIndex) -> FunctionIdent {
    let func_name = exporting_module.func_name(func_idx);

    FunctionIdent {
        module: exporting_module.name(),
        function: Ident::with_empty_span(func_name),
    }
}

/// Convert the given Wasm component function type to the Miden IR lifted function type
pub fn convert_lifted_func_ty(
    ty: &TypeFuncIndex,
    component_types: &ComponentTypes,
) -> FunctionType {
    let type_func = component_types[*ty].clone();
    let params_types = component_types[type_func.params].clone().types;
    let results_types = component_types[type_func.results].clone().types;
    let params = params_types
        .iter()
        .map(|ty| interface_type_to_ir(ty, component_types))
        .collect();
    let results = results_types
        .iter()
        .map(|ty| interface_type_to_ir(ty, component_types))
        .collect();
    FunctionType {
        params,
        results,
        abi: Abi::Wasm,
    }
}
