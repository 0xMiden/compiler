use std::rc::Rc;

use cranelift_entity::PrimaryMap;
use midenc_dialect_hir::WASM_COMPONENT_START_ATTR;
use midenc_frontend_wasm_metadata::{FrontendMetadata, ProtocolExportKind};
use midenc_hir::{
    self as hir2, BuilderExt, Context, FxHashMap, FxHashSet, Ident, OpExt, SymbolName,
    SymbolNameComponent, SymbolPath, SymbolTable,
    diagnostics::Report,
    dialects::builtin::{
        ComponentBuilder, FunctionRef, Module, ModuleBuilder, World, WorldBuilder,
        attributes::UnitAttr,
    },
    formatter::DisplayValues,
    interner::Symbol,
    smallvec,
};
use wasmparser::{component_types::ComponentEntityType, types::TypesRef};

use super::{
    CanonLift, CanonLower, ClosedOverComponent, ClosedOverModule, ComponentFuncIndex,
    ComponentFunctionType, ComponentIndex, ComponentInstanceIndex, ComponentInstantiation,
    ComponentTypesBuilder, ComponentUpvarIndex, ModuleIndex, ModuleInstanceIndex, ModuleUpvarIndex,
    ParsedComponent, StaticModuleIndex, TypeComponentInstanceIndex, TypeDef, TypeFuncIndex,
    TypeModuleIndex,
    flat::CanonicalAbiMode,
    shim_bypass::{self, ShimBypassInfo},
    start::{StartupAdapter, StartupAdapterFixup, classify_startup_adapter},
};
use crate::{
    FrontendOutput, WasmTranslationConfig,
    component::{
        ComponentItem, LocalInitializer, StaticComponentIndex, core_names,
        lift_exports::generate_export_lifting_function,
        naming::{ExportPaths, external_id_path},
    },
    error::WasmResult,
    module::{
        build_ir::build_ir_module,
        instance::ModuleArgument,
        module_env::{
            ParsedModule, collect_package_sections, merge_frontend_metadata,
            validate_lifted_frontend_metadata_exports,
        },
        module_translation_state::{ModuleTranslationState, core_import_path},
        types::{EntityIndex, FuncIndex},
    },
    unsupported_diag,
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

    /// Frontend metadata entries merged across all core modules that feed this component
    /// translation.
    component_frontend_metadata: Vec<FrontendMetadata>,

    /// The Miden paths of the function exports of the nested components.
    export_paths: ExportPaths<'a>,

    /// Miden paths of the component exports for which a lifting shim was emitted.
    lifted_export_paths: FxHashSet<String>,

    /// The core module instances to translate, in instantiation order.
    ///
    /// Their HIR is built after the initializer walk: the core functions are named after the
    /// exports they back, and wit-component lifts an interface's functions only after the core
    /// module is instantiated (and after the exports of the preceding interfaces).
    pending_modules: Vec<PendingModule>,

    /// The component exports to lift once the core modules are translated, in export order.
    pending_exports: Vec<PendingExport>,

    /// The core function a folded startup adapter runs, marked once the core modules are
    /// translated.
    pending_start: Option<(StaticModuleIndex, FuncIndex)>,

    /// For each Miden path of a lowered import, the core-import path of the first import that
    /// lowers to it.
    import_cm_paths: FxHashMap<SymbolPath, SymbolPath>,

    /// Information about shim modules to bypass
    shim_bypass_info: ShimBypassInfo,
}

/// A core module instance whose translation waits for the end of the initializer walk.
struct PendingModule {
    /// The instantiated module.
    static_module_idx: StaticModuleIndex,
    /// The instantiation arguments filling the module's imports, keyed by core-import path.
    import_canon_lower_args: FxHashMap<SymbolPath, ModuleArgument>,
}

/// A component export whose lifting waits for the translation of the core modules.
struct PendingExport {
    /// The core function the export lifts.
    core_func: (StaticModuleIndex, FuncIndex),
    /// The Miden path of the export.
    path: SymbolPath,
    /// The component-level type of the export.
    func_ty: ComponentFunctionType,
    /// The parameter names of the export.
    param_names: Box<[String]>,
    /// The protocol role of the export, if any.
    protocol_export_kind: Option<ProtocolExportKind>,
}

impl<'a> ComponentTranslator<'a> {
    /// Detect shim and fixup modules in the component
    fn detect_shim_modules(&mut self, root_component: &ParsedComponent) {
        log::debug!(target: "component-translator",
            "Component has {} initializers and {} static modules",
            root_component.initializers.len(),
            self.nested_modules.len()
        );

        // First, check all static modules
        for (static_idx, module) in self.nested_modules.iter() {
            log::debug!(target: "component-translator",
                "Static module {}: exports={}, imports={}",
                static_idx.as_u32(),
                DisplayValues::new(module.module.exports.keys()),
                DisplayValues::new(module.module.imports.iter()),
            );

            if shim_bypass::is_shim_module(module) {
                log::info!(target: "component-translator", "Detected shim module at static index {}", static_idx.as_u32());
                self.shim_bypass_info.shim_static_modules.push(static_idx.as_u32());
            } else if shim_bypass::is_fixup_module(module) {
                log::info!(target: "component-translator", "Detected fixup module at static index {}", static_idx.as_u32());
                self.shim_bypass_info.fixup_static_modules.push(static_idx.as_u32());
            }
        }

        for (i, init) in root_component.initializers.iter().enumerate() {
            log::trace!(target: "component-translator", "Initializer {}: {:?}", i, std::mem::discriminant(init));
        }

        log::debug!(target: "component-translator",
            "Shim bypass info: shim_static_modules={:?}, fixup_static_modules={:?}",
            self.shim_bypass_info.shim_static_modules,
            self.shim_bypass_info.fixup_static_modules
        );
    }

    /// Creates a translator that defines the component `name` (its `::`-joined namespace path)
    /// in the configured world, or in a new one, naming the lifted exports of the nested
    /// components by `export_paths`.
    pub fn new(
        name: SymbolName,
        export_paths: ExportPaths<'a>,
        nested_modules: &'a mut PrimaryMap<StaticModuleIndex, ParsedModule<'a>>,
        nested_components: &'a PrimaryMap<StaticComponentIndex, ParsedComponent<'a>>,
        config: &'a WasmTranslationConfig,
        context: Rc<Context>,
    ) -> WasmResult<Self> {
        let component_frontend_metadata =
            merge_frontend_metadata(nested_modules.iter().map(|(_, module)| module));

        // If a world wasn't provided to us, create one
        let world_ref = match config.world {
            Some(world) => world,
            None => context.clone().builder().create::<World, ()>(Default::default())()
                .expect("failed to create world"),
        };
        let mut world_builder = WorldBuilder::new(world_ref);

        let raw_entity_ref = world_builder
            .define_component(hir2::Ident::with_empty_span(name))
            .expect("failed to define component");
        let result = ComponentBuilder::new(raw_entity_ref);

        Ok(Self {
            config,
            context,
            nested_modules,
            nested_components,
            world_builder,
            result,
            shim_bypass_info: ShimBypassInfo::default(),
            component_frontend_metadata,
            export_paths,
            lifted_export_paths: FxHashSet::default(),
            pending_modules: Vec::new(),
            pending_exports: Vec::new(),
            pending_start: None,
            import_cm_paths: FxHashMap::default(),
        })
    }

    pub fn translate2(
        mut self,
        root_component: &'a ParsedComponent,
        types: &mut ComponentTypesBuilder,
    ) -> WasmResult<FrontendOutput> {
        self.detect_shim_modules(root_component);
        self.register_component_export_type_names(root_component, types)?;

        let mut frame = ComponentFrame::new(root_component.types_ref(), FxHashMap::default());

        for init in &root_component.initializers {
            self.initializer(&mut frame, types, init)?;
        }
        self.translate_pending(types)?;

        validate_lifted_frontend_metadata_exports(
            &self.component_frontend_metadata,
            &self.lifted_export_paths,
        )?;

        let sections = collect_package_sections(
            self.nested_modules.iter().map(|(_, module)| module),
            self.context.diagnostics(),
        )?;

        let output = FrontendOutput {
            component: self.result.component,
            sections,
        };
        Ok(output)
    }

    fn initializer(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        init: &'a LocalInitializer<'a>,
    ) -> WasmResult<()> {
        log::trace!(target: "component-translator", "init: {init:?}");
        match init {
            LocalInitializer::Import(name, ty) => {
                match frame.args.get(name.name) {
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
                log::debug!(target: "component-translator", "Adding canon lower function: {lower:?}");
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
            LocalInitializer::ResourceDrop(..) => {
                unsupported_diag!(self.context.diagnostics(), "Resource dropping is not supported")
            }
            LocalInitializer::ModuleStatic(static_module_idx) => {
                let module_idx = frame.modules.len() as u32;
                frame.modules.push(ModuleDef::Static(*static_module_idx));

                // Track the mapping from frame module index to static module index for shim/fixup modules
                if self.shim_bypass_info.shim_static_modules.contains(&static_module_idx.as_u32()) {
                    log::warn!(target: "component-translator",
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
                    log::warn!(target: "component-translator",
                        "Marking frame module {} as fixup module (static {})",
                        module_idx,
                        static_module_idx.as_u32()
                    );
                    self.shim_bypass_info.fixup_module_indices.push(module_idx);
                }
            }
            LocalInitializer::ModuleInstantiate(module_idx, args) => {
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
                    log::trace!(target: "component-translator",
                        "Detected shim-related synthetic instance at index {instance_idx}"
                    );
                    // This synthetic instance contains shim exports, mark it for bypass
                    self.shim_bypass_info.shim_instance_indices.push(instance_idx);
                }
            }
            LocalInitializer::ComponentStatic(idx, vars) => {
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
                    args,
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
                log::debug!(target: "component-translator",
                    "Processing {} nested component initializers for instance",
                    translation.initializers.len()
                );
                for (i, init) in translation.initializers.iter().enumerate() {
                    log::trace!(target: "component-translator", "Processing nested initializer {i}: {init:?}");
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
                log::debug!(target: "component-translator",
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
                    log::trace!(target: "component-translator",
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
            LocalInitializer::Export(name, component_item) => match component_item {
                ComponentItem::Func(i) => {
                    frame.component_funcs.push(frame.component_funcs[*i].clone());
                }
                ComponentItem::ComponentInstance(_) => {
                    let unwrap_instance = component_item.unwrap_instance();
                    self.component_export(frame, types, name, unwrap_instance)?;
                }
                ComponentItem::Type(ty) => {
                    let ty = types.convert_type(frame.types, *ty).map_err(Report::msg)?;
                    types.register_type_name(ty, (*name).to_owned());
                }
                _ => unsupported_diag!(
                    self.context.diagnostics(),
                    "Exporting of {:?} is not yet supported",
                    component_item
                ),
            },
        }
        Ok(())
    }

    /// Lifts every function exported by the component instance `component_instance_idx`, which
    /// the root component exports as the interface `interface`.
    fn component_export(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        interface: &str,
        component_instance_idx: ComponentInstanceIndex,
    ) -> WasmResult<()> {
        let instance = &frame.component_instances[component_instance_idx].unwrap_instantiated();
        let static_component_idx = frame.components[instance.component].index;
        let parsed_component = &self.nested_components[static_component_idx];
        self.register_component_export_type_names(parsed_component, types)?;
        for (name, item) in parsed_component.exports.iter() {
            if let ComponentItem::Func(f) = item {
                let path =
                    self.export_paths.get(&(static_component_idx, *name)).cloned().ok_or_else(
                        || {
                            Report::msg(format!(
                                "export `{name}` of interface `{interface}` has no Miden path"
                            ))
                        },
                    )?;
                self.define_component_export_lift_func(
                    frame,
                    types,
                    component_instance_idx,
                    &path,
                    f,
                )?;
            } else {
                // we're only interested in exported functions
            }
        }
        frame.component_instances.push(ComponentInstanceDef::Export);
        Ok(())
    }

    fn register_component_export_type_names(
        &self,
        parsed_component: &ParsedComponent<'a>,
        types: &mut ComponentTypesBuilder,
    ) -> WasmResult<()> {
        let component_types = parsed_component.types_ref();
        for (name, item) in parsed_component.exports.iter() {
            if let ComponentItem::Type(ty) = item {
                let ty = types.convert_type(component_types, *ty).map_err(Report::msg)?;
                types.register_type_name(ty, (*name).to_owned());
            }
        }
        Ok(())
    }

    /// Records the lifted function of a component export at its Miden path `path`, which is
    /// `<component namespace>::<leaf>`, to be defined once the core modules are translated.
    fn define_component_export_lift_func(
        &mut self,
        frame: &ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        component_instance_idx: ComponentInstanceIndex,
        path: &SymbolPath,
        f: &ComponentFuncIndex,
    ) -> WasmResult<()> {
        let nested_frame = &frame.frames[&component_instance_idx];
        let canon_lift = nested_frame.component_funcs[*f].unwrap_canon_lift();
        let type_func_idx = types.convert_component_func_type(frame.types, canon_lift.ty).unwrap();

        let component_types = types.resources_mut_and_types().1;
        let param_names = component_types[type_func_idx].param_names.clone();
        let func_ty =
            convert_lifted_func_ty(CanonicalAbiMode::Export, &type_func_idx, component_types);
        let core_func = self.core_func_of_lift(frame, canon_lift);
        let path_name = path.to_string();
        let protocol_export_kind: Option<ProtocolExportKind> = self
            .component_frontend_metadata
            .iter()
            .find_map(|metadata| metadata.protocol_export_kind_for(&path_name));

        self.pending_exports.push(PendingExport {
            core_func,
            path: path.clone(),
            func_ty,
            param_names,
            protocol_export_kind,
        });
        Ok(())
    }

    /// Translates the recorded core modules, marks the startup function and lifts the recorded
    /// exports.
    fn translate_pending(&mut self, types: &ComponentTypesBuilder) -> WasmResult<()> {
        let mut core_funcs: FxHashMap<(StaticModuleIndex, FuncIndex), FunctionRef> =
            FxHashMap::default();
        for PendingModule {
            static_module_idx,
            import_canon_lower_args,
        } in core::mem::take(&mut self.pending_modules)
        {
            let parsed_module = self.nested_modules.get_mut(static_module_idx).unwrap();
            parsed_module.module.set_name_fallback(self.config.source_name.clone());
            if let Some(name_override) = self.config.override_name.as_ref() {
                parsed_module.module.set_name_override(name_override.clone());
            }

            let module = &parsed_module.module;
            let exports = self
                .pending_exports
                .iter()
                .filter(|export| export.core_func.0 == static_module_idx)
                .map(|export| (export.core_func.1, &export.path));
            let imports = module
                .functions
                .keys()
                .filter(|index| module.is_imported_function(*index))
                .filter_map(|index| {
                    let import = &module.imports[index.as_u32() as usize];
                    match import_canon_lower_args.get(&core_import_path(import)) {
                        Some(ModuleArgument::ComponentImport { path, .. }) => Some((index, path)),
                        _ => None,
                    }
                });
            let names = core_names::assign(module, exports, imports)?;

            let module_types = types.module_types_builder();
            let module_name = module.name().as_str();
            let module_ref = self.result.define_module(Ident::from(module_name)).unwrap();
            let mut module_builder = ModuleBuilder::new(module_ref);
            let mut module_state = ModuleTranslationState::new(
                module,
                &mut module_builder,
                &mut self.world_builder,
                module_types,
                import_canon_lower_args,
                &names,
                self.context.diagnostics(),
            )?;
            build_ir_module(
                parsed_module,
                module_types,
                &mut module_state,
                self.config,
                self.context.clone(),
            )?;
            core_funcs.extend(
                module_state
                    .defined_functions()
                    .map(|(func_idx, function_ref)| ((static_module_idx, func_idx), function_ref)),
            );
        }

        if let Some(start) = self.pending_start {
            let Some(mut function_ref) = core_funcs.get(&start).copied() else {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "failed to resolve the core Wasm startup adapter target function"
                )
            };
            let marker = self.context.create_attribute::<UnitAttr, _>(());
            function_ref.borrow_mut().set_attribute(WASM_COMPONENT_START_ATTR, marker);
        }

        for PendingExport {
            core_func,
            path,
            func_ty,
            param_names,
            protocol_export_kind,
        } in core::mem::take(&mut self.pending_exports)
        {
            self.ensure_export_leaf_is_free(&path)?;
            let Some(core_func_ref) = core_funcs.get(&core_func).copied() else {
                return Err(Report::msg(format!(
                    "the core function lifted as `{path}` is not defined by a translated core \
                     module"
                )));
            };
            generate_export_lifting_function(
                &mut self.result,
                core_func_ref,
                path.name().as_str(),
                func_ty,
                &param_names,
                protocol_export_kind,
                self.context.diagnostics(),
            )?;
            self.lifted_export_paths.insert(path.to_string());
        }
        Ok(())
    }

    /// Reports an error when the component already holds a symbol named by the leaf of the
    /// export path `path`, i.e. when the export clashes with the core module of the same name.
    ///
    /// Two exports sharing one path are rejected earlier, by the export pre-scan.
    fn ensure_export_leaf_is_free(&self, path: &SymbolPath) -> WasmResult<()> {
        let leaf = path.name();
        let component = self.result.component.borrow();
        let Some(symbol) = component.get(leaf) else {
            return Ok(());
        };
        let message = if symbol.borrow().as_symbol_operation().is::<Module>() {
            format!(
                "export `{leaf}` of `{}` clashes with the core module `{leaf}` of the same name",
                component.namespace_path()
            )
        } else {
            // Internal error: the export pre-scan (`exports_namespace`) rejects two exports of
            // one path before any export is lifted.
            format!(
                "export `{path}` clashes with the component symbol `{leaf}` (two exports of one \
                 path are rejected by the export pre-scan)"
            )
        };
        Err(Report::msg(message))
    }

    /// Returns the core function `canon_lift` lifts, identified by its static module and index.
    fn core_func_of_lift(
        &self,
        frame: &ComponentFrame<'a>,
        canon_lift: &CanonLift,
    ) -> (StaticModuleIndex, FuncIndex) {
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
                            (static_module_idx, func_idx)
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
                panic!("expected export, got {canon_lower:?}")
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

        log::debug!(target: "component-translator",
            "Module instantiation: instance {} -> module {} (args: {})",
            instance_idx,
            current_module_idx,
            DisplayValues::new(args.keys())
        );

        let startup_adapter = match &frame.modules[*module_idx] {
            ModuleDef::Static(static_module_idx) => classify_startup_adapter(
                &self.nested_modules[*static_module_idx],
                types.module_types_builder(),
            ),
            ModuleDef::Import(_) => None,
        };
        if let Some(adapter) = startup_adapter {
            return self.fold_startup_adapter(frame, types, module_idx, args, adapter);
        }

        // Check if this module instantiation should be skipped (shim or fixup)
        if self.shim_bypass_info.shim_module_indices.contains(&current_module_idx) {
            log::warn!(target: "component-translator",
                "SKIPPING translation of shim module instance {instance_idx} (module {current_module_idx})"
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
            log::warn!(target: "component-translator",
                "SKIPPING translation of fixup module instance {instance_idx} (module {current_module_idx})"
            );
            // Push a placeholder instance but don't do any translation
            frame.module_instances.push(ModuleInstanceDef::Instantiated {
                module_idx: *module_idx,
                args: args.clone(),
            });
            return Ok(());
        }

        log::debug!(target: "component-translator",
            "Proceeding with normal translation of module instance {instance_idx} (module {current_module_idx})"
        );
        frame.module_instances.push(ModuleInstanceDef::Instantiated {
            module_idx: *module_idx,
            args: args.clone(),
        });

        let mut import_canon_lower_args: FxHashMap<SymbolPath, ModuleArgument> =
            FxHashMap::default();
        match frame.modules[*module_idx] {
            ModuleDef::Static(static_module_idx) => {
                for module_arg in args {
                    let arg_module_name = module_arg.0;
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
                                log::trace!(target: "component-translator",
                                    "Processing synthetic function '{func_name}' with entity {entity:?}"
                                );

                                let (signature, cm_path, path) = canon_lower_func(
                                    frame,
                                    types,
                                    arg_module_name,
                                    &module_path,
                                    func_name,
                                    entity,
                                    &self.shim_bypass_info,
                                )?;
                                log::trace!(target: "component-translator",
                                    "canon_lower_func returned signature '{}' for function '{func_name}' \
                                     at path '{cm_path}' (Miden path '{path}')"
                                    , signature.ir
                                );
                                let first_cm_path = self
                                    .import_cm_paths
                                    .entry(path.clone())
                                    .or_insert_with(|| cm_path.clone())
                                    .clone();
                                import_canon_lower_args.insert(
                                    cm_path,
                                    ModuleArgument::ComponentImport {
                                        signature,
                                        path,
                                        first_cm_path,
                                        namespace: self
                                            .result
                                            .component
                                            .borrow()
                                            .namespace_path()
                                            .to_symbol_name(),
                                    },
                                );
                            }
                        }
                    }
                }

                self.pending_modules.push(PendingModule {
                    static_module_idx,
                    import_canon_lower_args,
                });
            }
            ModuleDef::Import(_) => {
                panic!("Module import instantiation is not supported yet")
            }
        };
        Ok(())
    }

    /// Fold a supported startup adapter and record the defined function it resolves to, which is
    /// marked once the core modules are translated.
    fn fold_startup_adapter(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &ComponentTypesBuilder,
        module_idx: &ModuleIndex,
        args: &'a FxHashMap<&str, ModuleInstanceIndex>,
        adapter: StartupAdapter,
    ) -> Result<(), Report> {
        if self.pending_start.is_some() {
            unsupported_diag!(
                self.context.diagnostics(),
                "multiple core Wasm startup adapters in one component are not supported"
            )
        }

        if let Some(fixup) = adapter.fixup.as_ref() {
            self.validate_startup_fixup(frame, args, adapter.start.module.as_str(), fixup)?;
        } else if args.len() != 1 || !args.contains_key(adapter.start.module.as_str()) {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter instantiation arguments do not match its imports"
            )
        }

        let Some(target_instance_idx) = args.get(adapter.start.module.as_str()).copied() else {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter import module '{}' has no matching instantiation \
                 argument",
                adapter.start.module
            )
        };
        let ModuleInstanceDef::Instantiated {
            module_idx: target_module_idx,
            args: _,
        } = &frame.module_instances[target_instance_idx]
        else {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target must be an already-instantiated static module"
            )
        };
        let target_module_index = target_module_idx.as_u32();
        if self.shim_bypass_info.shim_module_indices.contains(&target_module_index)
            || self.shim_bypass_info.fixup_module_indices.contains(&target_module_index)
        {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target must be a translated static module"
            )
        }
        let ModuleDef::Static(target_static_module_idx) = frame.modules[*target_module_idx] else {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target must be an already-instantiated static module"
            )
        };

        let target_module = &self.nested_modules[target_static_module_idx].module;
        let Some(target_entity) = target_module.exports.get(adapter.start.field.as_str()).copied()
        else {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target instance does not export '{}'",
                adapter.start.field
            )
        };
        let EntityIndex::Function(target_func_idx) = target_entity else {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target export '{}' is not a function",
                adapter.start.field
            )
        };
        if target_module.is_imported_function(target_func_idx) {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target export '{}' must resolve to a defined function",
                adapter.start.field
            )
        }

        // Core functions are translated with the `C` calling convention, so the Wasm signature
        // decides whether the target fits.
        let signature =
            &types.module_types_builder()[target_module.functions[target_func_idx].signature];
        if !signature.params().is_empty() || !signature.returns().is_empty() {
            unsupported_diag!(
                self.context.diagnostics(),
                "core Wasm startup adapter target must be a defined `C` function with signature \
                 `() -> ()`"
            )
        }
        self.pending_start = Some((target_static_module_idx, target_func_idx));

        // Preserve component-model instance index numbering without emitting HIR for the adapter.
        frame.module_instances.push(ModuleInstanceDef::Instantiated {
            module_idx: *module_idx,
            args: args.clone(),
        });
        Ok(())
    }

    /// Validate that a combined startup/fixup adapter only wires canonical lowers into a shim
    /// whose indirection has already been removed by the frontend.
    fn validate_startup_fixup(
        &self,
        frame: &ComponentFrame<'a>,
        args: &'a FxHashMap<&str, ModuleInstanceIndex>,
        start_module: &str,
        fixup: &StartupAdapterFixup,
    ) -> Result<(), Report> {
        let mut expected_args = FxHashSet::default();
        expected_args.insert(start_module);
        expected_args.insert(fixup.table.module.as_str());
        for function in &fixup.functions {
            expected_args.insert(function.module.as_str());
        }

        let Some(table_instance_idx) = args.get(fixup.table.module.as_str()).copied() else {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter has no shim-table argument for '{}'",
                fixup.table.module
            )
        };
        let ModuleInstanceDef::Instantiated {
            module_idx: shim_module_idx,
            args: _,
        } = &frame.module_instances[table_instance_idx]
        else {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter table must come from an instantiated \
                 shim module"
            )
        };
        if !self.shim_bypass_info.shim_module_indices.contains(&shim_module_idx.as_u32())
            || !self
                .shim_bypass_info
                .shim_instance_indices
                .contains(&table_instance_idx.as_u32())
        {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter table must come from a bypassed shim \
                 module"
            )
        }
        let ModuleDef::Static(shim_static_module_idx) = frame.modules[*shim_module_idx] else {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter table must come from a static shim \
                 module"
            )
        };
        let shim_module = &self.nested_modules[shim_static_module_idx].module;
        let Some(EntityIndex::Table(_)) =
            shim_module.exports.get(fixup.table.field.as_str()).copied()
        else {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter shim does not export table '{}'",
                fixup.table.field
            )
        };

        let mut resolved_functions = FxHashSet::default();
        for function in &fixup.functions {
            let Some(function_instance_idx) = args.get(function.module.as_str()).copied() else {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter has no function argument for '{}'",
                    function.module
                )
            };
            let ModuleInstanceDef::Synthetic(entities) =
                &frame.module_instances[function_instance_idx]
            else {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter functions must come from a \
                     synthetic canonical-lower instance"
                )
            };
            let Some(EntityIndex::Function(function_idx)) =
                entities.get(function.field.as_str()).copied()
            else {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter function argument does not export \
                     '{}'",
                    function.field
                )
            };
            let CoreDef::Lower(lower) = &frame.funcs[function_idx] else {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter functions must resolve to direct \
                     canonical lowers"
                )
            };
            if !matches!(frame.component_funcs[lower.func], ComponentFuncDef::Import(..)) {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter functions must lower component \
                     imports"
                )
            }
            if !resolved_functions.insert(function_idx) {
                unsupported_diag!(
                    self.context.diagnostics(),
                    "combined core Wasm startup/fixup adapter functions must be distinct"
                )
            }
        }

        // The start target itself is validated by the ordinary startup fold below. Check its key
        // here so the combined adapter cannot smuggle unrelated instantiation arguments.
        if args.len() != expected_args.len() || args.keys().any(|key| !expected_args.contains(*key))
        {
            unsupported_diag!(
                self.context.diagnostics(),
                "combined core Wasm startup/fixup adapter instantiation arguments do not match \
                 its imports"
            )
        }

        Ok(())
    }

    /// Records the component instance import `name` of type `ty`, registering the names of the
    /// types it exports.
    fn component_import(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        name: &wasmparser::ComponentExternName<'_>,
        ty: &ComponentEntityType,
    ) -> Result<(), Report> {
        let ty = types.convert_component_entity_type(frame.types, *ty).map_err(Report::msg)?;
        let ty = match ty {
            TypeDef::ComponentInstance(type_component_instance_idx) => type_component_instance_idx,
            _ => panic!("expected component instance"),
        };
        types.register_component_instance_export_type_names(ty, Some(name.name));
        frame
            .component_instances
            .push(ComponentInstanceDef::Import(ComponentInstanceImport {
                name: name.name.to_string(),
                ty,
            }));

        Ok(())
    }
}

fn convert_lifted_func_ty(
    _mode: CanonicalAbiMode,
    ty: &TypeFuncIndex,
    component_types: &super::ComponentTypes,
) -> ComponentFunctionType {
    ComponentFunctionType::from_component_type(&component_types[*ty], component_types)
}

/// Resolves the core instantiation argument `func_name` of the argument module `import_name`
/// (whose component-model path is `module_path`) to the component import it lowers.
///
/// Returns the import's signature, the component-model path `module_path::func_name` the core
/// import is matched by, and the Miden path from the import's `external-id`.
fn canon_lower_func(
    frame: &mut ComponentFrame,
    types: &mut ComponentTypesBuilder,
    import_name: &str,
    module_path: &SymbolPath,
    func_name: &str,
    entity: &EntityIndex,
    shim_bypass_info: &ShimBypassInfo,
) -> WasmResult<(ComponentFunctionType, SymbolPath, SymbolPath)> {
    let func_id = entity.unwrap_func();
    log::debug!(target: "component-translator", "canon_lower_func: function '{}', func_id: {}", func_name, func_id.as_u32());

    let func_def = &frame.funcs[func_id];
    log::debug!(target: "component-translator", "canon_lower_func: func_def at index {}: {:?}", func_id.as_u32(), func_def);

    // Check if the function at this index is an alias export instead of a canon lower
    match func_def {
        CoreDef::Lower(lower) => {
            let type_func_idx = types
                .convert_component_func_type(frame.types, lower.lower_ty)
                .map_err(Report::msg)?;

            let component_types = types.resources_mut_and_types().1;
            let func_ty =
                convert_lifted_func_ty(CanonicalAbiMode::Import, &type_func_idx, component_types);

            let mut cm_path = module_path.clone();
            cm_path.path.push(SymbolNameComponent::Leaf(Symbol::intern(func_name)));

            let ComponentFuncDef::Import(instance_idx, import_func_name, _) =
                &frame.component_funcs[lower.func]
            else {
                return Err(Report::msg(format!(
                    "canon lower for '{func_name}' does not lower an imported component function"
                )));
            };
            let ComponentInstanceDef::Import(import) = &frame.component_instances[*instance_idx]
            else {
                return Err(Report::msg(format!(
                    "canon lower for '{func_name}' does not lower a function of an imported \
                     component instance"
                )));
            };
            let path = external_id_path(
                &import.name,
                import_func_name,
                types[import.ty].external_ids.get(*import_func_name).map(String::as_str),
            )?;

            Ok((func_ty, cm_path, path))
        }
        CoreDef::Export(module_instance_idx, export_name) => canon_lower_from_alias_export(
            frame,
            types,
            import_name,
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
///
/// The function is resolved in the imported component instance named `import_name`.
#[allow(clippy::too_many_arguments)]
fn canon_lower_from_alias_export(
    frame: &ComponentFrame,
    types: &mut ComponentTypesBuilder,
    import_name: &str,
    module_path: &SymbolPath,
    func_name: &str,
    module_instance_idx: &ModuleInstanceIndex,
    export_name: &str,
    shim_bypass_info: &ShimBypassInfo,
) -> WasmResult<(ComponentFunctionType, SymbolPath, SymbolPath)> {
    log::debug!(target: "component-translator",
        "Function {} is an alias export from module instance {} export '{}'",
        func_name,
        module_instance_idx.as_u32(),
        export_name
    );

    // Check if this is an alias export from a bypassed shim module
    if shim_bypass_info.shim_instance_indices.contains(&module_instance_idx.as_u32()) {
        log::debug!(target: "component-translator",
            "Alias export is from bypassed shim module instance {}",
            module_instance_idx.as_u32()
        );

        // This alias export is from a bypassed shim module. The canon lower function
        // that should have been provided by this shim module was lost during bypass.
        // We need to reconstruct the missing canon lower function.

        // The core instantiation argument is named after the component-model import it lowers
        // from, so the function belongs to the imported instance of that name.
        let import = frame
            .component_instances
            .values()
            .find_map(|inst_def| match inst_def {
                ComponentInstanceDef::Import(import) if import.name == import_name => Some(import),
                _ => None,
            })
            .ok_or_else(|| {
                Report::msg(format!(
                    "the core import '{func_name}' of '{import_name}' does not come from an \
                     imported component instance"
                ))
            })?;
        let inst_ty = &types[import.ty];
        let Some(TypeDef::ComponentFunc(type_func_idx)) = inst_ty.exports.get(func_name) else {
            return Err(Report::msg(format!(
                "the imported component instance '{import_name}' does not export the function \
                 '{func_name}'"
            )));
        };
        let type_func_idx = *type_func_idx;
        let miden_path = external_id_path(
            import_name,
            func_name,
            inst_ty.external_ids.get(func_name).map(String::as_str),
        )?;

        let component_types = types.resources_mut_and_types().1;
        let func_ty =
            convert_lifted_func_ty(CanonicalAbiMode::Import, &type_func_idx, component_types);

        let mut path = module_path.clone();
        path.path.push(SymbolNameComponent::Leaf(Symbol::intern(func_name)));

        log::debug!(target: "component-translator", "Created signature for '{func_name}' from type information: {}", func_ty.ir);

        Ok((func_ty, path, miden_path))
    } else {
        log::error!(target: "component-translator",
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

    fn unwrap_instantiated(&self) -> &ComponentInstantiation<'_> {
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
            _ => panic!("expected lift, got {self:?}"),
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
        match ty {
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
        }
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use midenc_dialect_hir::WASM_COMPONENT_START_ATTR;
    use midenc_hir::{
        Context, Op, OpExt, SymbolName,
        dialects::builtin::{ComponentBuilder, ModuleBuilder, attributes::UnitAttr},
    };

    use crate::{WasmTranslationConfig, translate};

    /// The configuration of a build rooting the component at a fixed namespace, which these
    /// components (having no function exports) need.
    fn config() -> WasmTranslationConfig {
        WasmTranslationConfig {
            namespace: Some(midenc_hir::SymbolPath::from_masm_module_id("miden::test::test")),
            ..Default::default()
        }
    }

    fn translate_wat(wat: &str) -> (Rc<Context>, ComponentBuilder) {
        let wasm = wat::parse_str(wat).expect("component WAT should compile");
        let context = Rc::new(Context::default());
        let output =
            translate(&wasm, &config(), context.clone()).expect("component should translate");
        (context, ComponentBuilder::new(output.component))
    }

    fn combined_startup_fixup_wat(actual_function: &str, table_instance: &str) -> String {
        format!(
            r#"
            (component
                (type $host-type (func))
                (type $host-instance-type (instance
                    (export "run" (func (type $host-type)))
                ))
                (import "host" (instance $host-instance (type $host-instance-type)))
                (alias export $host-instance "run" (func $host))
                (core func $lowered (canon lower (func $host)))

                (core module $shim
                    (type $shim-type (func))
                    (table (export "$imports") 1 1 funcref)
                    (func (export "0") (type $shim-type))
                )
                (core instance $shim-instance (instantiate $shim))
                (alias core export $shim-instance "0" (core func $shim-function))
                (alias core export $shim-instance "$imports" (core table $shim-table))
                (core instance $synthetic-shim
                    (export "$imports" (table $shim-table)))
                (core instance $actual
                    (export "0" (func {actual_function})))

                (core module $main
                    (func $actual-start (export "aliased-start"))
                )
                (core instance $main-instance (instantiate $main))

                (core module $fixup
                    (type $actual-type (func))
                    (type $start-type (func))
                    (import "actual-functions" "0" (func $actual (type $actual-type)))
                    (import "not-main" "aliased-start" (func $start (type $start-type)))
                    (import "indirect-shim" "$imports" (table 1 1 funcref))
                    (start $start)
                    (elem (i32.const 0) func $actual)
                )
                (core instance $fixup-instance
                    (instantiate $fixup
                        (with "actual-functions" (instance $actual))
                        (with "not-main" (instance $main-instance))
                        (with "indirect-shim" (instance {table_instance}))))

                (component $export-component)
                (instance $exports (instantiate $export-component))
                (export "miden:test/component@1.0.0" (instance $exports))
            )
            "#
        )
    }

    #[test]
    fn folds_startup_adapter_and_marks_resolved_definition() {
        let (_context, component) = translate_wat(
            r#"
            (component
                (core module $main
                    (func $actual-start (export "aliased-start"))
                )
                (core instance $main-instance (instantiate $main))
                (core module $adapter
                    (import "not-main" "aliased-start" (func $start))
                    (start $start)
                )
                (core instance $adapter-instance
                    (instantiate $adapter
                        (with "not-main" (instance $main-instance))))
                (component $export-component)
                (instance $exports (instantiate $export-component))
                (export "miden:test/component@1.0.0" (instance $exports))
            )
            "#,
        );

        let main = component
            .find_module(SymbolName::intern("main"))
            .expect("main module should be translated");
        let start = ModuleBuilder::new(main)
            .get_function("actual-start")
            .expect("actual start definition should be translated");
        assert!(
            start
                .borrow()
                .as_operation()
                .get_typed_attribute::<UnitAttr>(WASM_COMPONENT_START_ATTR)
                .is_some(),
            "resolved function should carry the typed start marker"
        );
        assert!(
            component.find_module(SymbolName::intern("adapter")).is_none(),
            "folded adapter must not produce HIR"
        );
    }

    #[test]
    fn folds_combined_startup_fixup_after_validating_bypassed_shim() {
        let wat = combined_startup_fixup_wat("$lowered", "$shim-instance");
        let (_context, component) = translate_wat(&wat);

        let main = component
            .find_module(SymbolName::intern("main"))
            .expect("main module should be translated");
        let start = ModuleBuilder::new(main)
            .get_function("actual-start")
            .expect("actual start definition should be translated");
        assert!(
            start
                .borrow()
                .as_operation()
                .get_typed_attribute::<UnitAttr>(WASM_COMPONENT_START_ATTR)
                .is_some(),
            "combined adapter should mark the resolved start definition"
        );
        assert!(
            component.find_module(SymbolName::intern("fixup")).is_none(),
            "folded combined adapter must not produce HIR"
        );
    }

    #[test]
    fn combined_startup_fixup_rejects_a_synthetic_table_argument() {
        let wat = combined_startup_fixup_wat("$lowered", "$synthetic-shim");
        let wasm = wat::parse_str(&wat).expect("component WAT should compile");
        let err = match translate(&wasm, &config(), Rc::new(Context::default())) {
            Ok(_) => panic!("a synthetic table argument must not satisfy the shim relationship"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("table must come from an instantiated shim module"),
            "unexpected diagnostic: {err:?}"
        );
    }

    #[test]
    fn combined_startup_fixup_rejects_an_alias_instead_of_a_canonical_lower() {
        let wat = combined_startup_fixup_wat("$shim-function", "$shim-instance");
        let wasm = wat::parse_str(&wat).expect("component WAT should compile");
        let err = match translate(&wasm, &config(), Rc::new(Context::default())) {
            Ok(_) => panic!("a shim alias must not satisfy the canonical-lower relationship"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("must resolve to direct canonical lowers"),
            "unexpected diagnostic: {err:?}"
        );
    }

    #[test]
    fn exported_initialize_without_adapter_is_not_marked() {
        let (_context, component) = translate_wat(
            r#"
            (component
                (core module $main
                    (func $_initialize (export "_initialize"))
                )
                (core instance $main-instance (instantiate $main))
                (component $export-component)
                (instance $exports (instantiate $export-component))
                (export "miden:test/component@1.0.0" (instance $exports))
            )
            "#,
        );

        let main = component
            .find_module(SymbolName::intern("main"))
            .expect("main module should be translated");
        let initialize = ModuleBuilder::new(main)
            .get_function("_initialize")
            .expect("initialize definition should be translated");
        assert!(
            !initialize.borrow().has_attribute(WASM_COMPONENT_START_ATTR),
            "the export name alone must not drive startup recognition"
        );
    }

    #[test]
    fn rejects_multiple_startup_adapters() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $main
                    (func $actual-start (export "start"))
                )
                (core instance $main-instance (instantiate $main))
                (core module $adapter
                    (import "target" "start" (func $start))
                    (start $start)
                )
                (core instance $first
                    (instantiate $adapter (with "target" (instance $main-instance))))
                (core instance $second
                    (instantiate $adapter (with "target" (instance $main-instance))))
                (component $export-component)
                (instance $exports (instantiate $export-component))
                (export "miden:test/component@1.0.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let err = match translate(&wasm, &config(), Rc::new(Context::default())) {
            Ok(_) => panic!("a second startup adapter must be rejected"),
            Err(err) => err,
        };

        assert!(
            err.to_string().contains("multiple core Wasm startup adapters"),
            "unexpected diagnostic: {err:?}"
        );
    }

    #[test]
    fn startup_lookalike_takes_the_existing_unsupported_instantiation_path() {
        let wasm = wat::parse_str(
            r#"
            (component
                (core module $main
                    (func $actual-start (export "start"))
                )
                (core instance $main-instance (instantiate $main))
                (core module $observable-adapter
                    (import "target" "start" (func $start))
                    (export "observable" (func $start))
                    (start $start)
                )
                (core instance $adapter-instance
                    (instantiate $observable-adapter
                        (with "target" (instance $main-instance))))
                (component $export-component)
                (instance $exports (instantiate $export-component))
                (export "miden:test/component@1.0.0" (instance $exports))
            )
            "#,
        )
        .expect("component WAT should compile");
        let err = match translate(&wasm, &config(), Rc::new(Context::default())) {
            Ok(_) => panic!("an observable adapter must not be folded"),
            Err(err) => err,
        };

        assert!(
            err.to_string()
                .contains("Instantiated module as another module instantiation argument"),
            "the lookalike must continue through ordinary translation: {err:?}"
        );
    }
}
