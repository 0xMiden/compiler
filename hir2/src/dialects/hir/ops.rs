mod assertions;
mod binary;
mod cast;
mod constants;
mod control;
mod function;
mod invoke;
mod mem;
mod module;
mod primop;
mod ternary;
mod unary;

pub use self::{
    assertions::*, binary::*, cast::*, constants::*, control::*, function::*, invoke::*, mem::*,
    module::*, primop::*, ternary::*, unary::*,
};
