// big_frame x mut_arrays x dispatch (campaign 14): a 2 KiB stack frame
// (`[u32; 512]`) escapes as a runtime-bounded `&mut [u32]` sub-slice (fat
// pointer: address + length) through fn-pointer dispatch into helpers that
// fill it, copy a runtime-length prefix onto its (disjoint) suffix and xor-fold it, with a u64
// carried across the dispatches in a loop; the window position and length
// are input-driven and the result is read back at runtime indexes.
type Op = fn(&mut [u32], u32, u64) -> u64;

#[inline(never)]
fn op_fill(buf: &mut [u32], seed: u32, k: u64) -> u64 {
    let mut i = 0u32;
    for w in buf.iter_mut() {
        *w = seed.wrapping_mul(i | 1) ^ k as u32;
        i = i.wrapping_add(1);
    }
    (buf.len() as u64) ^ k
}

#[inline(never)]
fn op_shift(buf: &mut [u32], n: u32, k: u64) -> u64 {
    // Runtime-length copy of a prefix onto the suffix, always disjoint
    // (h <= len / 2): an overlapping copy_within is the known mem_overlap trap.
    let len = buf.len();
    let h = 1 + (n as usize) % (len / 2);
    buf.copy_within(0..h, len - h);
    (buf[len - h] as u64) ^ k.rotate_left(5)
}

#[inline(never)]
fn op_xor(buf: &mut [u32], m: u32, k: u64) -> u64 {
    let mut a = k;
    for w in buf.iter_mut() {
        *w ^= m;
        a = a.wrapping_add(*w as u64).rotate_left(1);
    }
    a
}

static OPS: [Op; 3] = [op_fill, op_shift, op_xor];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut frame = [0u32; 512];
    let lo = (input1 % 300) as usize;
    let len = 8 + (input2 % 200) as usize;
    let mut k = (input1 as u64) << 32 | input2 as u64;
    let mut sel = (input2 % 3) as usize;
    let mut i = 0u32;
    let trips = input1 % 5 + 2;
    while i < trips {
        let f = OPS[sel];
        let r = f(&mut frame[lo..lo + len], input1.wrapping_add(i), k);
        k = k.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ r;
        sel = (r % 3) as usize;
        i += 1;
    }
    let at = lo + (input2 % len as u32) as usize;
    let edge = frame[lo + len - 1] ^ frame[lo];
    frame[at] ^ edge.rotate_left(3) ^ (k as u32) ^ ((k >> 32) as u32) ^ (sel as u32)
}
