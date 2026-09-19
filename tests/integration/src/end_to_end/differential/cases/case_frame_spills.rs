// Spill slots next to a large stack array. One function owns a
// `[MaybeUninit<u32>; 65536]` local (256 KiB of the 1 MiB wasm shadow stack)
// and runs the campaign-20 freight recipe around it: a stride-1637 stripe is
// written from the inputs BEFORE an eight-u64 cluster is spilled across a
// loop, the array's first and last elements are written at a runtime index
// WHILE the cluster is spilled, and the whole stripe is checksummed after the
// cluster is reloaded. The mirror order follows: a second spilled loop, then
// stripe writes, then the reloads. A spill slot overlapping the frame array
// would change either the checksum or the reloaded cluster.
use core::mem::MaybeUninit;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut big: [MaybeUninit<u32>; 65536] = [MaybeUninit::uninit(); 65536];
    let m = (input1 | 1) as u64;
    let q = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    // Stripe written before any spill.
    let seed = input1.wrapping_add(input2);
    let mut w: u32 = 0;
    while w < 40 {
        let idx = ((w.wrapping_mul(1637)).wrapping_add(seed % 1637) % 65536) as usize;
        big[idx].write(seed.wrapping_mul(0x9e37_79b9) ^ w.rotate_left(3));
        w = w.wrapping_add(1);
    }
    // The extremes are addressed by a runtime index that is 0 or 65535.
    let ext = ((input1 & 1) as usize) * 65535;
    big[ext].write(seed ^ 0x1111_1111);
    big[65535 - ext].write(seed ^ 0x2222_2222);

    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q;
    let v1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ q.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let trips = (input2 % 3) + 2;
    let mut acc = m ^ q.rotate_left(7);
    let mut i: u32 = 0;
    while i < trips {
        acc = acc.rotate_left(1)
            ^ (v0 ^ v1.rotate_left(3))
                .wrapping_add(v2 ^ v3.rotate_left(5))
                .wrapping_mul(v4 | 1)
            ^ (v5 ^ v6.rotate_left(7)).wrapping_sub(v7 ^ acc.rotate_left(9));
        // Frame writes at the extremes while the cluster is spilled.
        big[ext].write((acc as u32) ^ i);
        big[65535 - ext].write(((acc >> 32) as u32) ^ i.rotate_left(5));
        i = i.wrapping_add(1);
    }
    // Reloads after the loop, then the stripe checksum.
    let mut out = acc
        ^ v0.rotate_left(15)
        ^ v1.rotate_left(17)
        ^ v2.rotate_left(19)
        ^ v3.rotate_left(21)
        ^ v4.rotate_left(23)
        ^ v5.rotate_left(25)
        ^ v6.rotate_left(27)
        ^ v7.rotate_left(29);
    let mut r: u32 = 0;
    let mut sum: u32 = 0;
    while r < 40 {
        let idx = ((r.wrapping_mul(1637)).wrapping_add(seed % 1637) % 65536) as usize;
        // Written by the first stripe loop at exactly this index.
        sum = sum.rotate_left(1) ^ unsafe { big[idx].assume_init() };
        r = r.wrapping_add(1);
    }
    sum = sum.wrapping_add(unsafe { big[ext].assume_init() });
    sum ^= unsafe { big[65535 - ext].assume_init() }.rotate_left(9);

    // Mirror: spill first, write the frame afterwards, reload last.
    let mut acc2 = out ^ (sum as u64);
    let mut j: u32 = 0;
    while j < trips {
        acc2 = acc2.rotate_left(1)
            ^ (v7 ^ v6.rotate_left(3))
                .wrapping_add(v5 ^ v4.rotate_left(5))
                .wrapping_mul(v3 | 1)
            ^ (v2 ^ v1.rotate_left(7)).wrapping_sub(v0 ^ acc2.rotate_left(9));
        j = j.wrapping_add(1);
    }
    let mut w2: u32 = 0;
    while w2 < 40 {
        let idx = ((w2.wrapping_mul(1637)).wrapping_add(seed % 1637) % 65536) as usize;
        big[idx].write((acc2 as u32) ^ w2.rotate_left(7));
        w2 = w2.wrapping_add(1);
    }
    out ^= acc2 ^ v2.rotate_left(3) ^ v5.rotate_left(11);
    let mut r2: u32 = 0;
    let mut sum2: u32 = 0;
    while r2 < 40 {
        let idx = ((r2.wrapping_mul(1637)).wrapping_add(seed % 1637) % 65536) as usize;
        sum2 = sum2.rotate_left(1) ^ unsafe { big[idx].assume_init() };
        r2 = r2.wrapping_add(1);
    }
    (out as u32) ^ ((out >> 32) as u32) ^ sum ^ sum2.rotate_left(13)
}
