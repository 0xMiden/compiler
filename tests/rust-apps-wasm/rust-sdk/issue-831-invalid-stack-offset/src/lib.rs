#![no_std]
#![no_main]
use miden_stdlib_sys::{Felt, Word};

#[panic_handler]
fn my_panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint() -> Felt {
    let w0 = Word::new([
        Felt::from_u32(1),
        Felt::from_u32(2),
        Felt::from_u32(3),
        Felt::from_u32(4),
    ]);
    let w1 = Word::new([
        Felt::from_u32(5),
        Felt::from_u32(6),
        Felt::from_u32(7),
        Felt::from_u32(8),
    ]);
    let w2 = Word::new([
        Felt::from_u32(9),
        Felt::from_u32(10),
        Felt::from_u32(11),
        Felt::from_u32(12),
    ]);

    let f0 = Felt::from_u32(13);
    let f1 = Felt::from_u32(14);
    let f2 = Felt::from_u32(15);
    let f3 = Felt::from_u32(16);
    let f4 = Felt::from_u32(17);

    let r14 = args_14(w0, w1, w2, f0, f1);
    let r15 = args_15(w0, w1, w2, f0, f1, f2);
    let r16 = args_16(w0, w1, w2, f0, f1, f2, f3);
    let r17 = args_17(w0, w1, w2, f0, f1, f2, f3, f4);

    r14 + r15 + r16 + r17
}

#[inline(never)]
fn args_14(serial_num: Word, offered_asset: Word, requested_asset: Word, creator: Felt, aux: Felt) -> Felt {
    // Force a word store/load to exercise operand scheduling around `hir.store`/`hir.load` with
    // many live function arguments on the stack.
    let mut scratch = ::core::mem::MaybeUninit::<Word>::uninit();
    let stored = unsafe {
        ::core::ptr::write_volatile(scratch.as_mut_ptr(), offered_asset);
        let loaded = ::core::ptr::read_volatile(scratch.as_ptr());
        ::core::ptr::drop_in_place(scratch.as_mut_ptr());
        loaded
    };

    let keep_live = serial_num[0] + requested_asset[0] + creator;
    aux + keep_live + stored[0]
}

#[inline(never)]
fn args_15(
    serial_num: Word,
    offered_asset: Word,
    requested_asset: Word,
    creator_prefix: Felt,
    creator_suffix: Felt,
    aux: Felt,
) -> Felt {
    let mut scratch = ::core::mem::MaybeUninit::<Word>::uninit();
    let stored = unsafe {
        ::core::ptr::write_volatile(scratch.as_mut_ptr(), offered_asset);
        let loaded = ::core::ptr::read_volatile(scratch.as_ptr());
        ::core::ptr::drop_in_place(scratch.as_mut_ptr());
        loaded
    };

    let keep_live = serial_num[0] + requested_asset[0] + creator_prefix + creator_suffix;
    aux + keep_live + stored[0]
}

#[inline(never)]
fn args_16(
    serial_num: Word,
    offered_asset: Word,
    requested_asset: Word,
    creator_prefix: Felt,
    creator_suffix: Felt,
    aux: Felt,
    extra: Felt,
) -> Felt {
    let mut scratch = ::core::mem::MaybeUninit::<Word>::uninit();
    let stored = unsafe {
        ::core::ptr::write_volatile(scratch.as_mut_ptr(), offered_asset);
        let loaded = ::core::ptr::read_volatile(scratch.as_ptr());
        ::core::ptr::drop_in_place(scratch.as_mut_ptr());
        loaded
    };

    let keep_live = serial_num[0] + requested_asset[0] + creator_prefix + creator_suffix + extra;
    aux + keep_live + stored[0]
}

#[inline(never)]
fn args_17(
    serial_num: Word,
    offered_asset: Word,
    requested_asset: Word,
    creator_prefix: Felt,
    creator_suffix: Felt,
    aux: Felt,
    extra0: Felt,
    extra1: Felt,
) -> Felt {
    let mut scratch = ::core::mem::MaybeUninit::<Word>::uninit();
    let stored = unsafe {
        ::core::ptr::write_volatile(scratch.as_mut_ptr(), offered_asset);
        let loaded = ::core::ptr::read_volatile(scratch.as_ptr());
        ::core::ptr::drop_in_place(scratch.as_mut_ptr());
        loaded
    };

    let keep_live = serial_num[0]
        + requested_asset[0]
        + creator_prefix
        + creator_suffix
        + extra0
        + extra1;
    aux + keep_live + stored[0]
}
