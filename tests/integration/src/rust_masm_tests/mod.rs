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
