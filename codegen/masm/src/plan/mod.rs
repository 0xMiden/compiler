//! Immutable contracts shared by selection, allocation, expansion, and verification.
//!
//! These types contain no allocator state. The production emitter remains unchanged until cutover.
mod abi;
pub(crate) mod actions;
mod diagnostics;
mod effects;
mod frame;
mod ids;
mod layout;
mod selected;

pub(crate) use self::{
    abi::*, diagnostics::*, effects::*, frame::*, ids::*, layout::*, selected::*,
};

#[cfg(test)]
mod tests;
