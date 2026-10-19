//! Immutable contracts shared by selection, allocation, expansion, and verification.
//!
//! Selection describes semantic values and lowering requirements here; allocation adds physical
//! actions and entry contracts, and frame assignment resolves symbolic storage. Verification
//! checks these claims against source evidence and the expanded instructions before assembly.
//! The contracts describe a proposed implementation, rather than granting it authority.
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
pub(crate) mod tests;
