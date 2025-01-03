#![allow(unused)]

use hir2_sketch::{Component, Interface, Module};
use midenc_hir::{
    cranelift_entity::PrimaryMap, diagnostics::Report, CanonAbiImport, ComponentBuilder,
    ComponentExport, FunctionIdent, FunctionType, Ident, InterfaceFunctionIdent, InterfaceIdent,
    MidenAbiImport, Signature, SourceSpan, Symbol,
};
use midenc_hir_type::Abi;
use midenc_session::{DiagnosticsHandler, Session};
use rustc_hash::FxHashMap;
use wasmparser::types::ComponentEntityType;

use super::{
    interface_type_to_ir, CanonLift, CanonLower, CanonicalOptions, ComponentFuncIndex,
    ComponentIndex, ComponentInstanceIndex, ComponentInstantiation, ComponentTypes,
    ComponentTypesBuilder, CoreDef, CoreExport, Export, ExportItem, GlobalInitializer, ImportIndex,
    InstantiateModule, LinearComponent, LinearComponentTranslation, LoweredIndex,
    ModuleInstanceIndex, ParsedRootComponent, RuntimeImportIndex, RuntimeInstanceIndex,
    RuntimePostReturnIndex, RuntimeReallocIndex, StaticModuleIndex, Trampoline, TypeDef,
    TypeFuncIndex,
};
use crate::{
    component::{ComponentItem, LocalInitializer, StaticComponentIndex, StringEncoding},
    error::WasmResult,
    intrinsics::{
        intrinsics_conversion_result, is_miden_intrinsics_module, IntrinsicsConversionResult,
    },
    miden_abi::{is_miden_abi_module, miden_abi_function_type, recover_imported_masm_function_id},
    module::{
        build_ir::build_ir_module,
        instance::ModuleArgument,
        module_translation_state::ModuleTranslationState,
        types::{EntityIndex, FuncIndex},
    },
    unsupported_diag, WasmTranslationConfig,
};

pub mod hir2_sketch;

/// A translator from the linearized Wasm component model to the Miden IR component
pub struct ComponentTranslator2<'a> {
    /// The translation configuration
    config: &'a WasmTranslationConfig,

    // TODO: extract into a separate struct ComponentTranslationState
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

impl<'a> ComponentTranslator2<'a> {
    pub fn new(config: &'a WasmTranslationConfig, session: &'a Session) -> Self {
        Self {
            config,
            session,
            module_instances_source: PrimaryMap::new(),
            lower_imports: FxHashMap::default(),
            reallocs: FxHashMap::default(),
            post_returns: FxHashMap::default(),
        }
    }

    /// Translate the given parsed Wasm component to the Miden IR component
    pub fn translate(
        mut self,
        parsed_root_component: ParsedRootComponent,
        types: &mut ComponentTypesBuilder,
        diagnostics: &DiagnosticsHandler,
    ) -> WasmResult<Component> {
        // dbg!(&parset_root_component.static_components.len());
        let mut component = hir2_sketch::Component {
            name: "root".to_string(),
            interfaces: vec![],
            modules: vec![],
        };
        let mut static_modules = Vec::new();
        let mut components: PrimaryMap<ComponentIndex, (_, _)> = PrimaryMap::new();
        let mut component_instances: PrimaryMap<ComponentInstanceIndex, ComponentInstance> =
            PrimaryMap::new();
        let mut component_funcs: PrimaryMap<ComponentFuncIndex, (ComponentInstanceIndex, String)> =
            PrimaryMap::new();
        let mut core_funcs: PrimaryMap<FuncIndex, (ModuleInstanceIndex, String)> =
            PrimaryMap::new();
        let mut lowerings: PrimaryMap<FuncIndex, CanonLower> = PrimaryMap::new();
        let mut liftings: PrimaryMap<ComponentFuncIndex, CanonLift> = PrimaryMap::new();
        let types_ref = parsed_root_component.root_component.types_ref();
        for init in parsed_root_component.root_component.initializers.iter() {
            // dbg!(&init);

            match init {
                LocalInitializer::Import(name, ty) => {
                    // dbg!(name, ty);
                    assert!(
                        matches!(ty, ComponentEntityType::Instance(_)),
                        "only component instances imports supported yet"
                    );
                    let ty =
                        types.convert_component_entity_type(types_ref, *ty).map_err(Report::msg)?;
                    // self.import_types.push((name.0.to_string(), ty));
                    component_instances.push(ComponentInstance::Import(ComponentInstanceImport {
                        name: name.0.to_string(),
                        ty,
                    }));
                }
                LocalInitializer::Lower(
                    lower @ CanonLower {
                        func,
                        lower_ty,
                        canonical_abi,
                        ref options,
                    },
                ) => {
                    // dbg!(&init);
                    lowerings.push(lower.clone());
                }
                LocalInitializer::Lift(
                    lift @ CanonLift {
                        ty: component_func_type_id,
                        func: func_index,
                        options: ref local_canonical_options,
                    },
                ) => {
                    // dbg!(&init);
                    liftings.push(lift.clone());
                }
                LocalInitializer::Resource(aliasable_resource_id, wasm_type, func_index) => todo!(),
                LocalInitializer::ResourceNew(aliasable_resource_id, signature_index) => todo!(),
                LocalInitializer::ResourceRep(aliasable_resource_id, signature_index) => todo!(),
                LocalInitializer::ResourceDrop(aliasable_resource_id, signature_index) => todo!(),
                LocalInitializer::ModuleStatic(static_module_index) => {
                    let parsed_module = &parsed_root_component.static_modules[*static_module_index];
                    let module = Module {
                        name: parsed_module.module.name(),
                        functions: vec![],
                    };
                    static_modules.push(module);
                }
                LocalInitializer::ModuleInstantiate(module_idx, ref args) => {
                    // TODO: assert that module imports are satisfied by the args (every import has
                    // an argument of the correct type)

                    // we don't support multiple instances of the same module, so it's safe to
                    // remove the module
                    component.modules.push(static_modules.remove(module_idx.as_u32() as usize));
                }
                LocalInitializer::ModuleSynthetic(hash_map) => {
                    // dbg!(&hash_map);
                    let mut module_name: Option<String> = None;
                    let functions_ids: Vec<(Ident, Signature)> = hash_map
                        .iter()
                        .map(|(k, v)| {
                            let func_id = v.unwrap_func();
                            let canon_lower = &lowerings[func_id];
                            let comp_func = &component_funcs[canon_lower.func];
                            let import_instance = &component_instances[comp_func.0].unwrap_import();
                            if let Some(module_name) = &module_name {
                                assert_eq!(
                                    module_name, &import_instance.name,
                                    "unexpected functions from different import instances in one \
                                     synthetic core module"
                                );
                            } else {
                                module_name = Some(import_instance.name.clone());
                            }
                            let func = Ident::new(Symbol::intern(*k), SourceSpan::default());
                            // TODO: get the component function type
                            let signature = Signature::new(vec![], vec![]);
                            (func, signature)
                        })
                        .collect();
                    let module_id =
                        Ident::new(Symbol::intern(module_name.unwrap()), SourceSpan::default());
                    let functions = functions_ids
                        .into_iter()
                        .map(|(function, signature)| {
                            let id = FunctionIdent {
                                module: module_id,
                                function,
                            };
                            // TODO: generate lowering
                            hir2_sketch::Function { id, signature }
                        })
                        .collect();
                    let module = Module {
                        name: module_id,
                        functions,
                    };
                    component.modules.push(module);
                }
                LocalInitializer::ComponentStatic(idx, ref closed_over_vars) => {
                    // dbg!(&init);
                    let comp = &parsed_root_component.static_components[*idx];
                    // dbg!(&comp.initializers);
                    // dbg!(&comp.exports);
                    components.push((idx, closed_over_vars));
                }
                LocalInitializer::ComponentInstantiate(
                    instance @ ComponentInstantiation {
                        component: component_index,
                        ref args,
                        ty: component_instance_type_id,
                    },
                ) => {
                    // dbg!(&init);
                    component_instances.push(ComponentInstance::Instantiated(instance.clone()));
                }
                LocalInitializer::ComponentSynthetic(hash_map) => {
                    dbg!(&init);
                }
                LocalInitializer::AliasExportFunc(module_instance_index, name) => {
                    // dbg!(&init);
                    core_funcs.push((*module_instance_index, name.to_string()));
                    // let module = component.modules[module_instance_index]
                }
                LocalInitializer::AliasExportTable(module_instance_index, _) => todo!(),
                LocalInitializer::AliasExportGlobal(module_instance_index, _) => todo!(),
                LocalInitializer::AliasExportMemory(module_instance_index, _) => {
                    // dbg!(&init);
                }
                LocalInitializer::AliasComponentExport(component_instance_index, name) => {
                    component_funcs.push((*component_instance_index, name.to_string()));
                }
                LocalInitializer::AliasModule(closed_over_module) => todo!(),
                LocalInitializer::AliasComponent(closed_over_component) => todo!(),
                LocalInitializer::Export(name, component_item) => {
                    // dbg!(&init);
                    assert!(
                        matches!(component_item, ComponentItem::ComponentInstance(_)),
                        "only component instances exports supported yet"
                    );
                    let interface_name = name.to_string();
                    let instance = &component_instances[component_item.unwrap_instance()]
                        .unwrap_instantiated();
                    let static_component_idx = components[instance.component].0;
                    let parsed_component =
                        &parsed_root_component.static_components[*static_component_idx];
                    dbg!(&parsed_component.exports);
                    let module =
                        Ident::new(Symbol::intern(interface_name.clone()), SourceSpan::default());
                    let functions = parsed_component
                        .exports
                        .iter()
                        .flat_map(|(name, item)| {
                            if let ComponentItem::Func(f) = item {
                                // let (component_instance_id, name) = component_funcs[*f];
                                // let component_instance = component_instances[component_instance_id]
                                //     .unwrap_instantiated();
                                // TODO: get the component function type
                                let signature = Signature::new(vec![], vec![]);

                                let function_id = FunctionIdent {
                                    module,
                                    function: Ident::new(
                                        Symbol::intern(name.to_string()),
                                        SourceSpan::default(),
                                    ),
                                };
                                let function = hir2_sketch::Function {
                                    id: function_id,
                                    signature,
                                };
                                vec![function]
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
                    component.interfaces.push(interface);
                    component_instances.push(ComponentInstance::Export);
                    // TODO: generate synth module with liftings
                }
            }
        }

        Ok(component)
    }
}

enum ComponentInstance<'a> {
    Import(ComponentInstanceImport),
    Instantiated(ComponentInstantiation<'a>),
    Export,
}
impl<'a> ComponentInstance<'a> {
    fn unwrap_import(&self) -> ComponentInstanceImport {
        match self {
            ComponentInstance::Import(import) => import.clone(),
            _ => panic!("expected import"),
        }
    }

    fn unwrap_instantiated(&self) -> ComponentInstantiation {
        match self {
            ComponentInstance::Instantiated(instantiated) => instantiated.clone(),
            _ => panic!("expected instantiated"),
        }
    }
}

#[derive(Debug, Clone)]
struct ComponentInstanceImport {
    name: String,
    ty: TypeDef,
}
