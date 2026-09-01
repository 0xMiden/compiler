//! Stored-procedure dispatch stubs.
//!
//! A guest that calls a procedure whose MAST root it reads from storage calls one of these
//! stubs. The frontend recognizes the `intrinsics::exec_root` module prefix, reads the call
//! signature from the stub itself and replaces the body with a dispatch through the root.
//!
//! The stubs are keyed by shape, not by call site: `a<N>_r<f|v>` takes the four root field
//! elements, then `N` argument field elements, and returns one field element (`rf`) or nothing
//! (`rv`). The `StoredProcedure` call traits flatten every argument into field elements, so `N`
//! runs from 0 to 12 — the operand stack holds 16 field elements and the root takes four.
//!
//! The exported names must stay in lockstep with the `MODULE_PREFIX` in the compiler frontend
//! (`frontend/wasm/src/intrinsics/exec_root.rs`) and with the `link_name` of the declarations in
//! `sdk/base/src/types/stored_procedure.rs`.
//!
//! The stubs live here, and not in the crate that calls them, on purpose: `build.rs` compiles
//! this directory into a separate archive, so the optimizer of the calling crate sees a
//! declaration only. A stub defined next to its caller has a body the optimizer reads as
//! unreachable, and the call is then removed.

/// Defines the two dispatch stubs of one argument count.
///
/// Each entry supplies the exported name and the Rust name of the field-element-returning stub,
/// the same pair for the stub that returns nothing, and the argument parameters.
macro_rules! exec_root_stubs {
    ($($felt_symbol:literal, $felt_name:ident, $unit_symbol:literal, $unit_name:ident,
       ($($arg:ident),*);)*) => {
        $(
            #[unsafe(export_name = $felt_symbol)]
            #[optimize(none)]
            #[inline(never)]
            pub extern "C" fn $felt_name(
                _root_f0: f32,
                _root_f1: f32,
                _root_f2: f32,
                _root_f3: f32,
                $($arg: f32),*
            ) -> f32 {
                unsafe { core::hint::unreachable_unchecked() }
            }

            #[unsafe(export_name = $unit_symbol)]
            #[optimize(none)]
            #[inline(never)]
            pub extern "C" fn $unit_name(
                _root_f0: f32,
                _root_f1: f32,
                _root_f2: f32,
                _root_f3: f32,
                $($arg: f32),*
            ) {
                unsafe { core::hint::unreachable_unchecked() }
            }
        )*
    };
}

exec_root_stubs! {
    "intrinsics::exec_root::a0_rf", exec_root_a0_rf_plain,
    "intrinsics::exec_root::a0_rv", exec_root_a0_rv_plain, ();
    "intrinsics::exec_root::a1_rf", exec_root_a1_rf_plain,
    "intrinsics::exec_root::a1_rv", exec_root_a1_rv_plain, (_a0);
    "intrinsics::exec_root::a2_rf", exec_root_a2_rf_plain,
    "intrinsics::exec_root::a2_rv", exec_root_a2_rv_plain, (_a0, _a1);
    "intrinsics::exec_root::a3_rf", exec_root_a3_rf_plain,
    "intrinsics::exec_root::a3_rv", exec_root_a3_rv_plain, (_a0, _a1, _a2);
    "intrinsics::exec_root::a4_rf", exec_root_a4_rf_plain,
    "intrinsics::exec_root::a4_rv", exec_root_a4_rv_plain, (_a0, _a1, _a2, _a3);
    "intrinsics::exec_root::a5_rf", exec_root_a5_rf_plain,
    "intrinsics::exec_root::a5_rv", exec_root_a5_rv_plain, (_a0, _a1, _a2, _a3, _a4);
    "intrinsics::exec_root::a6_rf", exec_root_a6_rf_plain,
    "intrinsics::exec_root::a6_rv", exec_root_a6_rv_plain, (_a0, _a1, _a2, _a3, _a4, _a5);
    "intrinsics::exec_root::a7_rf", exec_root_a7_rf_plain,
    "intrinsics::exec_root::a7_rv", exec_root_a7_rv_plain, (_a0, _a1, _a2, _a3, _a4, _a5, _a6);
    "intrinsics::exec_root::a8_rf", exec_root_a8_rf_plain,
    "intrinsics::exec_root::a8_rv", exec_root_a8_rv_plain,
    (_a0, _a1, _a2, _a3, _a4, _a5, _a6, _a7);
    "intrinsics::exec_root::a9_rf", exec_root_a9_rf_plain,
    "intrinsics::exec_root::a9_rv", exec_root_a9_rv_plain,
    (_a0, _a1, _a2, _a3, _a4, _a5, _a6, _a7, _a8);
    "intrinsics::exec_root::a10_rf", exec_root_a10_rf_plain,
    "intrinsics::exec_root::a10_rv", exec_root_a10_rv_plain,
    (_a0, _a1, _a2, _a3, _a4, _a5, _a6, _a7, _a8, _a9);
    "intrinsics::exec_root::a11_rf", exec_root_a11_rf_plain,
    "intrinsics::exec_root::a11_rv", exec_root_a11_rv_plain,
    (_a0, _a1, _a2, _a3, _a4, _a5, _a6, _a7, _a8, _a9, _a10);
    "intrinsics::exec_root::a12_rf", exec_root_a12_rf_plain,
    "intrinsics::exec_root::a12_rv", exec_root_a12_rv_plain,
    (_a0, _a1, _a2, _a3, _a4, _a5, _a6, _a7, _a8, _a9, _a10, _a11);
}
