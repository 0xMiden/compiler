//! RUN: env RUSTFLAGS="-Copt-level=0 -Cdebuginfo=2" midenc %s --entrypoint=aggregate_location::entrypoint --emit=hir=- -Canalyze-only 2>&1 | filecheck %s
//!
//! CHECK: name = "q"
//! CHECK-SAME: expression = #di.expression<[DI_OP_local_slot(0), DW_OP_deref]>
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[repr(C)]
pub struct Quad {
    pub first: u64,
    pub second: u64,
    pub third: u64,
    pub fourth: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(q: Quad) -> u64 {
    q.first.wrapping_add(q.second).wrapping_add(q.third).wrapping_add(q.fourth)
}
