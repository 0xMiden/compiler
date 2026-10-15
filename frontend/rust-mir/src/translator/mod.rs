//! Translates the extracted MIR of the analyzed crate into Miden IR.
//!
//! The translator reads only the plain data of [crate::extracted], thus it runs on the caller
//! thread, after rustc has stopped.

mod body;

use std::rc::Rc;

use midenc_hir::{
    BuilderExt, Context, Ident, Report, SourceSpan,
    dialects::builtin::{ComponentBuilder, ComponentRef, ModuleBuilder, World, WorldBuilder},
};

use crate::extracted::ExtractedCrate;

/// The name of the component that holds the translated functions.
const COMPONENT_NAME: &str = "root";

/// Translates every extracted function into one Miden IR component, inside one module with the
/// name `module_name`.
pub(crate) fn translate_crate(
    module_name: &str,
    extracted: &ExtractedCrate,
    context: Rc<Context>,
) -> Result<ComponentRef, Report> {
    let world = context.clone().builder().create::<World, ()>(SourceSpan::default())()?;
    let mut world_builder = WorldBuilder::new(world);
    let component = world_builder.define_component(Ident::from(COMPONENT_NAME))?;

    let mut component_builder = ComponentBuilder::new(component);
    let module = component_builder.define_module(Ident::from(module_name))?;
    let mut module_builder = ModuleBuilder::new(module);

    for function in &extracted.functions {
        body::translate_function(function, &mut module_builder)?;
    }

    Ok(component)
}
