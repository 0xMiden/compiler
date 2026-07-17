// Ten independent u64 values (20 stack felts) live across a two-way branch,
// consumed in different orders in each arm, with four of them still live after
// the join. Operand-stack pressure > 16 felts spans a multi-block CFG, so the
// first (pre-SCF) TransformSpills pass must insert spills/reloads across
// control-flow edges and restore SSA form with block-parameter phis
// (rewrite_cfg_spills / insert_required_phis), not just the straight-line
// single-block path that case_stack_pressure exercises.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    // Ten u64s, all live at the branch below (each is used in both arms).
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n;
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ n.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let v8 = v6.wrapping_add(v4.rotate_left(19)) ^ n.rotate_left(3);
    let v9 = v7.wrapping_sub(v5.rotate_left(21)) ^ m.rotate_left(5);
    // Long dependent chains in each arm (too big for if-conversion), consuming
    // the ten values in opposite orders so reloads land at different depths.
    let t = if m % 97 < 48 {
        let mut s = v0 ^ v9.rotate_left(1);
        s = s.wrapping_add(v1 ^ v8.rotate_left(3));
        s = s.wrapping_mul(v2 | 1) ^ v7.rotate_left(5);
        s = s.wrapping_sub(v3 ^ v6.rotate_left(7));
        s ^ v4 ^ v5.rotate_left(9)
    } else {
        let mut s = v9 ^ v0.rotate_left(2);
        s = s.wrapping_sub(v8 ^ v1.rotate_left(4));
        s = s.wrapping_mul(v7 | 1) ^ v2.rotate_left(6);
        s = s.wrapping_add(v6 ^ v3.rotate_left(8));
        s ^ v5 ^ v4.rotate_left(10)
    };
    // Join-block uses of values reloaded inside the arms: the reload in either
    // arm does not dominate these uses, forcing phi insertion at the join.
    let r = t
        ^ v0.rotate_left(25)
        ^ v3.rotate_left(33)
        ^ v6.rotate_left(41)
        ^ v9.rotate_left(49);
    (r as u32) ^ ((r >> 32) as u32)
}
