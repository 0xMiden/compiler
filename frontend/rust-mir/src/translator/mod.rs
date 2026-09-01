//! Translates the MIR of the analyzed crate into Miden IR.
//!
//! Every module below reads only `rustc_public` types. The internal rustc types stay out of the
//! translator.

mod body;
mod operand;
mod rvalue;
mod statement;
mod terminator;
mod types;

use std::rc::Rc;

use midenc_hir::{
    BuilderExt, Context, Ident, Report, SourceSpan,
    dialects::builtin::{ComponentBuilder, ComponentRef, ModuleBuilder, World, WorldBuilder},
    version::Version,
};
use rustc_public::{ItemKind, mir::mono::Instance};

/// The namespace of the component that holds the translated functions.
const COMPONENT_NAMESPACE: &str = "root_ns";

/// The name of the component that holds the translated functions.
const COMPONENT_NAME: &str = "root";

/// The version of the component that holds the translated functions.
const COMPONENT_VERSION: &str = "1.0.0";

/// Translates every local, non-generic function of the analyzed crate into one Miden IR
/// component.
///
/// The caller must call this function inside a `rustc_public` driver callback, because the MIR
/// is only reachable there.
pub(crate) fn translate_crate(
    module_name: &str,
    context: Rc<Context>,
) -> Result<ComponentRef, Report> {
    let world = context.clone().builder().create::<World, ()>(SourceSpan::default())()?;
    let mut world_builder = WorldBuilder::new(world);
    let component = world_builder.define_component(
        Ident::from(COMPONENT_NAMESPACE),
        Ident::from(COMPONENT_NAME),
        Version::parse(COMPONENT_VERSION).map_err(|err| {
            Report::msg(format!("rust mir frontend: invalid component version: {err}"))
        })?,
    )?;

    let mut component_builder = ComponentBuilder::new(component);
    let module = component_builder.define_module(Ident::from(module_name))?;
    let mut module_builder = ModuleBuilder::new(module);

    for item in rustc_public::all_local_items() {
        if item.kind() != ItemKind::Fn {
            continue;
        }
        // A generic item has no single body, thus it is skipped. Monomorphization is a later
        // milestone.
        let Ok(instance) = Instance::try_from(item) else {
            continue;
        };
        let Some(mir_body) = instance.body() else {
            continue;
        };
        body::translate_function(&function_name(&instance.name()), &mir_body, &mut module_builder)?;
    }

    Ok(component)
}

/// Returns the last segment of a Rust item path.
///
/// Miden IR names functions inside a module, thus the crate and module path of the Rust item is
/// not part of the name.
fn function_name(path: &str) -> String {
    match path.rsplit_once("::") {
        Some((_, name)) => name.to_string(),
        None => path.to_string(),
    }
}
