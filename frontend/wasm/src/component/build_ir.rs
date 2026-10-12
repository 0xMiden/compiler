use std::{rc::Rc, sync::Arc};

use cranelift_entity::PrimaryMap;
use midenc_hir::{Context, SymbolPath, dialects::builtin::BuiltinDialect};
use midenc_package_interface::PackageInterface;
use midenc_session::{Session, diagnostics::Report};
use wasmparser::component_types::ComponentEntityType;

use super::{
    ComponentIndex, ComponentInstanceIndex, ComponentItem, ComponentTypesBuilder, LocalInitializer,
    ParsedComponent, ParsedRootComponent, StaticComponentIndex,
    naming::{ExportPaths, exports_namespace, external_id_path, world_level_function_export},
    translator::ComponentTranslator,
};
use crate::{
    FrontendOutput, WasmTranslationConfig, component::ComponentParser, error::WasmResult,
    supported_component_model_features,
};

fn parse<'data>(
    config: &WasmTranslationConfig,
    wasm: &'data [u8],
    session: &Session,
) -> Result<(ComponentTypesBuilder, ParsedRootComponent<'data>), Report> {
    let mut validator =
        wasmparser::Validator::new_with_features(supported_component_model_features());
    let mut component_types_builder = Default::default();
    let component_parser =
        ComponentParser::new(config, session, &mut validator, &mut component_types_builder);
    let parsed_component = component_parser.parse(wasm)?;
    Ok((component_types_builder, parsed_component))
}

/// Translate a Wasm component binary into Miden IR component
pub fn translate_component(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    context: Rc<Context>,
) -> WasmResult<FrontendOutput> {
    let (mut component_types_builder, mut parsed_root_component) =
        parse(config, wasm, context.session())?;
    let dialect = context.get_or_register_dialect::<BuiltinDialect>();
    dialect.expect_registered_name::<midenc_hir::dialects::builtin::Component>();
    let (namespace, export_paths) = component_namespace(&parsed_root_component, config)?;
    let translator = ComponentTranslator::new(
        namespace.to_symbol_name(),
        export_paths,
        &mut parsed_root_component.static_modules,
        &parsed_root_component.static_components,
        config,
        context,
    )?;
    translator.translate2(&parsed_root_component.root_component, &mut component_types_builder)
}

/// Returns the namespace declared by the function exports of the Wasm component `wasm`, i.e. the
/// namespace the frontend roots the component at when the build does not provide one.
///
/// `linked_packages` are the package interfaces the translation is given (see
/// [`WasmTranslationConfig::linked_packages`]), so the scan classifies linker stubs exactly as
/// the translation does.
///
/// Returns `Ok(None)` for a core module or a component without function exports.
pub fn declared_namespace(
    wasm: &[u8],
    linked_packages: Option<Arc<[PackageInterface]>>,
    session: &Session,
) -> WasmResult<Option<SymbolPath>> {
    if !wasmparser::Parser::is_component(wasm) {
        return Ok(None);
    }
    let config = WasmTranslationConfig {
        parse_wasm_debuginfo: false,
        linked_packages,
        ..Default::default()
    };
    let (_, parsed) = parse(&config, wasm, session)?;
    Ok(declared_exports(&parsed)?.0)
}

/// Decides the namespace the component is rooted at, and returns it with the Miden paths of the
/// function exports of the nested components whose instances the root component exports.
///
/// The Miden paths of the component's function exports must all be `<namespace>::<name>` for one
/// namespace, which must equal the target namespace when the build provides one. A component
/// without function exports takes the target namespace.
fn component_namespace<'data>(
    parsed: &ParsedRootComponent<'data>,
    config: &WasmTranslationConfig,
) -> WasmResult<(SymbolPath, ExportPaths<'data>)> {
    let (declared, export_paths) = declared_exports(parsed)?;
    let namespace = match (declared, &config.namespace) {
        (Some(declared), Some(expected)) if declared != *expected => {
            return Err(Report::msg(format!(
                "the target namespace `{expected}` (from the project manifest or `--name`) does \
                 not match the component's exports, which are under `{declared}`"
            )));
        }
        (Some(declared), _) => declared,
        (None, Some(expected)) => expected.clone(),
        (None, None) => {
            return Err(Report::msg(format!(
                "component `{}` exports no functions and no namespace was given; declare \
                 `[lib].namespace` or export a function with `@external-id`",
                config.source_name
            )));
        }
    };
    Ok((namespace, export_paths))
}

/// Returns the namespace shared by the function exports of the nested components whose instances
/// the root component of `parsed` exports, or `None` when there are none, with the Miden paths of
/// those exports.
///
/// Fails when the root component exports a function itself, or as [`exports_namespace`] and
/// [`external_id_path`] do.
fn declared_exports<'data>(
    parsed: &ParsedRootComponent<'data>,
) -> WasmResult<(Option<SymbolPath>, ExportPaths<'data>)> {
    if let Some((name, _)) = parsed
        .root_component
        .exports
        .iter()
        .find(|(_, item)| matches!(item, ComponentItem::Func(_)))
    {
        return Err(world_level_function_export(name));
    }
    let mut exports = Vec::new();
    for (interface, index) in exported_components(&parsed.root_component) {
        let component = &parsed.static_components[index];
        for (name, item) in component.exports.iter() {
            if matches!(item, ComponentItem::Func(_)) {
                let external_id = component.export_external_ids.get(name).copied();
                exports.push(((index, *name), external_id_path(interface, name, external_id)?));
            }
        }
    }
    let declared = exports_namespace(exports.iter().map(|((_, name), path)| (*name, path)))?;
    Ok((declared, exports.into_iter().collect()))
}

/// Returns the nested components whose instances the root component `root` exports, each paired
/// with the interface name of its first export, in export order.
///
/// An exported instance that does not instantiate a nested component (an imported or aliased
/// instance) contributes nothing, and a nested component reached twice is returned once.
fn exported_components<'data>(
    root: &ParsedComponent<'data>,
) -> Vec<(&'data str, StaticComponentIndex)> {
    // The root's component and component-instance index spaces, numbered as the translator's
    // `ComponentFrame` numbers them. `None` marks a component that is not a nested component
    // definition, or an instance that is not an instantiation of one.
    let mut components: PrimaryMap<ComponentIndex, Option<StaticComponentIndex>> =
        PrimaryMap::new();
    let mut instances: PrimaryMap<ComponentInstanceIndex, Option<StaticComponentIndex>> =
        PrimaryMap::new();
    // Imports and instance-export aliases add to the index space of their entity type.
    let push_opaque =
        |ty: ComponentEntityType,
         components: &mut PrimaryMap<ComponentIndex, _>,
         instances: &mut PrimaryMap<ComponentInstanceIndex, _>| match ty {
            ComponentEntityType::Component(_) => {
                components.push(None);
            }
            ComponentEntityType::Instance(_) => {
                instances.push(None);
            }
            _ => {}
        };
    let types = root.types_ref();
    let mut exported: Vec<(&'data str, StaticComponentIndex)> = Vec::new();
    for init in &root.initializers {
        match init {
            LocalInitializer::ComponentStatic(index, _) => {
                components.push(Some(*index));
            }
            LocalInitializer::AliasComponent(_) => {
                components.push(None);
            }
            LocalInitializer::ComponentInstantiate(instance) => {
                instances.push(components[instance.component]);
            }
            LocalInitializer::ComponentSynthetic(_) => {
                instances.push(None);
            }
            LocalInitializer::Import(_, ty) => push_opaque(*ty, &mut components, &mut instances),
            LocalInitializer::AliasComponentExport(instance, name) => {
                let instance_ty = types.component_instance_at(instance.as_u32());
                push_opaque(types[instance_ty].exports[*name].ty, &mut components, &mut instances);
            }
            LocalInitializer::Export(name, ComponentItem::ComponentInstance(instance)) => {
                if let Some(index) = instances[*instance]
                    && !exported.iter().any(|(_, seen)| *seen == index)
                {
                    exported.push((*name, index));
                }
                // An exported instance takes a new index of its own.
                instances.push(None);
            }
            LocalInitializer::Export(_, ComponentItem::Component(_)) => {
                components.push(None);
            }
            _ => {}
        }
    }
    exported
}
