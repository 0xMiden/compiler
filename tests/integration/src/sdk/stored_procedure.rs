//! Integration tests for the stored-procedure feature, pinned at the Miden Assembly it produces.
//!
//! A stored-procedure call is a chain of layers that only the assembled output proves whole: the
//! `call` method of a `StoredProcedure` handle selects a shape stub by the flat width of its
//! arguments, the frontend replaces the stub body with `hir.exec_root`, and codegen emits the
//! zero-root guard, the spill of the root word to a linker-allocated scratch cell, and the
//! `dynexec` through that cell.
//!
//! The calls are read from `examples/stored-procedure-example`.

use midenc_frontend_wasm::WasmTranslationConfig;

use crate::CompilerTest;

/// Path of the example this module compiles, relative to the test crate.
const EXAMPLE_PATH: &str = "../../examples/stored-procedure-example";

/// Component id of the example, as it appears in the lifted export paths.
const COMPONENT_ID: &str = "miden:stored-procedure-example/stored-procedure-example@0.1.0";

/// Compiles the example and returns its Miden Assembly.
fn compile_example() -> String {
    let mut test =
        CompilerTest::rust_source_cargo_miden(EXAMPLE_PATH, WasmTranslationConfig::default(), []);
    test.masm_src()
}

/// Returns the body of the top-level procedure named `name`, without its declaration.
///
/// A declaration line names the procedure in quotes, and a line holding `end` alone in the first
/// column closes it; every nested block is indented.
#[track_caller]
fn procedure_body<'a>(masm: &'a str, name: &str) -> &'a str {
    let declaration = format!("proc \"{name}\"");
    let public_declaration = format!("pub {declaration}");
    let mut body_start: Option<usize> = None;
    let mut offset = 0usize;
    for line in masm.lines() {
        let next_line = offset + line.len() + 1;
        match body_start {
            None if line.starts_with(&declaration) || line.starts_with(&public_declaration) => {
                body_start = Some(next_line);
            }
            Some(start) if line == "end" => return &masm[start..offset],
            _ => {}
        }
        offset = next_line;
    }
    match body_start {
        Some(_) => panic!("the procedure '{name}' is not closed by an `end` line"),
        None => panic!("MASM does not declare a procedure named '{name}'"),
    }
}

/// Asserts that `body` holds every needle, in the order given.
#[track_caller]
fn assert_ordered(body: &str, needles: &[&str], description: &str) {
    let mut cursor = 0usize;
    for needle in needles {
        let found = body[cursor..].find(needle).unwrap_or_else(|| {
            panic!("{description} misses '{needle}' after offset {cursor}:\n{body}")
        });
        cursor += found + needle.len();
    }
}

/// Asserts the dispatch sequence of a stored-procedure call.
///
/// The sequence lives in the shape stub the call selects, not in the calling procedure: calls that
/// flatten to the same shape share one stub. `dispatch` calls through a
/// `StoredProcedure<fn() -> Felt>` slot, which takes no argument and returns one field element, so
/// its stub is `a0_rf`. `dispatch_weighted` calls through a
/// `StoredProcedure<fn(Word, Felt) -> Felt>` slot, whose arguments flatten to five field elements,
/// so its stub is `a5_rf`. Each `call` also proves that the argument buffer and the width folded
/// away: a stub call survives in the assembled output only if the shape is a compile-time
/// constant.
#[test]
fn stored_procedure_call_guards_the_root_and_dispatches_through_the_scratch_cell() {
    let masm = compile_example();

    let dispatch = procedure_body(&masm, &format!("{COMPONENT_ID}#dispatch"));
    assert!(
        dispatch.contains("::\"intrinsics::exec_root::a0_rf\""),
        "`dispatch` must call the no-argument, felt-returning dispatch stub:\n{dispatch}"
    );

    let dispatch_weighted = procedure_body(&masm, &format!("{COMPONENT_ID}#dispatch-weighted"));
    assert!(
        dispatch_weighted.contains("::\"intrinsics::exec_root::a5_rf\""),
        "`dispatch-weighted` must call the five-argument, felt-returning dispatch \
         stub:\n{dispatch_weighted}"
    );

    let stub = procedure_body(&masm, "intrinsics::exec_root::a0_rf");
    let scratch_address = stub
        .split("mem_storew_le.")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("the dispatch stub does not spill the root word:\n{stub}"))
        .to_string();

    assert_ordered(
        stub,
        &[
            // Zero-root guard: an uninitialized storage slot reads as the zero word
            "assertz.err=\"stored procedure call: procedure root is zero (storage slot not \
             initialized)\"",
            // Spill the root word to the compiler-owned scratch cell
            &format!("mem_storew_le.{scratch_address}"),
            // `dynexec` pops the cell's address and reads the callee digest there
            &format!("push.{scratch_address}"),
            "dynexec",
        ],
        "the dispatch stub",
    );
}
