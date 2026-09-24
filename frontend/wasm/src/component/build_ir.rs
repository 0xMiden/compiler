use std::rc::Rc;

use midenc_hir::{Context, SymbolPath, dialects::builtin::BuiltinDialect};
use midenc_session::{Session, diagnostics::Report};

use super::{
    ComponentItem, ComponentTypesBuilder, ParsedRootComponent,
    naming::{ExportPaths, exports_namespace, external_id_path, interface_hint},
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

/// Decides the namespace the component is rooted at, and returns it with the Miden paths of the
/// function exports of the nested components.
///
/// The Miden paths of the component's function exports must all be `<namespace>::<name>` for one
/// namespace, which must equal the target namespace when the build provides one. A component
/// without function exports takes the target namespace.
fn component_namespace<'data>(
    parsed: &ParsedRootComponent<'data>,
    config: &WasmTranslationConfig,
) -> WasmResult<(SymbolPath, ExportPaths<'data>)> {
    let root_instance_exports: Vec<&str> = parsed
        .root_component
        .exports
        .iter()
        .filter_map(|(name, item)| {
            matches!(item, ComponentItem::ComponentInstance(_)).then_some(*name)
        })
        .collect();
    let mut exports = Vec::new();
    for (index, component) in parsed.static_components.iter() {
        let interface = interface_hint(&root_instance_exports, index.as_u32());
        for (name, item) in component.exports.iter() {
            if matches!(item, ComponentItem::Func(_)) {
                let external_id = component.export_external_ids.get(name).copied();
                exports.push(((index, *name), external_id_path(&interface, name, external_id)?));
            }
        }
    }
    let namespace = match (
        exports_namespace(exports.iter().map(|((_, name), path)| (*name, path)))?,
        &config.namespace,
    ) {
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
    Ok((namespace, exports.into_iter().collect()))
}
