// recursion_indirect x indirect_wide (campaign 14): bounded recursion
// THROUGH a fn-pointer table with a five-u64 signature (ten argument felts
// + the table index), depth `input1 % 6`, every frame keeping two u64 of
// state live across its dispatch (non-tail: the callee's result is mixed
// with the frame's state), the callee chosen per frame from the frame's
// own state, and two independent recursions from the entrypoint whose
// depths come from both inputs.
type Rec = fn(u64, u64, u64, u64, u64) -> u64;

#[inline(never)]
fn rec_a(d: u64, a: u64, b: u64, c: u64, e: u64) -> u64 {
    let s = a.wrapping_mul(b | 1) ^ c.rotate_left((e & 63) as u32);
    if d == 0 {
        return s ^ e;
    }
    let f = TABLE[(s & 1) as usize];
    let r = f(d - 1, s, a ^ d, b.wrapping_add(c), e.rotate_left(3));
    r.wrapping_add(s).rotate_left(7) ^ a
}

#[inline(never)]
fn rec_b(d: u64, a: u64, b: u64, c: u64, e: u64) -> u64 {
    let s = a.rotate_right(11) ^ b.wrapping_mul(c | 1) ^ e;
    if d == 0 {
        return s.wrapping_add(a);
    }
    let f = TABLE[((s >> 3) & 1) as usize];
    let r = f(d - 1, b, s, a.wrapping_sub(e), c ^ d);
    (r ^ s).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ b
}

static TABLE: [Rec; 2] = [rec_a, rec_b];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let f = TABLE[(input2 & 1) as usize];
    let r1 = f((input1 % 6) as u64, x, y, x ^ y, x.wrapping_add(y));
    let g = TABLE[((input1 >> 1) & 1) as usize];
    let r2 = g((input2 % 6) as u64, r1, x, y.rotate_left(13), r1 ^ y);
    let z = r1 ^ r2.rotate_left(21) ^ x;
    (z as u32) ^ ((z >> 32) as u32)
}
