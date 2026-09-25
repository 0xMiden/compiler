// recursion_indirect x mut_arrays (campaign 14): bounded recursion THROUGH
// a fn-pointer table (depth `input1 % 6`) where every frame owns a stack
// array that escapes by `&mut` into the callee frame (address-taken locals
// in recursive frames: each dynexec level pushes its own shadow-stack frame
// while the caller's array stays live), the callee writes the array, and
// the frame reads it back after the call together with its own state.
type Rec = fn(u32, &mut [u64; 6], u64) -> u64;

#[inline(never)]
fn rec_a(d: u32, up: &mut [u64; 6], k: u64) -> u64 {
    let mut own = [k, k ^ 1, k.rotate_left(3), d as u64, 0, 0];
    up[(d as usize) % 6] = up[(d as usize) % 6].wrapping_add(k) ^ (d as u64);
    if d == 0 {
        return own[0] ^ own[2] ^ up[1];
    }
    let f = TABLE[(k & 1) as usize];
    let r = f(d - 1, &mut own, k.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ up[0]);
    own[4] = r;
    own[5] = own[0] ^ own[1] ^ own[2] ^ own[3] ^ own[4];
    up[5] ^= own[5];
    r.rotate_left(d).wrapping_add(own[(r % 6) as usize]) ^ up[(d as usize + 1) % 6]
}

#[inline(never)]
fn rec_b(d: u32, up: &mut [u64; 6], k: u64) -> u64 {
    let mut own = [0u64; 6];
    let mut i = 0;
    while i < 6 {
        own[i] = up[i].rotate_left(i as u32 * 5) ^ k;
        i += 1;
    }
    if d == 0 {
        return own[3].wrapping_sub(own[5]);
    }
    let f = TABLE[((k >> 2) & 1) as usize];
    let seed = own[(d as usize) % 6];
    let r = f(d - 1, &mut own, seed);
    up[(d as usize) % 6] = r ^ own[0];
    r.wrapping_mul(own[1] | 1) ^ own[(r % 6) as usize]
}

static TABLE: [Rec; 2] = [rec_a, rec_b];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let mut top = [x, x ^ 0xffff, input2 as u64, x.rotate_left(11), 0, 1];
    let f = TABLE[(input2 & 1) as usize];
    let r = f(input1 % 6, &mut top, x);
    let g = TABLE[((input1 >> 3) & 1) as usize];
    let s = g(input2 % 6, &mut top, r);
    let mut z = r ^ s.rotate_left(13);
    let mut i = 0;
    while i < 6 {
        z = z.rotate_left(7) ^ top[i];
        i += 1;
    }
    (z as u32) ^ ((z >> 32) as u32)
}
