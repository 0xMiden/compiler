//! Commands for the `cargo-component` CLI.

mod add;
mod bindings;
mod new;
mod publish;
mod update;

pub use self::{add::*, bindings::*, new::*, publish::*, update::*};
