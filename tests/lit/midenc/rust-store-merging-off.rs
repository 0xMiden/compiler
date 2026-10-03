//! RUN: midenc %s --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
//!
//! Verify that a standalone `.rs` file is compiled with LLVM's store merging off: copying a pair
//! of felts, which Rust carries in `f32`, stays two 32-bit loads and stores rather than becoming
//! one 64-bit load and store.
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn copy_pair(dst: &mut [f32; 2], src: &[f32; 2]) {
    dst[0] = src[0];
    dst[1] = src[1];
}

// CHECK-LABEL: builtin.function public extern("C") @copy_pair
// CHECK-NOT: ptr<i64
// CHECK: hir.store {{.*}} : (ptr<felt, element>, felt);
// CHECK-NOT: ptr<i64
// CHECK: hir.store {{.*}} : (ptr<felt, element>, felt);
// CHECK-NOT: ptr<i64
// CHECK: builtin.ret
