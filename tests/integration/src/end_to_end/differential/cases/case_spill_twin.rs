// Two SEQUENTIAL branch diamonds whose arms are wide expression trees over
// ~12 mixed u64/u32 locals. The pipeline batches each arm's `hir.load_local`s
// ahead of the (sunk) arithmetic, so 20+ felts go live inside four different
// arms plus both join tails: values are spilled once and reloaded
// independently in sibling arms of BOTH diamonds and used after BOTH joins —
// iterated dominance-frontier phi insertion across two join blocks, spill
// uses inside two separate scf regions after lifting, and mixed 1-felt/2-felt
// spill candidates for the MIN size tie-breakers.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
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
    let u0 = (input1 ^ 0x85eb_ca6b).wrapping_mul(0xc2b2_ae35) | 1;
    let u1 = (input2 ^ 0x27d4_eb2f).wrapping_mul(0x1656_67b1) | 1;
    // First diamond.
    let t = if m % 97 < 48 {
        (v0 ^ v9.rotate_left(1))
            .wrapping_add(v1 ^ v8.rotate_left(3))
            .wrapping_mul(v2 | 1)
            ^ v7.rotate_left(u0 & 31)
            ^ (v3 ^ v6.rotate_left(7)).wrapping_sub(v4 ^ v5.rotate_left(9))
    } else {
        (v9 ^ v0.rotate_left(2))
            .wrapping_sub(v8 ^ v1.rotate_left(4))
            .wrapping_mul(v7 | 1)
            ^ v2.rotate_left(u1 & 31)
            ^ (v6 ^ v3.rotate_left(8)).wrapping_add(v5 ^ v4.rotate_left(10))
    };
    // First join: uses of values reloaded inside both arms.
    let mid = t ^ v0.rotate_left(25) ^ v5.rotate_left(27) ^ ((u0 ^ u1) as u64);
    // Second diamond over the SAME locals, different consumption orders.
    let s = if u0 % 89 < 44 {
        (v4 ^ mid.rotate_left(6))
            .wrapping_mul(v1 | 1)
            .wrapping_add(v6 ^ v2.rotate_left(12))
            .wrapping_sub(v8 ^ v0.rotate_left(14))
            ^ v3.rotate_left(u1 & 31)
    } else {
        (v8 ^ mid.rotate_left(15))
            .wrapping_add(v3 ^ v7.rotate_left(18))
            .wrapping_mul(v9 | 1)
            .wrapping_sub(v2 ^ v6.rotate_left(20))
            ^ v0.rotate_left(u0 & 31)
    };
    // Second join: more uses of the same spilled values.
    let r = s
        ^ v1.rotate_left(33)
        ^ v4.rotate_left(37)
        ^ v7.rotate_left(41)
        ^ v9.rotate_left(43)
        ^ ((u0.rotate_left(5) ^ u1) as u64) << 32;
    (r as u32) ^ ((r >> 32) as u32) ^ u0.rotate_left(u1 & 31)
}
