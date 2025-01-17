use std::rc::Rc;

use midenc_hir::diagnostics::Report;
use midenc_hir2::{dialects::builtin::BuiltinDialect, Context};
use midenc_session::Session;

use super::{
    inline, translator::ComponentTranslator, translator2::ComponentTranslator2,
    ComponentTypesBuilder, LinearComponentTranslation, ParsedRootComponent,
};
use crate::{
    component::ComponentParser, error::WasmResult, supported_component_model_features,
    WasmTranslationConfig,
};

/// Translate a Wasm component binary into Miden IR component
pub fn translate_component(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    session: &Session,
) -> WasmResult<midenc_hir::Component> {
    let (mut component_types_builder, parsed_component) = parse(config, wasm, session)?;
    let linearized_component_translation = inline(&mut component_types_builder, &parsed_component)?;
    let component_types = component_types_builder.finish();
    let parsed_modules = parsed_component.static_modules;
    let translator = ComponentTranslator::new(component_types, parsed_modules, config, session);
    translator.translate(linearized_component_translation, session.diagnostics.as_ref())
}

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

fn inline(
    component_types_builder: &mut ComponentTypesBuilder,
    parsed_component: &ParsedRootComponent<'_>,
) -> WasmResult<LinearComponentTranslation> {
    // ... after translation initially finishes the next pass is performed
    // which we're calling "inlining". This will "instantiate" the root
    // component, following nested component instantiations, creating a
    // global list of initializers along the way. This phase uses the simple
    // initializers in each component to track dataflow of host imports and
    // internal references to items throughout a component at translation time.
    // The produce initializers in the final `LinearComponent` are intended to be
    // much simpler than the original component and more efficient for
    // us to process (e.g. no string lookups as
    // most everything is done through indices instead).
    let component_dfg = inline::run(
        component_types_builder,
        &parsed_component.root_component,
        &parsed_component.static_modules,
        &parsed_component.static_components,
    )
    .map_err(Report::msg)?;
    Ok(component_dfg.finish())
}

/// Translate a Wasm component binary into Miden IR component
pub fn translate_component2(
    wasm: &[u8],
    config: &WasmTranslationConfig,
    session: Rc<Session>,
) -> WasmResult<midenc_hir2::dialects::builtin::ComponentRef> {
    let (mut component_types_builder, parsed_root_component) = parse(config, wasm, &session)?;
    let context = Rc::new(Context::new(session));
    let dialect = context.get_or_register_dialect::<BuiltinDialect>();
    dialect.expect_registered_name::<midenc_hir2::dialects::builtin::Component>();
    // context.get_or_register_dialect::<HirDialect>();
    let translator = ComponentTranslator2::new(
        &parsed_root_component.static_modules,
        &parsed_root_component.static_components,
        config,
        context,
    );
    translator.translate2(&parsed_root_component.root_component, &mut component_types_builder)
}
