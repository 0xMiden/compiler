use std::rc::Rc;

use midenc_hir2::{
    dialects::builtin::{self, BuiltinDialect},
    Context,
};
use midenc_session::{diagnostics::Report, Session};

use super::{translator::ComponentTranslator, ComponentTypesBuilder, ParsedRootComponent};
use crate::{
    component::ComponentParser, error::WasmResult, supported_component_model_features,
    WasmTranslationConfig,
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
) -> WasmResult<midenc_hir2::dialects::builtin::ComponentRef> {
    let (mut component_types_builder, parsed_root_component) =
        parse(config, wasm, context.session())?;
    let dialect = context.get_or_register_dialect::<BuiltinDialect>();
    dialect.expect_registered_name::<midenc_hir2::dialects::builtin::Component>();
    // context.get_or_register_dialect::<HirDialect>();
    // Extract component name from exported component instance
    let id = {
        let instance = parsed_root_component
            .root_component
            .exports
            .iter()
            .find_map(|(name, c)| match c {
                super::ComponentItem::ComponentInstance(_) => Some(*name),
                _ => None,
            })
            .expect("expected at least one component instance to be exported");
        instance.parse::<builtin::ComponentId>().expect("invalid component name")
    };
    let translator = ComponentTranslator::new(
        id,
        &parsed_root_component.static_modules,
        &parsed_root_component.static_components,
        config,
        context,
    );
    translator.translate2(&parsed_root_component.root_component, &mut component_types_builder)
}
