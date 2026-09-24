use std::rc::Rc;

use midenc_hir::{
    Context, SymbolNameComponent, SymbolPath, dialects::builtin::BuiltinDialect, interner::Symbol,
};
use midenc_session::{Session, diagnostics::Report};

use super::{
    ComponentItem, ComponentTypesBuilder, ParsedRootComponent,
    naming::{ExportedFunction, exports_namespace, interface_hint},
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
    let namespace = component_namespace(&parsed_root_component, config)?;
    let translator = ComponentTranslator::new(
        namespace.to_symbol_name(),
        &mut parsed_root_component.static_modules,
        &parsed_root_component.static_components,
        config,
        context,
    )?;
    translator.translate2(&parsed_root_component.root_component, &mut component_types_builder)
}

/// Decides the namespace the component is rooted at.
///
/// The Miden paths of the component's function exports must all be `<namespace>::<name>` for one
/// namespace, which must equal the target namespace when the build provides one. A component
/// without function exports takes the target namespace, or else its exported instance name.
fn component_namespace(
    parsed: &ParsedRootComponent<'_>,
    config: &WasmTranslationConfig,
) -> WasmResult<SymbolPath> {
    let root_instance_exports: Vec<&str> = parsed
        .root_component
        .exports
        .iter()
        .filter_map(|(name, item)| {
            matches!(item, ComponentItem::ComponentInstance(_)).then_some(*name)
        })
        .collect();
    let exports = parsed.static_components.iter().flat_map(|(index, component)| {
        let interface = interface_hint(&root_instance_exports, index.as_u32());
        component
            .exports
            .iter()
            .filter(|(_, item)| matches!(item, ComponentItem::Func(_)))
            .map(move |(name, _)| ExportedFunction {
                interface: interface.clone(),
                name,
                external_id: component.export_external_ids.get(name).copied(),
            })
    });
    match (exports_namespace(exports)?, &config.namespace) {
        (Some(declared), Some(expected)) if declared != *expected => Err(Report::msg(format!(
            "manifest namespace `{expected}` does not match the component's exports, which are \
             under `{declared}`"
        ))),
        (Some(declared), _) => Ok(declared),
        (None, Some(expected)) => Ok(expected.clone()),
        (None, None) => {
            let name = root_instance_exports
                .first()
                .expect("expected at least one component instance to be exported");
            Ok(SymbolPath::from_iter([
                SymbolNameComponent::Root,
                SymbolNameComponent::Component(Symbol::intern(*name)),
            ]))
        }
    }
}
