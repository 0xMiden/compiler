// COMPILE-TIME COMPILER PANIC REPRODUCER (campaign 14, 2026-09-03), third
// manifestation of the `indirect_spill` class: a loop-free 7-u64
// fn-pointer dispatch whose fourth and sixth arguments are computed IN
// PLACE from two more u64 locals and two runtime rotate counts, so the
// argument setup itself needs more than sixteen felts. The spill analysis
// does not see `hir.exec_indirect` arguments (operand group 1), spills four
// of them and never reloads them; the emitter keeps them physically (their
// use at the dispatch is real), and the first arity-1 `arith.trunc` with a
// Copy-constrained deep u64 operand aborts in the emitter with `invalid
// operand stack index (11): requires access to more than 16 elements`
// (codegen/masm/src/emit/mod.rs:623). See the ignored test in
// tests/calls.rs; `case_direct_args.rs` is the passing direct-call twin.
type Wide = fn(u64, u64, u64, u64, u64, u64, u64) -> u64;

#[inline(never)]
fn w_fold(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    a.wrapping_add(b).wrapping_mul(c | 1).wrapping_sub(d).rotate_left((e & 63) as u32) ^ f.wrapping_add(g)
}

#[inline(never)]
fn w_zip(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64) -> u64 {
    (a ^ b.rotate_right(17)).wrapping_add(c.wrapping_mul(c)).wrapping_add(d >> 3).wrapping_add(e << 5).wrapping_add(f ^ g.swap_bytes())
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
    let c = input1 & 63;
    let d = (input2 >> 6) & 63;
    let f = WIDES[(input1 & 1) as usize];
    let r = f(v0, v1, v2, v3.rotate_left(c), v4, v5.rotate_right(d), v6);
    let z = r ^ v0.rotate_left(c) ^ v1.rotate_right(d) ^ v2 ^ v3 ^ v4 ^ v5 ^ v6;
    (z as u32) ^ ((z >> 32) as u32)
}
