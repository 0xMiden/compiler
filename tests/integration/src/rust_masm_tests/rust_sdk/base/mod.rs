use super::*;

pub mod account;
pub mod asset;
pub mod faucet;
pub mod input_note;
pub mod note;
pub mod output_note;
pub mod tx;

/// Asserts that the final MASM contains an exec to the expected protocol linker stub target.
fn assert_masm_execs_protocol_link(test: &mut CompilerTest, module: &str, function: &str) {
    let masm = test.masm_src();
    let link_name = format!("miden::protocol::{module}::{function}");
    let checks = format!(r#"; CHECK: exec.{{{{.*}}}}::"{link_name}""#);
    litcheck_filecheck::filecheck!(&masm, checks);
}
