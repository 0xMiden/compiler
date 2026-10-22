//! RUN: env RUSTFLAGS="-Copt-level=0 -Cdebuginfo=2" midenc %s --entrypoint=lexical_shadowing::entrypoint --emit=hir=- -Canalyze-only > %t.hir
//! RUN: filecheck %s < %t.hir
//! RUN: env RUSTFLAGS="-Copt-level=0 -Cdebuginfo=2" midenc %s --entrypoint=lexical_shadowing::entrypoint --debug full -o %t.masp
//!
//! CHECK-LABEL: builtin.function{{.*}}@entrypoint
//! CHECK: di.debug_declare {{.*}}name = "shadowed"{{.*}}line = [[OUTER_LINE:[0-9]+]]{{.*}}DI_OP_frame_base(local_slot, [[BASE:[0-9]+]]+[[OFFSET:[0-9]+]])
//! CHECK: di.debug_declare {{.*}}name = "shadowed"{{.*}}DI_OP_frame_base
//! CHECK: di.debug_declare {{.*}}name = "shadowed"{{.*}}line = [[OUTER_LINE]]{{.*}}DI_OP_frame_base(local_slot, [[BASE]]+[[OFFSET]])
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[inline(never)]
fn observe(value: u32) -> u32 {
    core::hint::black_box(value)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input: u32) -> u32 {
    let shadowed = core::hint::black_box(input.wrapping_add(10));
    let before = observe(shadowed);
    let inside = {
        let shadowed = core::hint::black_box(input.wrapping_add(20));
        observe(shadowed)
    };
    let after = observe(shadowed);
    before.wrapping_add(inside).wrapping_add(after)
}
