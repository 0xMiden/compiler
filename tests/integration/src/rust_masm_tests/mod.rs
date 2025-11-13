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
    eval_package::<Felt, _, _>(package, None, args, session, |trace| {
        let vm_out_felt0 = trace.outputs().get_stack_item(0).unwrap();
        let vm_out_felt1 = trace.outputs().get_stack_item(1).unwrap();
        let vm_out: T = T::from_felts(&[vm_out_felt0, vm_out_felt1]);
        dbg!(&vm_out);
        prop_assert_eq!(rust_out.clone(), vm_out, "VM output mismatch");
        Ok(())
    })?;
    Ok(())
}
