mod component;
mod function;
mod globals;
mod module;
mod segment;

pub use self::{
    component::{
        Component, ComponentBuilder as PrimComponentBuilder, ComponentExport, ComponentId,
        ComponentInterface, ModuleExport, ModuleInterface,
    },
    function::{Function, FunctionBuilder as PrimFunctionBuilder, FunctionRef},
    globals::*,
    module::{Module, ModuleBuilder as PrimModuleBuilder, ModuleRef},
    segment::*,
};
