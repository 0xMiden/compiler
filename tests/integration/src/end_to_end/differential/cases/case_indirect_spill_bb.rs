// COMPILE-TIME COMPILER PANIC REPRODUCER (campaign 31, 2026-09-17): the
// `case_indirect_spill.rs` shape with the fn-pointer table read through
// `core::hint::black_box(&WIDES)`, which is what keeps the dispatch
// INDIRECT on the nightly-2026-09-01 guest toolchain — LLVM devirtualizes a
// plain read of a constant fn-pointer table into a switch of direct calls,
// so the original case no longer emits a single `call_indirect` and no
// longer reaches `hir.exec_indirect` at all. Everything else is identical:
// seven u64 values live across a 7-u64 dispatch inside a loop with a
// loop-carried table index, so the spill analysis (which reads operand
// group 0 only and therefore never sees `hir.exec_indirect`'s arguments in
// group 1) spills arguments, never reloads them, and budgets the call as
// one felt while the emitter still holds them. See the ignored test in
// tests/calls.rs.
type Wide = fn(u64, u64, u64, u64, u64, u64, u64) -> u64;

#[inline(never)]
fn w_fold(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    a.wrapping_add(b)
        .wrapping_mul(c | 1)
        .wrapping_sub(d)
        .rotate_left((e & 63) as u32)
        ^ f.wrapping_add(g)
}

#[inline(never)]
fn w_zip(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    (a ^ b.rotate_right(17))
        .wrapping_add(c.wrapping_mul(c))
        .wrapping_add(d >> 3)
        .wrapping_add(e << 5)
        .wrapping_add(f ^ g.swap_bytes())
}

static WIDES: [Wide; 2] = [w_fold, w_zip];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let v0 = x.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ y.rotate_left(21);
    let v1 = y.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ x.rotate_left(23);
    let v2 = x.wrapping_mul(0x94d0_49bb_1331_11eb) ^ y.rotate_left(25);
    let v3 = y.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ x.rotate_left(27);
    let v4 = x.wrapping_mul(0xa076_1d64_78bd_642f) ^ y.rotate_left(29);
    let v5 = y.wrapping_mul(0xe703_7ed1_a0b4_28db) ^ x.rotate_left(31);
    let v6 = x.wrapping_mul(0x8ebc_6af0_9c88_c6e3) ^ y.rotate_left(33);
    let mut idx = (input1 & 1) as usize;
    let mut acc = x;
    let n = (input2 % 7).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        let f = core::hint::black_box(&WIDES)[idx];
        let r = f(v0, v1, v2, v3 ^ acc, v4, v5.wrapping_add(i as u64), v6);
        acc = acc.rotate_left(19) ^ r ^ v0 ^ v1 ^ v2;
        idx = (r & 1) as usize;
        i = i.wrapping_add(1);
    }
    let z = v0.rotate_left(2)
        ^ v1.rotate_left(4)
        ^ v2.rotate_left(6)
        ^ v3.rotate_left(8)
        ^ v4.rotate_left(10)
        ^ v5.rotate_left(12)
        ^ v6.rotate_left(14)
        ^ acc;
    (z as u32) ^ ((z >> 32) as u32) ^ (idx as u32)
}
