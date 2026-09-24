use std::rc::Rc;

use midenc_hir::{Context, dialects::builtin::BuiltinDialect, interner::Symbol};
use midenc_session::{Session, diagnostics::Report};

use super::{ComponentTypesBuilder, ParsedRootComponent, translator::ComponentTranslator};
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
    // The component is named by its exported component instance name, as one symbol
    let name = parsed_root_component
        .root_component
        .exports
        .iter()
        .find_map(|(name, c)| match c {
            super::ComponentItem::ComponentInstance(_) => Some(Symbol::intern(*name)),
            _ => None,
        })
        .expect("expected at least one component instance to be exported");
    let translator = ComponentTranslator::new(
        name,
        &mut parsed_root_component.static_modules,
        &parsed_root_component.static_components,
        config,
        context,
    )?;
    translator.translate2(&parsed_root_component.root_component, &mut component_types_builder)
}
