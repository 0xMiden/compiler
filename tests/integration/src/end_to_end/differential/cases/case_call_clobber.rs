// Spill slots must SURVIVE a call whose callee spills heavily and owns a
// frame of its own. Three levels deep (`lvl_a` -> `lvl_b` -> `lvl_c`), each
// level defines a u64 cluster, spills it across a loop, calls the next level
// with the cluster live, and reads its own values back after the callee
// returns. Every level also owns a `[u32; 256]` frame that it zero-fills
// (`memset`) and then overwrites with a runtime-length `copy_from_slice`
// (`memcpy`), so a callee frame or a callee spill slot that aliased the
// caller's slots would be written with the callee's data before the caller
// reloads.
#[inline(never)]
fn lvl_c(n: u32, x: u64, y: u64) -> u64 {
    let mut own = [0u32; 256];
    let mut src = [0u32; 128];
    let c0 = x.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ y;
    let c1 = y.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ x.rotate_left(11);
    let c2 = c0.rotate_left(17) ^ y.wrapping_mul(0x94d0_49bb_1331_11eb);
    let c3 = c1.rotate_left(23) ^ x.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let c4 = c2.wrapping_add(c0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let c5 = c3.wrapping_sub(c1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let mut i: u32 = 0;
    while i < (n % 3) + 2 {
        src[(i as usize * 13) & 127] = (c0 as u32) ^ i;
        own[(i as usize * 29) & 255] = (c5 as u32) ^ i.rotate_left(3);
        i = i.wrapping_add(1);
    }
    let len = 96 + (n % 32) as usize;
    own[16..16 + len].copy_from_slice(&src[..len]);
    let mut acc = x ^ y.rotate_left(5);
    let mut j: u32 = 0;
    while j < (n % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (c0 ^ c1.rotate_left(3))
                .wrapping_add(c2 ^ c3.rotate_left(5))
                .wrapping_mul(c4 | 1)
            ^ (c5 ^ acc.rotate_left(7));
        j = j.wrapping_add(1);
    }
    acc ^ c0.rotate_left(15)
        ^ c1.rotate_left(19)
        ^ c2.rotate_left(23)
        ^ c3.rotate_left(27)
        ^ c4.rotate_left(31)
        ^ c5.rotate_left(35)
        ^ (own[(n as usize * 7) & 255] as u64)
        ^ ((src[(n as usize * 11) & 127] as u64) << 32)
}

#[inline(never)]
fn lvl_b(n: u32, x: u64, y: u64) -> u64 {
    let mut own = [0u32; 256];
    let mut src = [0u32; 128];
    let b0 = x.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ y.rotate_left(7);
    let b1 = y.wrapping_mul(0x5851_f42d_4c95_7f2d) ^ x.rotate_left(13);
    let b2 = b0.rotate_left(19) ^ y.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let b3 = b1.rotate_left(25) ^ x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let b4 = b2.wrapping_add(b0.rotate_left(3)) ^ 0x1234_5678_9abc_def0;
    let b5 = b3.wrapping_sub(b1.rotate_left(9)) ^ 0x0fed_cba9_8765_4321;
    let mut acc = x ^ y.rotate_left(3);
    let mut i: u32 = 0;
    while i < (n % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (b0 ^ b1.rotate_left(3))
                .wrapping_add(b2 ^ b3.rotate_left(5))
                .wrapping_mul(b4 | 1)
            ^ (b5 ^ acc.rotate_left(7));
        own[(i as usize * 17) & 255] = (acc as u32) ^ i;
        i = i.wrapping_add(1);
    }
    // The whole cluster is live across this call.
    let child = lvl_c(n.wrapping_add(1), acc | 1, b3 ^ b4);
    let len = 96 + ((n >> 2) % 32) as usize;
    src[..len].copy_from_slice(&own[32..32 + len]);
    acc ^= child.rotate_left(11);
    let mut j: u32 = 0;
    while j < (n % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (b5 ^ b4.rotate_left(3))
                .wrapping_add(b3 ^ b2.rotate_left(5))
                .wrapping_mul(b1 | 1)
            ^ (b0 ^ acc.rotate_left(7));
        j = j.wrapping_add(1);
    }
    acc ^ b0.rotate_left(15)
        ^ b2.rotate_left(21)
        ^ b5.rotate_left(29)
        ^ (src[(n as usize * 5) & 127] as u64)
}

#[inline(never)]
fn lvl_a(n: u32, x: u64, y: u64) -> u64 {
    let mut own = [0u32; 256];
    let a0 = x.wrapping_mul(0x8ebc_6af0_9c88_c6e3) ^ y.rotate_left(21);
    let a1 = y.wrapping_mul(0x5895_58cb_3521_e49d) ^ x.rotate_left(27);
    let a2 = a0.rotate_left(11) ^ y.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let a3 = a1.rotate_left(15) ^ x.wrapping_mul(0x94d0_49bb_1331_11eb);
    let a4 = a2.wrapping_add(a0.rotate_left(23)) ^ 0xa076_1d64_78bd_642f;
    let a5 = a3.wrapping_sub(a1.rotate_left(29)) ^ 0xe703_7ed1_a0b4_28db;
    let mut acc = x ^ y.rotate_left(9);
    let mut i: u32 = 0;
    while i < (n % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (a0 ^ a1.rotate_left(3))
                .wrapping_add(a2 ^ a3.rotate_left(5))
                .wrapping_mul(a4 | 1)
            ^ (a5 ^ acc.rotate_left(7));
        own[(i as usize * 23) & 255] = (acc >> 32) as u32;
        i = i.wrapping_add(1);
    }
    let child = lvl_b(n, acc | 1, a2 ^ a5);
    acc ^= child.rotate_left(13);
    let mut j: u32 = 0;
    while j < (n % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (a5 ^ a4.rotate_left(3))
                .wrapping_add(a3 ^ a2.rotate_left(5))
                .wrapping_mul(a1 | 1)
            ^ (a0 ^ acc.rotate_left(7));
        j = j.wrapping_add(1);
    }
    acc ^ a0.rotate_left(17)
        ^ a1.rotate_left(19)
        ^ a2.rotate_left(23)
        ^ a3.rotate_left(25)
        ^ a4.rotate_left(29)
        ^ a5.rotate_left(31)
        ^ (own[(n as usize * 3) & 255] as u64)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = ((input1 as u64) << 32) | input2 as u64;
    let y = ((input2 as u64) << 32) | input1 as u64;
    let r = lvl_a(input1 % 7, x | 1, y | 2);
    (r as u32) ^ ((r >> 32) as u32)
}
