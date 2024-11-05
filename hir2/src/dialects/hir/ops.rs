mod assertions;
mod binary;
mod cast;
mod component;
mod constants;
mod control;
mod function;
mod globals;
mod invoke;
mod mem;
mod module;
mod primop;
mod ternary;
mod unary;

pub use self::{
    assertions::*,
    binary::*,
    cast::*,
    component::{ComponentBuilder as PrimComponentBuilder, *},
    constants::*,
    control::*,
    function::{FunctionBuilder as PrimFunctionBuilder, *},
    globals::*,
    invoke::*,
    mem::*,
    module::{ModuleBuilder as PrimModuleBuilder, *},
    primop::*,
    ternary::*,
    unary::*,
};
