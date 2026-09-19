// Straight-line twin of `case_indirect_spill.rs` (campaign 14): eight u64
// values kept in wasm locals (each is an argument of a fn-pointer dispatch
// AND used after it) are live across two `call_indirect` dispatches of
// 7-u64 helpers picked from a runtime-indexed table, with no loop. LLVM
// keeps the arguments stackified here, so the spill analysis never has to
// spill a dispatch argument and its group-0-only operand accounting (see
// the ignored `indirect_spill` test in tests/calls.rs) does no harm: the
// case compiles and passes, bounding the reproducer to the loop shape.
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
    let v0 = x.wrapping_mul(0x94d0_49bb_1331_11eb) ^ y.rotate_left(21);
    let v1 = y.wrapping_mul(0xd6e8_feb8_6659_fd93) ^ x.rotate_left(23);
    let v2 = x.wrapping_mul(0xa076_1d64_78bd_642f) ^ y.rotate_left(25);
    let v3 = y.wrapping_mul(0xe703_7ed1_a0b4_28db) ^ x.rotate_left(27);
    let v4 = x.wrapping_mul(0x8ebc_6af0_9c88_c6e3) ^ y.rotate_left(29);
    let v5 = y.wrapping_mul(0x5895_58cb_3521_e49d) ^ x.rotate_left(31);
    let v6 = x.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ y.rotate_left(33);
    let v7 = y.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ x.rotate_left(35);
    let f = WIDES[(input1 & 1) as usize];
    let r = f(v0, v1, v2, v3, v4, v5, v6);
    let g = WIDES[((r >> 5) & 1) as usize];
    let s = g(v7, v6, v5, v4, v3 ^ r, v2, v1);
    let z = v0
        ^ v1.rotate_left(2)
        ^ v2.rotate_left(4)
        ^ v3.rotate_left(6)
        ^ v4.rotate_left(8)
        ^ v5.rotate_left(10)
        ^ v6.rotate_left(12)
        ^ v7.rotate_left(14)
        ^ r
        ^ s.rotate_left(16);
    (z as u32) ^ ((z >> 32) as u32)
}
