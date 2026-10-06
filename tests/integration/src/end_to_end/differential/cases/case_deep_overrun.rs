// Nine activations of a [MaybeUninit<u64>; 16384] frame (131072 bytes each,
// 1179648 bytes in total) recursing through a function-pointer table (the
// assembler's linker rejects a direct call-graph cycle), each writing and
// reading back its first, last and one runtime-indexed element around the
// recursive call, so every frame must really span its 131072 bytes and no two
// activations may share one. The guest shadow stack is 1 MiB. The table is
// read through `black_box` so LLVM (nightly-2026-09-01 and later) cannot
// devirtualize the dispatch into direct calls that close the cycle.
use core::{hint::black_box, mem::MaybeUninit};

type Step = fn(u32, u64) -> u64;

#[inline(never)]
fn rec_a(n: u32, s: u64) -> u64 {
    let mut frame: [MaybeUninit<u64>; 16384] = [MaybeUninit::uninit(); 16384];
    let mid = (s % 16384) as usize;
    frame[0].write(s ^ 0x1111_1111_1111_1111);
    frame[16383].write(s.rotate_left(17));
    frame[mid].write(s.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (n as u64));
    let child = if n == 0 {
        s.wrapping_mul(0x2545_f491_4f6c_dd1d)
    } else {
        let f = black_box(&STEPS)[((s >> 5) % 2) as usize];
        f(n - 1, s.wrapping_add(0x9e37_79b9) ^ (n as u64).rotate_left(7))
    };
    let a = unsafe { frame[0].assume_init() };
    let b = unsafe { frame[16383].assume_init() };
    let c = unsafe { frame[mid].assume_init() };
    child.rotate_left(3) ^ a ^ b.rotate_left(9) ^ c.rotate_left(21)
}

#[inline(never)]
fn rec_b(n: u32, s: u64) -> u64 {
    let mut frame: [MaybeUninit<u64>; 16384] = [MaybeUninit::uninit(); 16384];
    let mid = (s % 16384) as usize;
    frame[0].write(s ^ 0x2222_2222_2222_2222);
    frame[16383].write(s.rotate_left(29));
    frame[mid].write(s.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ (n as u64));
    let child = if n == 0 {
        !s
    } else {
        let f = black_box(&STEPS)[((s >> 11) % 2) as usize];
        f(n - 1, s.wrapping_sub(0x5851_f42d) ^ (n as u64).rotate_left(13))
    };
    let a = unsafe { frame[0].assume_init() };
    let b = unsafe { frame[16383].assume_init() };
    let c = unsafe { frame[mid].assume_init() };
    child.rotate_left(5) ^ a ^ b.rotate_left(11) ^ c.rotate_left(23)
}

static STEPS: [Step; 2] = [rec_a, rec_b];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = ((input1 as u64) << 32) | input2 as u64;
    let f = black_box(&STEPS)[(input2 % 2) as usize];
    let r = f(8, s | 1);
    (r as u32) ^ ((r >> 32) as u32)
}
