mod component;
mod function;
mod global_variable;
mod interface;
mod module;
mod segment;

pub use self::{
    component::{
        Component, ComponentBuilder as PrimComponentBuilder, ComponentExport, ComponentId,
        ComponentInterface, ModuleExport, ModuleInterface,
    },
    function::{Function, FunctionBuilder as PrimFunctionBuilder, FunctionRef, LocalId},
    global_variable::*,
    interface::{Interface, InterfaceBuilder as PrimInterfaceBuilder, InterfaceRef},
    module::{Module, ModuleBuilder as PrimModuleBuilder, ModuleRef},
    segment::*,
};
