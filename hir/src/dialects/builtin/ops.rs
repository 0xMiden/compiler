mod cast;
mod component;
mod function;
mod function_table;
mod global_variable;
mod interface;
mod module;
mod segment;
mod world;

pub(crate) use self::world::shadowing_error;
// The op builders of components, modules and interfaces are not exported: they would create ops
// with names that skip the spelling and the checks of the builtin builders
pub use self::{
    cast::UnrealizedConversionCast,
    component::{
        Component, ComponentExport, ComponentInterface, ComponentRef, ModuleExport, ModuleInterface,
    },
    function::{Function, FunctionBuilder as PrimFunctionBuilder, FunctionRef, Ret, RetImm},
    function_table::*,
    global_variable::*,
    interface::{Interface, InterfaceRef},
    module::{Module, ModuleRef},
    segment::*,
    world::{World, WorldRef},
};

/// Parses the raw segment naming a module or an interface, a symbol table of `kind`, and adds its
/// Miden Assembly spelling (see [crate::SymbolPath::segments_of]) to `state` as the `name`
/// attribute.
fn parse_segment_name(
    state: &mut crate::OperationState,
    parser: &mut dyn crate::OpAsmParser<'_>,
    kind: crate::SymbolTableKind,
) -> crate::ParseResult {
    let name = parser.parse_symbol_name()?;
    let spelled = crate::SymbolPath::table_name(kind, name.name)
        .map_err(|err| crate::parse::ParserError::invalid_symbol_table_name(name.span, err))?;
    let name = crate::Ident::new(spelled, name.span);
    state.add_attribute(
        "name",
        parser.context_rc().create_attribute::<super::attributes::IdentAttr, _>(name),
    );
    Ok(())
}
