//! RUN: env MIDENC_TRACE=module-parser=debug midenc %s --release -Canalyze-only --emit=masm=- 2>&1 | filecheck %s
//! RUN: env MIDENC_TRACE=module-parser=debug midenc %s --release -Canalyze-only -Zprint-hir-source-locations --emit=hir=- 2>&1 | filecheck %s --check-prefix=HIR
//!
//! This test verifies that source location information from DWARF is correctly
//! resolved when trim-paths is enabled or `--remap-path-prefix` is used. Both
//! are enabled by default, depending on whether Cargo or rustc is used to build
//! the input.
//!
//! CHECK: resolved source path 'trim-paths.rs' -> '/{{.+}}/trim-paths.rs'
//! CHECK-NOT: failed to resolve source path 'trim-paths
//! CHECK-LABEL: pub proc test_assertion

// Verify HIR output contains source locations with absolute paths
//
// HIR: hir.bitcast {{.*}} loc(trim-paths.rs:{{.*}});
// HIR: arith.gt {{.*}} loc(trim-paths.rs:{{.*}});
// HIR: builtin.ret {{.*}} loc(trim-paths.rs:{{.*}});

// Verify that unreachable instructions following panic calls inherit source locations
// This tests the fix where unreachable instructions without DWARF debug info
// inherit the span from the previous valid instruction (the panic call).
//
// HIR: hir.exec {{.*}}panic_fmt(
// HIR-NEXT: ub.unreachable loc(trim-paths.rs:36:{{\d+}});

#![no_std]
#![no_main]

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn test_assertion(x: u32) -> u32 {
    assert!(x > 100, "x should be greater than 100");

    x
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn entrypoint(x: u32) -> u32 {
    test_assertion(x)
}
