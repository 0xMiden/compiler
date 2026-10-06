// Bit-permutation kernel: the classic mask/shift u64 bit reversal, Morton
// (Z-order) encode and decode of 2D grid coordinates, and a byte swap, run
// over a table of points.  SIX distinct shift constants (1, 2, 4, 8, 16, 32)
// by construction -- the halving ladder every one of these algorithms uses --
// shared between the reversal, both Morton directions and the byte swap, and
// they cross the point loop and the final fold.
fn reverse_bits64(mut v: u64) -> u64 {
    v = ((v >> 1) & 0x5555_5555_5555_5555) | ((v & 0x5555_5555_5555_5555) << 1);
    v = ((v >> 2) & 0x3333_3333_3333_3333) | ((v & 0x3333_3333_3333_3333) << 2);
    v = ((v >> 4) & 0x0f0f_0f0f_0f0f_0f0f) | ((v & 0x0f0f_0f0f_0f0f_0f0f) << 4);
    v = ((v >> 8) & 0x00ff_00ff_00ff_00ff) | ((v & 0x00ff_00ff_00ff_00ff) << 8);
    v = ((v >> 16) & 0x0000_ffff_0000_ffff) | ((v & 0x0000_ffff_0000_ffff) << 16);
    (v >> 32) | (v << 32)
}

fn morton_spread(mut v: u64) -> u64 {
    v &= 0x0000_0000_ffff_ffff;
    v = (v | (v << 16)) & 0x0000_ffff_0000_ffff;
    v = (v | (v << 8)) & 0x00ff_00ff_00ff_00ff;
    v = (v | (v << 4)) & 0x0f0f_0f0f_0f0f_0f0f;
    v = (v | (v << 2)) & 0x3333_3333_3333_3333;
    (v | (v << 1)) & 0x5555_5555_5555_5555
}

fn morton_compact(mut v: u64) -> u64 {
    v &= 0x5555_5555_5555_5555;
    v = (v | (v >> 1)) & 0x3333_3333_3333_3333;
    v = (v | (v >> 2)) & 0x0f0f_0f0f_0f0f_0f0f;
    v = (v | (v >> 4)) & 0x00ff_00ff_00ff_00ff;
    v = (v | (v >> 8)) & 0x0000_ffff_0000_ffff;
    (v | (v >> 16)) & 0x0000_0000_ffff_ffff
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let points = 8 + (input2 % 25) as usize;
    let mut acc = 0u64;
    let mut rev_acc = 0u64;
    let mut swapped = 0u64;
    let mut mismatches = 0u64;

    let mut x = input1 | 1;
    let mut p = 0usize;
    while p < points {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let cx = (x >> 16) & 0xffff;
        let cy = (x ^ input2) & 0xffff;

        // Morton encode, then decode back and check the round trip.
        let code = morton_spread(cx as u64) | (morton_spread(cy as u64) << 1);
        let back_x = morton_compact(code);
        let back_y = morton_compact(code >> 1);
        if back_x != cx as u64 || back_y != cy as u64 {
            mismatches = mismatches.wrapping_add(1);
        }

        // Bit-reversal permutation index for a 2^16-point transform.
        let rev = reverse_bits64(code) >> 32;
        rev_acc = rev_acc.rotate_left(1) ^ rev;

        // Byte swap of the code, assembled from the same halving ladder.
        let mut s = code;
        s = ((s >> 8) & 0x00ff_00ff_00ff_00ff) | ((s & 0x00ff_00ff_00ff_00ff) << 8);
        s = ((s >> 16) & 0x0000_ffff_0000_ffff) | ((s & 0x0000_ffff_0000_ffff) << 16);
        s = (s >> 32) | (s << 32);
        swapped ^= s;

        acc = acc.wrapping_add(code ^ (rev << 2)) ^ (swapped >> 4);
        p += 1;
    }

    // Fold with the same ladder.
    let mut out = acc ^ rev_acc ^ swapped ^ mismatches;
    out = ((out >> 1) & 0x5555_5555_5555_5555) | ((out & 0x5555_5555_5555_5555) << 1);
    out = ((out >> 2) & 0x3333_3333_3333_3333) | ((out & 0x3333_3333_3333_3333) << 2);
    out = ((out >> 4) & 0x0f0f_0f0f_0f0f_0f0f) | ((out & 0x0f0f_0f0f_0f0f_0f0f) << 4);
    out = ((out >> 8) & 0x00ff_00ff_00ff_00ff) | ((out & 0x00ff_00ff_00ff_00ff) << 8);
    out = ((out >> 16) & 0x0000_ffff_0000_ffff) | ((out & 0x0000_ffff_0000_ffff) << 16);
    out = (out >> 32) | (out << 32);
    (out as u32) ^ ((out >> 32) as u32)
}
