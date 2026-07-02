//! Execution coverage for the runtime failure modes of indirect calls: an out-of-bounds table
//! index must trap with the bounds-check message, and dispatching through a null slot must fail
//! on its zero MAST root. Neither can be a differential case, since both are undefined behavior
//! when executed natively.

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use super::differential::harness::{CASE_HEADER, cargo_toml, miden_project_toml};
use crate::{CompilerTest, project, testing::executor_with_std};

/// Calls through the funcref table with a runtime-chosen index: `input1` is transmuted into a
/// function pointer, and at the Wasm level a function pointer *is* its table index, so the
/// dispatched slot is entirely input-controlled. The `OPS` dispatch keeps real table entries
/// alive (rustc reserves slot 0 as the null function pointer, so they land in slots 1 and 2).
const SOURCE: &str = r#"
#[inline(never)]
fn op_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[inline(never)]
fn op_mul(a: u32, b: u32) -> u32 {
    a.wrapping_mul(b)
}

static OPS: [fn(u32, u32) -> u32; 2] = [op_add, op_mul];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let base = OPS[(input2 & 1) as usize](1, 2);
    let f: fn(u32, u32) -> u32 = unsafe { core::mem::transmute(input1 as usize) };
    f(input2, base)
}
"#;

#[test]
fn indirect_call_runtime_traps() {
    let pkg_name = "indirect_call_traps";
    let manifest = cargo_toml(pkg_name);
    let miden_project_manifest = miden_project_toml(pkg_name);
    let full_source = format!("{CASE_HEADER}{SOURCE}");

    let proj = project(&format!("{pkg_name}_masm"))
        .file("miden-project.toml", &miden_project_manifest)
        .file("Cargo.toml", &manifest)
        .file("src/lib.rs", &full_source)
        .build();
    let mut test =
        CompilerTest::rust_source_cargo_miden(proj.root(), WasmTranslationConfig::default(), []);
    let package = test.compile_package();
    let source_manager = test.session.source_manager.clone();

    let run = |index: u32, input2: u32| -> Result<u32, String> {
        let package = package.clone();
        let source_manager = source_manager.clone();
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let exec = executor_with_std(
                vec![Felt::new_unchecked(index as u64), Felt::new_unchecked(input2 as u64)],
                Some(&package),
            );
            exec.execute_into::<u32>(&package.unwrap_program(), source_manager)
        }))
        .map_err(|panic| {
            panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "opaque panic".to_string())
        })
    };

    // In-bounds dispatch through a live slot succeeds; slot numbering between the two ops is a
    // toolchain detail, so accept either callee: with input2 = 5, base = op_mul(1, 2) = 2, and
    // f(5, 2) is 7 (op_add) or 10 (op_mul)
    let result = run(1, 5).expect("in-bounds dispatch should succeed");
    assert!(matches!(result, 7 | 10), "unexpected dispatch result: {result}");

    // An out-of-bounds index trips the emitted bounds check deterministically
    let err = run(1000, 5).expect_err("out-of-bounds dispatch should trap");
    assert!(
        err.contains("indirect call: function table index out of bounds"),
        "unexpected out-of-bounds failure: {err}"
    );

    // Slot 0 is the null function pointer: its slot holds the zero word, which resolves to no
    // procedure in the MAST forest
    let err = run(0, 5).expect_err("null-slot dispatch should fail");
    assert!(err.contains("could not be found"), "unexpected null-slot failure: {err}");
}
