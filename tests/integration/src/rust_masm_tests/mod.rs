#![allow(unused_imports)]
#![allow(unused_variables)]

use std::{collections::VecDeque, sync::Arc};

use miden_core::Felt;
use miden_debug::{Executor, FromMidenRepr, ToMidenRepr};
use midenc_session::Session;
use proptest::{prop_assert_eq, test_runner::TestCaseError};

use crate::testing::eval_package;

mod abi_transform;
mod apps;
mod debug_source_locations;
mod examples;
mod instructions;
mod intrinsics;
mod misc;
mod rust_sdk;
mod types;

/// Push a value onto an argument vector in the order expected by `miden_debug::Executor`.
///
/// `Executor` expects stack inputs with the top at index 0. [ToMidenRepr::to_felts] returns field
/// elements in little-endian order (least significant first), so we can append those elements
/// directly to ensure the least-significant element is on top.
pub trait PushToStackInputs: ToMidenRepr {
    /// Push `self` onto `stack` in top-to-bottom order.
    fn push_to_stack_inputs(&self, stack: &mut Vec<Felt>) {
        for felt in self.to_felts() {
            stack.push(Felt::new(felt.as_canonical_u64()));
        }
    }
}

impl<T: ToMidenRepr + ?Sized> PushToStackInputs for T {}

pub fn run_masm_vs_rust<T>(
    rust_out: T,
    package: &miden_mast_package::Package,
    args: &[Felt],
    session: &Session,
) -> Result<(), TestCaseError>
where
    T: Clone + FromMidenRepr + PartialEq + std::fmt::Debug,
{
    let vm_out = eval_package::<T, _, _>(package, None, args, session, |_| Ok(()))?;
    prop_assert_eq!(rust_out, vm_out, "VM output mismatch");
    Ok(())
}
