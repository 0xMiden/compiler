#![allow(unused_imports)]
#![allow(unused_variables)]

use std::{collections::VecDeque, sync::Arc};

use miden_core::Felt;
use midenc_debug::{Executor, FromMidenRepr};
use midenc_session::Session;
use proptest::{prop_assert_eq, test_runner::TestCaseError};

mod abi_transform;
mod apps;
mod examples;
mod instructions;
mod intrinsics;
mod local_node;
mod misc;
mod rust_sdk;
mod testnet;
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
    let exec = Executor::for_package(package, args.iter().copied(), session)
        .map_err(|err| TestCaseError::fail(err.to_string()))?;
    let output = exec.execute_into(&package.unwrap_program(), session);
    std::dbg!(&output);
    prop_assert_eq!(rust_out.clone(), output, "VM output mismatch");
    // TODO: Uncomment after https://github.com/0xMiden/compiler/issues/228 is fixed
    // let emul_out: T = (*execute_emulator(ir_program.clone(), args).first().unwrap()).into();
    // prop_assert_eq!(rust_out, emul_out, "Emulator output mismatch");
    Ok(())
}
