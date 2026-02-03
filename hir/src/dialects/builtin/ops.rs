mod cast;
mod component;
mod function;
mod global_variable;
mod interface;
mod module;
mod segment;
mod world;

pub use self::{
    cast::UnrealizedConversionCast,
    component::{
        Component, ComponentBuilder as PrimComponentBuilder, ComponentExport, ComponentId,
        ComponentInterface, ComponentRef, ModuleExport, ModuleInterface,
    },
    function::{
        Function, FunctionBuilder as PrimFunctionBuilder, FunctionRef, LocalVariable, Ret, RetImm,
    },
    global_variable::*,
    interface::{Interface, InterfaceBuilder as PrimInterfaceBuilder, InterfaceRef},
    module::{Module, ModuleBuilder as PrimModuleBuilder, ModuleRef},
    segment::*,
    world::{World, WorldRef},
};
