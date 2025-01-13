// TODO: remove after its completed
#![allow(unused)]

// TODO: document

use hir2_sketch::{Component, Interface, Module};
use midenc_hir::{
    cranelift_entity::PrimaryMap, diagnostics::Report, AbiParam, CallConv, FunctionIdent, Ident,
    Linkage, Signature, SourceSpan, Symbol,
};
use midenc_session::{DiagnosticsHandler, Session};
use rustc_hash::FxHashMap;
use wasmparser::types::{ComponentEntityType, TypesRef};

use super::{
    translator::convert_lifted_func_ty, CanonLift, CanonLower, ClosedOverComponent,
    ClosedOverModule, ComponentFuncIndex, ComponentIndex, ComponentInstanceIndex,
    ComponentInstantiation, ComponentTypesBuilder, ComponentUpvarIndex, ModuleIndex,
    ModuleInstanceIndex, ModuleUpvarIndex, ParsedComponent, StaticModuleIndex,
    TypeComponentInstanceIndex, TypeDef, TypeModuleIndex,
};
use crate::{
    component::{ComponentItem, LocalInitializer, StaticComponentIndex},
    error::WasmResult,
    module::{
        module_env::ParsedModule,
        types::{EntityIndex, FuncIndex},
    },
    unsupported_diag, WasmTranslationConfig,
};

pub mod hir2_sketch;

/// A translator from the linearized Wasm component model to the Miden IR component
pub struct ComponentTranslator2<'a> {
    /// The translation configuration
    config: &'a WasmTranslationConfig,

    /// The list of static modules that were found during initial translation of
    /// the component.
    ///
    /// This is used during the instantiation of these modules to ahead-of-time
    /// order the arguments precisely according to what the module is defined as
    /// needing which avoids the need to do string lookups or permute arguments
    /// at runtime.
    nested_modules: &'a PrimaryMap<StaticModuleIndex, ParsedModule<'a>>,

    /// The list of static components that were found during initial translation of
    /// the component.
    ///
    /// This is used when instantiating nested components to push a new
    /// `ComponentFrame` with the `ParsedComponent`s here.
    nested_components: &'a PrimaryMap<StaticComponentIndex, ParsedComponent<'a>>,

    result: hir2_sketch::WorldBuilder,

    session: &'a Session,
}

impl<'a> ComponentTranslator2<'a> {
    pub fn new(
        nested_modules: &'a PrimaryMap<StaticModuleIndex, ParsedModule<'a>>,
        nested_components: &'a PrimaryMap<StaticComponentIndex, ParsedComponent<'a>>,
        config: &'a WasmTranslationConfig,
        session: &'a Session,
    ) -> Self {
        let builder = hir2_sketch::WorldBuilder::new("root".to_string());
        Self {
            config,
            session,
            nested_modules,
            nested_components,
            result: builder,
        }
    }

    pub fn translate2(
        mut self,
        root_component: &'a ParsedComponent,
        types: &mut ComponentTypesBuilder,
        _diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<hir2_sketch::World> {
        let mut frame = ComponentFrame::new(root_component.types_ref(), FxHashMap::default());

        for init in &root_component.initializers {
            self.initializer(&mut frame, types, init)?;
        }

        Ok(self.result.build())
    }

    fn initializer(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        init: &'a LocalInitializer<'a>,
    ) -> WasmResult<()> {
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
                                    &self.session.diagnostics,
                                    "Importing of {:?} is not yet supported",
                                    ty
                                )
                            }
                        };
                    }
                };
            }
            LocalInitializer::Lower(lower) => {
                frame.funcs.push(CoreDef::Lower(lower.clone()));
            }
            LocalInitializer::Lift(lift) => {
                frame.component_funcs.push(ComponentFuncDef::Lifted(lift.clone()));
            }
            LocalInitializer::Resource(..) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Resource initializers are not supported"
                )
            }
            LocalInitializer::ResourceNew(..) => {
                unsupported_diag!(&self.session.diagnostics, "Resource creation is not supported")
            }
            LocalInitializer::ResourceRep(..) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Resource representation is not supported"
                )
            }
            LocalInitializer::ResourceDrop(..) => {
                unsupported_diag!(&self.session.diagnostics, "Resource dropping is not supported")
            }
            LocalInitializer::ModuleStatic(static_module_idx) => {
                frame.modules.push(ModuleDef::Static(*static_module_idx));
            }
            LocalInitializer::ModuleInstantiate(module_idx, ref args) => {
                self.module_instantiation(frame, types, module_idx, args)?;
            }
            LocalInitializer::ModuleSynthetic(entities) => {
                frame.module_instances.push(ModuleInstanceDef::Synthetic(entities));
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
                for init in &translation.initializers {
                    self.initializer(&mut new_frame, types, init)?;
                }
                let instance_idx = frame
                    .component_instances
                    .push(ComponentInstanceDef::Instantiated(instance.clone()));
                frame.frames.insert(instance_idx, new_frame);
            }
            LocalInitializer::ComponentSynthetic(_) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Synthetic components are not yet supported"
                )
            }
            LocalInitializer::AliasExportFunc(module_instance_idx, name) => {
                frame.funcs.push(CoreDef::Export(*module_instance_idx, name));
            }
            LocalInitializer::AliasExportTable(..) => {
                unsupported_diag!(&self.session.diagnostics, "Table exports are not yet supported")
            }
            LocalInitializer::AliasExportGlobal(..) => {
                unsupported_diag!(&self.session.diagnostics, "Global exports are not yet supported")
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
                unsupported_diag!(&self.session.diagnostics, "Module aliases are not yet supported")
            }
            LocalInitializer::AliasComponent(_) => {
                unsupported_diag!(
                    &self.session.diagnostics,
                    "Component aliases are not yet supported"
                )
            }
            LocalInitializer::Export(name, component_item) => {
                match component_item {
                    ComponentItem::Func(i) => {
                        frame.component_funcs.push(frame.component_funcs[*i].clone());
                    }
                    ComponentItem::ComponentInstance(_) => {
                        let unwrap_instance = component_item.unwrap_instance();
                        let interface_name = name.to_string();
                        self.component_export(frame, types, unwrap_instance, interface_name)?;
                    }
                    ComponentItem::Type(_) => {
                        // do nothing
                    }
                    _ => unsupported_diag!(
                        &self.session.diagnostics,
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
        interface_name: String,
    ) -> Result<(), Report> {
        let instance = &frame.component_instances[component_instance_idx].unwrap_instantiated();
        let static_component_idx = frame.components[instance.component].index;
        let parsed_component = &self.nested_components[static_component_idx];
        let module = Ident::new(Symbol::intern(interface_name.clone()), SourceSpan::default());
        let functions = parsed_component
            .exports
            .iter()
            .flat_map(|(name, item)| {
                if let ComponentItem::Func(f) = item {
                    self.component_export_func(
                        frame,
                        types,
                        component_instance_idx,
                        module,
                        name,
                        f,
                    )
                } else {
                    // we're only interested in exported functions
                    vec![]
                }
            })
            .collect();
        let interface = Interface {
            name: interface_name,
            functions,
        };
        self.result.root_mut().interfaces.push(interface);
        frame.component_instances.push(ComponentInstanceDef::Export);
        Ok(())
    }

    fn component_export_func(
        &self,
        frame: &ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        component_instance_idx: ComponentInstanceIndex,
        module: Ident,
        name: &str,
        f: &ComponentFuncIndex,
    ) -> Vec<hir2_sketch::SyntheticFunction> {
        let nested_frame = &frame.frames[&component_instance_idx];
        let canon_lift = nested_frame.component_funcs[*f].unwrap_canon_lift();
        let type_func_idx = types.convert_component_func_type(frame.types, canon_lift.ty).unwrap();

        let component_types = types.resources_mut_and_types().1;
        let func_ty = convert_lifted_func_ty(&type_func_idx, component_types);
        let signature = Signature {
            params: func_ty.params.into_iter().map(AbiParam::new).collect(),
            results: func_ty.results.into_iter().map(AbiParam::new).collect(),
            cc: CallConv::CanonLift,
            linkage: Linkage::External,
        };

        let function_id = FunctionIdent {
            module,
            function: Ident::new(Symbol::intern(name.to_string()), SourceSpan::default()),
        };
        let function = hir2_sketch::SyntheticFunction {
            id: function_id,
            signature,
            inner_function: self.core_module_export_func_id(frame, canon_lift),
        };
        vec![function]
    }

    fn core_module_export_func_id(
        &self,
        frame: &ComponentFrame<'a>,
        canon_lift: &CanonLift,
    ) -> FunctionIdent {
        let core_func_id: FunctionIdent = match &frame.funcs[canon_lift.func] {
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
                            FunctionIdent {
                                module: module_ident,
                                function: Ident::new(func_name, SourceSpan::default()),
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
        };
        core_func_id
    }

    fn module_instantiation(
        &mut self,
        frame: &mut ComponentFrame<'a>,
        types: &mut ComponentTypesBuilder,
        module_idx: &ModuleIndex,
        args: &'a FxHashMap<&str, ModuleInstanceIndex>,
    ) -> Result<(), Report> {
        frame.module_instances.push(ModuleInstanceDef::Instantiated {
            module_idx: *module_idx,
            args: args.clone(),
        });

        let mut import_canon_lower_args: FxHashMap<FunctionIdent, Signature> = FxHashMap::default();
        match &frame.modules[*module_idx] {
            ModuleDef::Static(static_module_idx) => {
                let parsed_module = &self.nested_modules[*static_module_idx];
                let mut module = Module {
                    name: parsed_module.module.name(),
                    functions: vec![],
                };
                for module_arg in args {
                    let arg_module_name = module_arg.0;
                    let module_ident =
                        Ident::new(Symbol::intern(*arg_module_name), SourceSpan::default());
                    let arg_module = &frame.module_instances[*module_arg.1];
                    match arg_module {
                        ModuleInstanceDef::Instantiated {
                            module_idx: _,
                            args: _,
                        } => {
                            unsupported_diag!(
                                &self.session.diagnostics,
                                "Instantiated module as another module instantiation argument is \
                                 not supported yet"
                            )
                        }
                        ModuleInstanceDef::Synthetic(entities) => {
                            // module with CanonLower synthetic functions
                            for (func_name, entity) in entities.iter() {
                                let (signature, func_id) =
                                    canon_lower_func(frame, types, module_ident, func_name, entity);
                                import_canon_lower_args.insert(func_id, signature);
                            }
                        }
                    }
                }

                // TODO: the part below happens inside `build_ir` while translating the
                // core module with `import_canon_lower_args` passed as a parameter.
                for import in &parsed_module.module.imports {
                    // find the CanonLower function signature in the instantiation args for
                    // every core module function import
                    let internal_import_func_name = match import.index {
                        EntityIndex::Function(func_idx) => parsed_module.module.func_name(func_idx),
                        _ => panic!(
                            "only function import supported in Wasm core modules yet, got {:?}",
                            import.index
                        ),
                    };
                    let import_func_id = FunctionIdent {
                        module: Ident::new(Symbol::intern(&import.module), SourceSpan::default()),
                        function: Ident::new(Symbol::intern(&import.field), SourceSpan::default()),
                    };
                    // TODO: handle error
                    let import_canon_lower_func_sig =
                        &import_canon_lower_args.remove(&import_func_id).unwrap();

                    let internal_func_id = FunctionIdent {
                        module: module.name,
                        function: Ident::new(internal_import_func_name, SourceSpan::default()),
                    };
                    let function = hir2_sketch::SyntheticFunction {
                        id: internal_func_id,
                        signature: import_canon_lower_func_sig.clone(),
                        inner_function: import_func_id,
                    };
                    module.functions.push(function);
                }

                self.result.root_mut().modules.push(module);
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
        let interface_name = name.0.to_string();
        let module = Ident::new(Symbol::intern(interface_name.clone()), SourceSpan::default());
        let inner_function_empty = FunctionIdent {
            module: Ident::new(Symbol::intern(""), SourceSpan::default()),
            function: Ident::new(Symbol::intern(""), SourceSpan::default()),
        };
        let component_types = types.resources_mut_and_types().1;
        let instance_type = &component_types[ty];
        let functions = instance_type
            .exports
            .iter()
            .filter_map(|(name, ty)| {
                import_component_export_func(
                    module,
                    inner_function_empty,
                    component_types,
                    name,
                    ty,
                )
            })
            .collect();
        let interface = Interface {
            name: interface_name.clone(),
            functions,
        };
        let import_component = Component {
            name: interface_name,
            interfaces: vec![interface],
            modules: Default::default(),
        };
        self.result.add_import(import_component);
        Ok(())
    }
}

fn import_component_export_func(
    module: Ident,
    inner_function_empty: FunctionIdent,
    component_types: &super::ComponentTypes,
    name: &String,
    ty: &TypeDef,
) -> Option<hir2_sketch::SyntheticFunction> {
    if let TypeDef::ComponentFunc(func_ty) = ty {
        let func_ty = convert_lifted_func_ty(func_ty, component_types);
        let signature = Signature {
            params: func_ty.params.into_iter().map(AbiParam::new).collect(),
            results: func_ty.results.into_iter().map(AbiParam::new).collect(),
            cc: CallConv::CanonLift,
            linkage: Linkage::External,
        };
        Some(hir2_sketch::SyntheticFunction {
            id: FunctionIdent {
                module,
                function: Ident::new(Symbol::intern(name), SourceSpan::default()),
            },
            signature,
            inner_function: inner_function_empty,
        })
    } else {
        None
    }
}

fn canon_lower_func(
    frame: &mut ComponentFrame,
    types: &mut ComponentTypesBuilder,
    module_ident: Ident,
    func_name: &str,
    entity: &EntityIndex,
) -> (Signature, FunctionIdent) {
    let func_id = entity.unwrap_func();
    let canon_lower = frame.funcs[func_id].unwrap_canon_lower();
    let func_name_ident = Ident::new(Symbol::intern(func_name), SourceSpan::default());
    // TODO: handle error
    let type_func_idx =
        types.convert_component_func_type(frame.types, canon_lower.lower_ty).unwrap();

    let component_types = types.resources_mut_and_types().1;
    let func_ty = convert_lifted_func_ty(&type_func_idx, component_types);
    let signature = Signature {
        params: func_ty.params.into_iter().map(AbiParam::new).collect(),
        results: func_ty.results.into_iter().map(AbiParam::new).collect(),
        cc: CallConv::CanonLower,
        linkage: Linkage::External,
    };

    let func_id = FunctionIdent {
        module: module_ident,
        function: func_name_ident,
    };
    (signature, func_id)
}

#[derive(Clone, Debug)]
enum ComponentInstanceDef<'a> {
    Import(ComponentInstanceImport),
    Instantiated(ComponentInstantiation<'a>),
    Export,
}
impl<'a> ComponentInstanceDef<'a> {
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
    Import(ComponentInstanceIndex, &'a str),

    /// A core wasm function was lifted into a component function.
    Lifted(CanonLift),
}
impl<'a> ComponentFuncDef<'a> {
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

impl<'a> CoreDef<'a> {
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
            TypeDef::ComponentFunc(_ty) => {
                ComponentItemDef::Func(ComponentFuncDef::Import(component_instance_idx, name))
            }
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
                // TODO: handle error
                ComponentItemDef::Type(types.convert_type(self.types, t).unwrap())
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
