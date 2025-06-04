use std::rc::Rc;

use cranelift_entity::PrimaryMap;
use midenc_hir::{
    self as hir2,
    diagnostics::Report,
    dialects::builtin::{self, ComponentBuilder, ModuleBuilder, World, WorldBuilder},
    interner::Symbol,
    smallvec, BuilderExt, CallConv, Context, FunctionType, FxHashMap, Ident, SymbolNameComponent,
    SymbolPath,
};
use wasmparser::{component_types::ComponentEntityType, types::TypesRef};

use super::{
    interface_type_to_ir,
    shim_bypass::{self, ShimBypassInfo},
    CanonLift, CanonLower, ClosedOverComponent, ClosedOverModule, ComponentFuncIndex,
    ComponentIndex, ComponentInstanceIndex, ComponentInstantiation, ComponentTypesBuilder,
    ComponentUpvarIndex, ModuleIndex, ModuleInstanceIndex, ModuleUpvarIndex, ParsedComponent,
    StaticModuleIndex, TypeComponentInstanceIndex, TypeDef, TypeFuncIndex, TypeModuleIndex,
};
use crate::{
    component::{
        lift_exports::generate_export_lifting_function, ComponentItem, LocalInitializer,
        StaticComponentIndex,
    },
    error::WasmResult,
    miden_abi::recover_imported_masm_module,
    module::{
        build_ir::build_ir_module,
        instance::ModuleArgument,
        module_env::ParsedModule,
        module_translation_state::ModuleTranslationState,
        types::{EntityIndex, FuncIndex},
    },
    unsupported_diag, FrontendOutput, WasmTranslationConfig,
};

/// A translator from the linearized Wasm component model to the Miden IR component
pub struct ComponentTranslator<'a> {
    /// The translation configuration
    config: &'a WasmTranslationConfig,

    /// The list of static modules that were found during initial translation of
    /// the component.
    ///
    /// This is used during the instantiation of these modules to ahead-of-time
    /// order the arguments precisely according to what the module is defined as
    /// needing which avoids the need to do string lookups or permute arguments
    /// at runtime.
    nested_modules: &'a mut PrimaryMap<StaticModuleIndex, ParsedModule<'a>>,

    /// The list of static components that were found during initial translation of
    /// the component.
    ///
    /// This is used when instantiating nested components to push a new
    /// `ComponentFrame` with the `ParsedComponent`s here.
    nested_components: &'a PrimaryMap<StaticComponentIndex, ParsedComponent<'a>>,

    world_builder: WorldBuilder,
    result: ComponentBuilder,

    context: Rc<Context>,

    /// Information about shim modules to bypass
    shim_bypass_info: ShimBypassInfo,
}

impl<'a> ComponentTranslator<'a> {
    /// Detect shim and fixup modules in the component
    fn detect_shim_modules(&mut self, root_component: &ParsedComponent) {
        log::debug!(
            "Component has {} initializers and {} static modules",
            root_component.initializers.len(),
            self.nested_modules.len()
        );

        // First, check all static modules
        for (static_idx, module) in self.nested_modules.iter() {
            log::debug!(
                "Static module {}: exports={:?}, imports={:?}",
                static_idx.as_u32(),
                module.module.exports.keys().collect::<Vec<_>>(),
                module.module.imports.iter().map(|i| (&i.module, &i.field)).collect::<Vec<_>>()
            );

            if shim_bypass::is_shim_module(module) {
                log::info!("Detected shim module at static index {}", static_idx.as_u32());
                self.shim_bypass_info.shim_static_modules.push(static_idx.as_u32());
            } else if shim_bypass::is_fixup_module(module) {
                log::info!("Detected fixup module at static index {}", static_idx.as_u32());
                self.shim_bypass_info.fixup_static_modules.push(static_idx.as_u32());
            }
        }

        for (i, init) in root_component.initializers.iter().enumerate() {
            log::trace!("Initializer {}: {:?}", i, std::mem::discriminant(init));
        }

        log::debug!(
            "Shim bypass info: shim_static_modules={:?}, fixup_static_modules={:?}",
            self.shim_bypass_info.shim_static_modules,
            self.shim_bypass_info.fixup_static_modules
        );
    }

    pub fn new(
        id: builtin::ComponentId,
        nested_modules: &'a mut PrimaryMap<StaticModuleIndex, ParsedModule<'a>>,
        nested_components: &'a PrimaryMap<StaticComponentIndex, ParsedComponent<'a>>,
        config: &'a WasmTranslationConfig,
        context: Rc<Context>,
    ) -> Self {
        let ns = hir2::Ident::with_empty_span(id.namespace);
        let name = hir2::Ident::with_empty_span(id.name);

        // If a world wasn't provided to us, create one
        let world_ref = match config.world {
            Some(world) => world,
            None => context.clone().builder().create::<World, ()>(Default::default())()
                .expect("failed to create world"),
        };
        let mut world_builder = WorldBuilder::new(world_ref);

        let raw_entity_ref = world_builder
            .define_component(ns, name, id.version)
            .expect("failed to define component");
        let result = ComponentBuilder::new(raw_entity_ref);

        Self {
            config,
            context,
            nested_modules,
            nested_components,
            world_builder,
            result,
            shim_bypass_info: ShimBypassInfo::default(),
        }
    }

    pub fn translate2(
        mut self,
        root_component: &'a ParsedComponent,
        types: &mut ComponentTypesBuilder,
    ) -> WasmResult<FrontendOutput> {
        self.detect_shim_modules(root_component);

        let mut frame = ComponentFrame::new(root_component.types_ref(), FxHashMap::default());

        for init in &root_component.initializers {
            self.initializer(&mut frame, types, init)?;
        }

        let account_component_metadata_bytes_vec: Vec<Vec<u8>> = self
            .nested_modules
            .into_iter()
            .flat_map(|t| t.1.account_component_metadata_bytes.map(|slice| slice.to_vec()))
            .collect();
        assert!(
            account_component_metadata_bytes_vec.len() <= 1,
            "unexpected multiple core Wasm module to have account component metadata section",
        );
        let account_component_metadata_bytes =
            account_component_metadata_bytes_vec.first().map(ToOwned::to_owned);

        let output = FrontendOutput {
            component: self.result.component,
            account_component_metadata_bytes,
        };
        Ok(output)
    }

    fn initializer(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        init: &'a LocalInitializer<'a>,
    ) -> WasmResult<()> {
        log::trace!("init: {init:?}");
        match init {
            LocalInitializer::Import(name, ty) => {
                match frame.args.get(name.0) {
                    Some(arg) => {
                        frame.push_item(arg.clone());
                    }

                    // Not all arguments need to be provided for instantiation, namely the root
                    // component doesn't require structural type imports to be satisfied.
                    None => {
                        match ty {
                            ComponentEntityType::Instance(_) => {
                                self.component_import(frame, types, name, ty)?;
                            }
                            _ => {
                                unsupported_diag!(
                                    self.context.diagnostics(),
                                    "Importing of {:?} is not yet supported",
                                    ty
                                )
                            }
                        };
                    }
                };
            }
            LocalInitializer::Lower(lower) => {
                log::debug!("Adding canon lower function: {:?}", lower);
                frame.funcs.push(CoreDef::Lower(lower.clone()));
            }
            LocalInitializer::Lift(lift) => {
                frame.component_funcs.push(ComponentFuncDef::Lifted(lift.clone()));
            }
            LocalInitializer::Resource(..) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Resource initializers are not supported"
                )
            }
            LocalInitializer::ResourceNew(..) => {
                unsupported_diag!(self.context.diagnostics(), "Resource creation is not supported")
            }
            LocalInitializer::ResourceRep(..) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Resource representation is not supported"
                )
            }
            LocalInitializer::ResourceDrop(..) | LocalInitializer::ResourceDropAsync(..) => {
                unsupported_diag!(self.context.diagnostics(), "Resource dropping is not supported")
            }
            LocalInitializer::ModuleStatic(static_module_idx) => {
                let module_idx = frame.modules.len() as u32;
                frame.modules.push(ModuleDef::Static(*static_module_idx));

                // Track the mapping from frame module index to static module index for shim/fixup modules
                if self.shim_bypass_info.shim_static_modules.contains(&static_module_idx.as_u32()) {
                    log::warn!(
                        "Marking frame module {} as shim module (static {})",
                        module_idx,
                        static_module_idx.as_u32()
                    );
                    self.shim_bypass_info.shim_module_indices.push(module_idx);
                } else if self
                    .shim_bypass_info
                    .fixup_static_modules
                    .contains(&static_module_idx.as_u32())
                {
                    log::warn!(
                        "Marking frame module {} as fixup module (static {})",
                        module_idx,
                        static_module_idx.as_u32()
                    );
                    self.shim_bypass_info.fixup_module_indices.push(module_idx);
                }
            }
            LocalInitializer::ModuleInstantiate(module_idx, ref args) => {
                self.module_instantiation(frame, types, module_idx, args)?;
            }
            LocalInitializer::ModuleSynthetic(entities) => {
                // Check if this synthetic module contains shim exports
                // If so, we need to track this as a shim-related instance
                let mut is_shim_related = false;
                for (name, _) in entities.iter() {
                    if frame.funcs.iter().any(|(_, f)| {
                        if let CoreDef::Export(inst_idx, export_name) = f {
                            // Check if this export is from a shim instance
                            self.shim_bypass_info.shim_instance_indices.contains(&inst_idx.as_u32())
                                && export_name == name
                        } else {
                            false
                        }
                    }) {
                        is_shim_related = true;
                        break;
                    }
                }

                let instance_idx = frame.module_instances.len() as u32;
                frame.module_instances.push(ModuleInstanceDef::Synthetic(entities));

                if is_shim_related {
                    log::trace!(
                        "Detected shim-related synthetic instance at index {}",
                        instance_idx
                    );
                    // This synthetic instance contains shim exports, mark it for bypass
                    self.shim_bypass_info.shim_instance_indices.push(instance_idx);
                }
            }
            LocalInitializer::ComponentStatic(idx, ref vars) => {
                frame.components.push(ComponentDef {
                    index: *idx,
                    closure: ComponentClosure {
                        modules: vars
                            .modules
                            .iter()
                            .map(|(_, m)| frame.closed_over_module(m))
                            .collect(),
                        components: vars
                            .components
                            .iter()
                            .map(|(_, m)| frame.closed_over_component(m))
                            .collect(),
                    },
                });
            }
            LocalInitializer::ComponentInstantiate(
                instance @ ComponentInstantiation {
                    component,
                    ref args,
                    ty: _,
                },
            ) => {
                let component: &ComponentDef = &frame.components[*component];

                let translation = &self.nested_components[component.index];
                let mut new_frame = ComponentFrame::new(
                    translation.types_ref(),
                    args.iter()
                        .map(|(name, item)| Ok((*name, frame.item(*item, types)?)))
                        .collect::<WasmResult<_>>()?,
                );
                log::debug!(
                    "Processing {} nested component initializers for instance",
                    translation.initializers.len()
                );
                for (i, init) in translation.initializers.iter().enumerate() {
                    log::trace!("Processing nested initializer {}: {:?}", i, init);
                    self.initializer(&mut new_frame, types, init)?;
                }
                let instance_idx = frame
                    .component_instances
                    .push(ComponentInstanceDef::Instantiated(instance.clone()));
                frame.frames.insert(instance_idx, new_frame);
            }
            LocalInitializer::ComponentSynthetic(_) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Synthetic components are not yet supported"
                )
            }
            LocalInitializer::AliasExportFunc(module_instance_idx, name) => {
                log::debug!(
                    "Pushing alias export to frame.funcs at index {} (module_instance: {}, name: \
                     '{}')",
                    frame.funcs.len(),
                    module_instance_idx.as_u32(),
                    name
                );
                frame.funcs.push(CoreDef::Export(*module_instance_idx, name));
            }
            LocalInitializer::AliasExportTable(module_instance_idx, name) => {
                // Check if this table alias is from a shim module that should be bypassed
                if self
                    .shim_bypass_info
                    .shim_instance_indices
                    .contains(&module_instance_idx.as_u32())
                {
                    log::trace!(
                        "Skipping table alias from shim instance {} (table: {})",
                        module_instance_idx.as_u32(),
                        name
                    );
                    // Skip table aliases from shim modules
                } else {
                    unsupported_diag!(
                        self.context.diagnostics(),
                        "Table exports are not yet supported"
                    )
                }
            }
            LocalInitializer::AliasExportGlobal(..) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Global exports are not yet supported"
                )
            }
            LocalInitializer::AliasExportMemory(..) => {
                // Do nothing, assuming Rust compiled code having one memory instance.
            }
            LocalInitializer::AliasComponentExport(component_instance_idx, name) => {
                let import = &frame.component_instances[*component_instance_idx].unwrap_import();
                let def = ComponentItemDef::from_import(
                    name,
                    types[import.ty].exports[*name],
                    *component_instance_idx,
                );
                frame.push_item(def);
            }
            LocalInitializer::AliasModule(_) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Module aliases are not yet supported"
                )
            }
            LocalInitializer::AliasComponent(_) => {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "Component aliases are not yet supported"
                )
            }
            LocalInitializer::Export(_name, component_item) => {
                match component_item {
                    ComponentItem::Func(i) => {
                        frame.component_funcs.push(frame.component_funcs[*i].clone());
                    }
                    ComponentItem::ComponentInstance(_) => {
                        let unwrap_instance = component_item.unwrap_instance();
                        self.component_export(frame, types, unwrap_instance)?;
                    }
                    ComponentItem::Type(_) => {
                        // do nothing
                    }
                    _ => unsupported_diag!(
                        self.context.diagnostics(),
                        "Exporting of {:?} is not yet supported",
                        component_item
                    ),
                }
            }
        }
        Ok(())
    }

    fn component_export(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        component_instance_idx: ComponentInstanceIndex,
    ) -> WasmResult<()> {
        let instance = &frame.component_instances[component_instance_idx].unwrap_instantiated();
        let static_component_idx = frame.components[instance.component].index;
        let parsed_component = &self.nested_components[static_component_idx];
        for (name, item) in parsed_component.exports.iter() {
            if let ComponentItem::Func(f) = item {
                self.define_component_export_lift_func(
                    frame,
                    types,
                    component_instance_idx,
                    name,
                    f,
                )?;
            } else {
                // we're only interested in exported functions
            }
        }
        frame.component_instances.push(ComponentInstanceDef::Export);
        Ok(())
    }

    fn define_component_export_lift_func(
        &mut self,
        frame: &ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        component_instance_idx: ComponentInstanceIndex,
        name: &str,
        f: &ComponentFuncIndex,
    ) -> WasmResult<()> {
        let nested_frame = &frame.frames[&component_instance_idx];
        let canon_lift = nested_frame.component_funcs[*f].unwrap_canon_lift();
        let type_func_idx = types.convert_component_func_type(frame.types, canon_lift.ty).unwrap();

        let component_types = types.resources_mut_and_types().1;
        let func_ty = convert_lifted_func_ty(CallConv::CanonLift, &type_func_idx, component_types);
        let core_export_func_path = self.core_module_export_func_path(frame, canon_lift);
        generate_export_lifting_function(
            &mut self.result,
            name,
            func_ty,
            core_export_func_path,
            self.context.diagnostics(),
        )?;
        Ok(())
    }

    fn core_module_export_func_path(
        &self,
        frame: &ComponentFrame<'a>,
        canon_lift: &CanonLift,
    ) -> SymbolPath {
        match &frame.funcs[canon_lift.func] {
            CoreDef::Export(module_instance_idx, name) => {
                match &frame.module_instances[*module_instance_idx] {
                    ModuleInstanceDef::Instantiated {
                        module_idx,
                        args: _,
                    } => match frame.modules[*module_idx] {
                        ModuleDef::Static(static_module_idx) => {
                            let parsed_module = &self.nested_modules[static_module_idx];
                            let func_idx = parsed_module.module.exports[*name].unwrap_func();
                            let func_name = parsed_module.module.func_name(func_idx);
                            let module_ident = parsed_module.module.name();
                            SymbolPath {
                                path: smallvec![
                                    SymbolNameComponent::Component(module_ident.as_symbol()),
                                    SymbolNameComponent::Leaf(func_name)
                                ],
                            }
                        }
                        ModuleDef::Import(_) => {
                            panic!("expected static module")
                        }
                    },
                    ModuleInstanceDef::Synthetic(_hash_map) => {
                        panic!("expected instantiated module")
                    }
                }
            }
            CoreDef::Lower(canon_lower) => {
                panic!("expected export, got {:?}", canon_lower)
            }
        }
    }

    fn module_instantiation(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        module_idx: &ModuleIndex,
        args: &'a FxHashMap<&str, ModuleInstanceIndex>,
    ) -> Result<(), Report> {
        let instance_idx = frame.module_instances.len() as u32;
        let current_module_idx = module_idx.as_u32();

        log::debug!(
            "Module instantiation: instance {} -> module {} (args: {:?})",
            instance_idx,
            current_module_idx,
            args.keys().collect::<Vec<_>>()
        );

        // Check if this module instantiation should be skipped (shim or fixup)
        if self.shim_bypass_info.shim_module_indices.contains(&current_module_idx) {
            log::warn!(
                "SKIPPING translation of shim module instance {} (module {})",
                instance_idx,
                current_module_idx
            );
            // Push a placeholder instance but don't do any translation
            frame.module_instances.push(ModuleInstanceDef::Instantiated {
                module_idx: *module_idx,
                args: args.clone(),
            });
            // Mark this instance as a shim instance for later redirection
            self.shim_bypass_info.shim_instance_indices.push(instance_idx);
            return Ok(());
        } else if self.shim_bypass_info.fixup_module_indices.contains(&current_module_idx) {
            log::warn!(
                "SKIPPING translation of fixup module instance {} (module {})",
                instance_idx,
                current_module_idx
            );
            // Push a placeholder instance but don't do any translation
            frame.module_instances.push(ModuleInstanceDef::Instantiated {
                module_idx: *module_idx,
                args: args.clone(),
            });
            return Ok(());
        }

        log::debug!(
            "Proceeding with normal translation of module instance {} (module {})",
            instance_idx,
            current_module_idx
        );
        frame.module_instances.push(ModuleInstanceDef::Instantiated {
            module_idx: *module_idx,
            args: args.clone(),
        });

        let mut import_canon_lower_args: FxHashMap<SymbolPath, ModuleArgument> =
            FxHashMap::default();
        match &frame.modules[*module_idx] {
            ModuleDef::Static(static_module_idx) => {
                let parsed_module = self.nested_modules.get_mut(*static_module_idx).unwrap();
                for module_arg in args {
                    dbg!(&module_arg);
                    let arg_module_name = module_arg.0;
                    if recover_imported_masm_module(arg_module_name).is_ok() {
                        // Skip processing module import if its an intrinsics, stdlib, tx-kernel, etc.
                        // They are processed in the core Wasm module translation
                        continue;
                    }

                    let module_path = SymbolPath {
                        path: smallvec![
                            SymbolNameComponent::Root,
                            SymbolNameComponent::Component(Symbol::intern(*arg_module_name))
                        ],
                    };

                    // Check if this argument references a shim instance
                    let actual_instance_idx = *module_arg.1;

                    let arg_module = &frame.module_instances[actual_instance_idx];
                    match arg_module {
                        ModuleInstanceDef::Instantiated {
                            module_idx: _,
                            args: _,
                        } => {
                            unsupported_diag!(
                                self.context.diagnostics(),
                                "Instantiated module as another module instantiation argument is \
                                 not supported yet"
                            )
                        }
                        ModuleInstanceDef::Synthetic(entities) => {
                            // module with CanonLower synthetic functions
                            for (func_name, entity) in entities.iter() {
                                log::trace!(
                                    "Processing synthetic function '{}' with entity {:?}",
                                    func_name,
                                    entity
                                );

                                let (signature, path) = canon_lower_func(
                                    frame,
                                    types,
                                    &module_path,
                                    func_name,
                                    entity,
                                    &self.shim_bypass_info,
                                )?;
                                log::trace!(
                                    "canon_lower_func returned signature '{}' for function '{}' \
                                     at path '{}'",
                                    signature,
                                    func_name,
                                    path
                                );
                                import_canon_lower_args
                                    .insert(path, ModuleArgument::ComponentImport(signature));
                            }
                        }
                    }
                }

                let module_types = types.module_types_builder();
                parsed_module.module.set_name_fallback(self.config.source_name.clone());
                if let Some(name_override) = self.config.override_name.as_ref() {
                    parsed_module.module.set_name_override(name_override.clone());
                }

                let module_name = parsed_module.module.name().as_str();
                let module_ref = self.result.define_module(Ident::from(module_name)).unwrap();
                let mut module_builder = ModuleBuilder::new(module_ref);
                let mut module_state = ModuleTranslationState::new(
                    &parsed_module.module,
                    &mut module_builder,
                    &mut self.world_builder,
                    module_types,
                    import_canon_lower_args,
                    self.context.diagnostics(),
                )?;

                build_ir_module(
                    parsed_module,
                    module_types,
                    &mut module_state,
                    self.config,
                    self.context.clone(),
                )?;
            }
            ModuleDef::Import(_) => {
                panic!("Module import instantiation is not supported yet")
            }
        };
        Ok(())
    }

    fn component_import(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        name: &wasmparser::ComponentImportName<'_>,
        ty: &ComponentEntityType,
    ) -> Result<(), Report> {
        let ty = types.convert_component_entity_type(frame.types, *ty).map_err(Report::msg)?;
        let ty = match ty {
            TypeDef::ComponentInstance(type_component_instance_idx) => type_component_instance_idx,
            _ => panic!("expected component instance"),
        };
        frame
            .component_instances
            .push(ComponentInstanceDef::Import(ComponentInstanceImport {
                name: name.0.to_string(),
                ty,
            }));

        Ok(())
    }
}

fn convert_lifted_func_ty(
    abi: CallConv,
    ty: &TypeFuncIndex,
    component_types: &super::ComponentTypes,
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
        abi,
    }
}

fn canon_lower_func(
    frame: &mut ComponentFrame,
    types: &mut ComponentTypesBuilder,
    module_path: &SymbolPath,
    func_name: &str,
    entity: &EntityIndex,
    shim_bypass_info: &ShimBypassInfo,
) -> WasmResult<(FunctionType, SymbolPath)> {
    let func_id = entity.unwrap_func();
    log::debug!("canon_lower_func: function '{}', func_id: {}", func_name, func_id.as_u32());

    let func_def = &frame.funcs[func_id];
    log::debug!("canon_lower_func: func_def at index {}: {:?}", func_id.as_u32(), func_def);

    // Check if the function at this index is an alias export instead of a canon lower
    match func_def {
        CoreDef::Lower(lower) => {
            let type_func_idx = types
                .convert_component_func_type(frame.types, lower.lower_ty)
                .map_err(Report::msg)?;

            let component_types = types.resources_mut_and_types().1;
            let func_ty =
                convert_lifted_func_ty(CallConv::CanonLower, &type_func_idx, component_types);

            let mut path = module_path.clone();
            path.path.push(SymbolNameComponent::Leaf(Symbol::intern(func_name)));

            Ok((func_ty, path))
        }
        CoreDef::Export(module_instance_idx, export_name) => canon_lower_from_alias_export(
            frame,
            types,
            module_path,
            func_name,
            module_instance_idx,
            export_name,
            shim_bypass_info,
        ),
    }
}

/// Handles the case where a function is an alias export from a module instance
/// instead of a direct canon lower definition.
fn canon_lower_from_alias_export(
    frame: &ComponentFrame,
    types: &mut ComponentTypesBuilder,
    module_path: &SymbolPath,
    func_name: &str,
    module_instance_idx: &ModuleInstanceIndex,
    export_name: &str,
    shim_bypass_info: &ShimBypassInfo,
) -> WasmResult<(FunctionType, SymbolPath)> {
    log::debug!(
        "Function {} is an alias export from module instance {} export '{}'",
        func_name,
        module_instance_idx.as_u32(),
        export_name
    );

    // Check if this is an alias export from a bypassed shim module
    if shim_bypass_info.shim_instance_indices.contains(&module_instance_idx.as_u32()) {
        log::debug!(
            "Alias export is from bypassed shim module instance {}",
            module_instance_idx.as_u32()
        );

        // This alias export is from a bypassed shim module. The canon lower function
        // that should have been provided by this shim module was lost during bypass.
        // We need to reconstruct the missing canon lower function.

        // Try to find the function type by looking for it in component instance exports
        let mut func_type_idx = None;

        // Search through component instances to find the export with this function name
        for (idx, inst_def) in frame.component_instances.iter() {
            if let ComponentInstanceDef::Import(import) = inst_def {
                // Get the component instance type
                let inst_ty = &types[import.ty];

                log::debug!(
                    "Checking component instance {} (import '{}') for function '{}'",
                    idx.as_u32(),
                    import.name,
                    func_name
                );

                // Check if this instance exports the function we're looking for
                if let Some(TypeDef::ComponentFunc(ty_idx)) = inst_ty.exports.get(func_name) {
                    func_type_idx = Some(*ty_idx);
                    log::debug!(
                        "Found function '{}' type in component instance '{}' exports: \
                         TypeFuncIndex({})",
                        func_name,
                        import.name,
                        ty_idx.as_u32()
                    );
                    break;
                }
            }
        }

        if let Some(type_func_idx) = func_type_idx {
            // We found the type information, use it to create the correct signature
            let component_types = types.resources_mut_and_types().1;
            let func_ty =
                convert_lifted_func_ty(CallConv::CanonLower, &type_func_idx, component_types);

            let mut path = module_path.clone();
            path.path.push(SymbolNameComponent::Leaf(Symbol::intern(func_name)));

            log::debug!("Created signature for '{}' from type information: {}", func_name, func_ty);

            Ok((func_ty, path))
        } else {
            Err(Report::msg(format!(
                "Could not find type information for function '{}' in component exports",
                func_name
            )))
        }
    } else {
        log::error!(
            "Alias export from non-bypassed module instance {} - this should not happen",
            module_instance_idx.as_u32()
        );
        Err(Report::msg("Unexpected alias export from non-bypassed module"))
    }
}

#[derive(Clone, Debug)]
enum ComponentInstanceDef<'a> {
    Import(ComponentInstanceImport),
    Instantiated(ComponentInstantiation<'a>),
    Export,
}
impl ComponentInstanceDef<'_> {
    fn unwrap_import(&self) -> ComponentInstanceImport {
        match self {
            ComponentInstanceDef::Import(import) => import.clone(),
            _ => panic!("expected import"),
        }
    }

    fn unwrap_instantiated(&self) -> &ComponentInstantiation {
        match self {
            ComponentInstanceDef::Instantiated(i) => i,
            _ => panic!("expected instantiated"),
        }
    }
}

#[derive(Debug, Clone)]
struct ComponentInstanceImport {
    name: String,
    ty: TypeComponentInstanceIndex,
}

#[derive(Clone, Debug)]
enum ComponentFuncDef<'a> {
    /// A host-imported component function.
    Import(ComponentInstanceIndex, &'a str, Option<TypeFuncIndex>),

    /// A core wasm function was lifted into a component function.
    Lifted(CanonLift),
}
impl ComponentFuncDef<'_> {
    fn unwrap_canon_lift(&self) -> &CanonLift {
        match self {
            ComponentFuncDef::Lifted(lift) => lift,
            _ => panic!("expected lift, got {:?}", self),
        }
    }
}

#[derive(Clone)]
enum ModuleDef {
    /// A core wasm module statically defined within the original component.
    ///
    /// The `StaticModuleIndex` indexes into the `static_modules` map in the
    /// `Inliner`.
    Static(StaticModuleIndex),

    /// A core wasm module that was imported from the host.
    Import(TypeModuleIndex),
}

/// "Closure state" for a component which is resolved from the `ClosedOverVars`
/// state that was calculated during translation.
#[derive(Default, Clone)]
struct ComponentClosure {
    modules: PrimaryMap<ModuleUpvarIndex, ModuleDef>,
    components: PrimaryMap<ComponentUpvarIndex, ComponentDef>,
}

#[derive(Clone)]
struct ComponentDef {
    index: StaticComponentIndex,
    closure: ComponentClosure,
}

/// Definition of a core wasm item and where it can come from within a
/// component.
#[derive(Debug, Clone)]
pub enum CoreDef<'a> {
    /// This item refers to an export of a previously instantiated core wasm
    /// instance.
    Export(ModuleInstanceIndex, &'a str),
    Lower(CanonLower),
}

impl CoreDef<'_> {
    pub fn unwrap_canon_lower(&self) -> &CanonLower {
        match self {
            CoreDef::Lower(lower) => lower,
            _ => panic!("expected lower"),
        }
    }
}

enum ModuleInstanceDef<'a> {
    /// A core wasm module instance was created through the instantiation of a
    /// module.
    Instantiated {
        module_idx: ModuleIndex,
        args: FxHashMap<&'a str, ModuleInstanceIndex>,
    },

    /// A "synthetic" core wasm module which is just a bag of named indices.
    Synthetic(&'a FxHashMap<&'a str, EntityIndex>),
}

/// Representation of all items which can be defined within a component.
///
/// This is the "value" of an item defined within a component and is used to
/// represent both imports and exports.
#[derive(Clone)]
enum ComponentItemDef<'a> {
    Component(ComponentDef),
    Instance(ComponentInstanceDef<'a>),
    Func(ComponentFuncDef<'a>),
    Module(ModuleDef),
    Type(TypeDef),
}

impl<'a> ComponentItemDef<'a> {
    fn from_import(
        name: &'a str,
        ty: TypeDef,
        component_instance_idx: ComponentInstanceIndex,
    ) -> ComponentItemDef<'a> {
        let item = match ty {
            TypeDef::Module(ty) => ComponentItemDef::Module(ModuleDef::Import(ty)),
            TypeDef::ComponentInstance(ty) => {
                ComponentItemDef::Instance(ComponentInstanceDef::Import(ComponentInstanceImport {
                    name: name.to_string(),
                    ty,
                }))
            }
            TypeDef::ComponentFunc(ty) => ComponentItemDef::Func(ComponentFuncDef::Import(
                component_instance_idx,
                name,
                Some(ty),
            )),
            TypeDef::Component(_ty) => panic!("root-level component imports are not supported"),
            TypeDef::Interface(_) | TypeDef::Resource(_) => ComponentItemDef::Type(ty),
        };
        item
    }
}

struct ComponentFrame<'a> {
    types: TypesRef<'a>,

    /// The "closure arguments" to this component, or otherwise the maps indexed
    /// by `ModuleUpvarIndex` and `ComponentUpvarIndex`. This is created when
    /// a component is created and stored as part of a component's state during
    /// inlining.
    closure: ComponentClosure,

    /// The arguments to the creation of this component.
    ///
    /// At the root level these are all imports from the host and between
    /// components this otherwise tracks how all the arguments are defined.
    args: FxHashMap<&'a str, ComponentItemDef<'a>>,

    // core wasm index spaces
    funcs: PrimaryMap<FuncIndex, CoreDef<'a>>,
    // memories: PrimaryMap<MemoryIndex, dfg::CoreExport<EntityIndex>>,
    // tables: PrimaryMap<TableIndex, dfg::CoreExport<EntityIndex>>,
    // globals: PrimaryMap<GlobalIndex, dfg::CoreExport<EntityIndex>>,
    modules: PrimaryMap<ModuleIndex, ModuleDef>,

    // component model index spaces
    component_funcs: PrimaryMap<ComponentFuncIndex, ComponentFuncDef<'a>>,
    module_instances: PrimaryMap<ModuleInstanceIndex, ModuleInstanceDef<'a>>,
    component_instances: PrimaryMap<ComponentInstanceIndex, ComponentInstanceDef<'a>>,
    frames: FxHashMap<ComponentInstanceIndex, ComponentFrame<'a>>,
    components: PrimaryMap<ComponentIndex, ComponentDef>,
}

impl<'a> ComponentFrame<'a> {
    fn new(types: TypesRef<'a>, args: FxHashMap<&'a str, ComponentItemDef<'a>>) -> Self {
        Self {
            types,
            funcs: PrimaryMap::new(),
            component_funcs: PrimaryMap::new(),
            component_instances: PrimaryMap::new(),
            components: PrimaryMap::new(),
            modules: PrimaryMap::new(),
            closure: Default::default(),
            module_instances: Default::default(),
            args,
            frames: Default::default(),
        }
    }

    fn closed_over_module(&self, index: &ClosedOverModule) -> ModuleDef {
        match *index {
            ClosedOverModule::Local(i) => self.modules[i].clone(),
            ClosedOverModule::Upvar(i) => self.closure.modules[i].clone(),
        }
    }

    fn closed_over_component(&self, index: &ClosedOverComponent) -> ComponentDef {
        match *index {
            ClosedOverComponent::Local(i) => self.components[i].clone(),
            ClosedOverComponent::Upvar(i) => self.closure.components[i].clone(),
        }
    }

    fn item(
        &self,
        index: ComponentItem,
        types: &mut ComponentTypesBuilder,
    ) -> WasmResult<ComponentItemDef<'a>> {
        Ok(match index {
            ComponentItem::Func(i) => ComponentItemDef::Func(self.component_funcs[i].clone()),
            ComponentItem::Component(i) => ComponentItemDef::Component(self.components[i].clone()),
            ComponentItem::ComponentInstance(i) => {
                ComponentItemDef::Instance(self.component_instances[i].clone())
            }
            ComponentItem::Module(i) => ComponentItemDef::Module(self.modules[i].clone()),
            ComponentItem::Type(t) => {
                ComponentItemDef::Type(types.convert_type(self.types, t).map_err(Report::msg)?)
            }
        })
    }

    /// Pushes the component `item` definition provided into the appropriate
    /// index space within this component.
    fn push_item(&mut self, item: ComponentItemDef<'a>) {
        match item {
            ComponentItemDef::Func(i) => {
                self.component_funcs.push(i);
            }
            ComponentItemDef::Module(i) => {
                self.modules.push(i);
            }
            ComponentItemDef::Component(i) => {
                self.components.push(i);
            }
            ComponentItemDef::Instance(i) => {
                self.component_instances.push(i);
            }
            ComponentItemDef::Type(_ty) => {}
        }
    }
}
