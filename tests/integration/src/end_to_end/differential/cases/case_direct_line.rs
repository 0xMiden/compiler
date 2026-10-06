// Probe: DIRECT twin of case_p_indirect_stk.rs — a loop-free pinned call with two single-use u64 call results
// kept on the wasm value stack UNDER the dispatch (LLVM stackifies single-use
// values across calls), so the frontend sees SSA values live across
// `hir.exec_indirect` in one block: 14 argument felts + table index + 4
// felts live-through = 19 > 16, forcing spills right before the dispatch.
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
    let n0 = w_zip(y, x, v0, v1, v2, v3, v4);
    let n1 = w_zip(x, v6, v5, v4, v3, v2, v1);
    let sel = (input1 & 1) as u64;
    let r = w_fold(v0, v1, v2, v3, v4, v5, v6) ^ sel;
    let z = n0.wrapping_add(r).wrapping_mul(n1 | 1);
    (z as u32) ^ ((z >> 32) as u32)
}
