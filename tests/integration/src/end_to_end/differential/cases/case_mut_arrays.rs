// Helpers taking `&mut` stack arrays and slices (address-taken locals, so
// every element lives in the caller's frame): `fill` takes two array
// references plus six u64 and one u32 (15 felts) and writes through both;
// `slice_mix` takes a `&mut [u32]` and a `&[u64]` (fat-pointer pairs) cut
// from the arrays at runtime bounds; `swap_halves` permutes an array in
// place through `swap`; `sum` reads both arrays through shared references.
// The same arrays go to consecutive calls with u64 scalars kept live in
// between, and the caller reads elements back at runtime indexes after each
// call, so a stale element, a wrong slice bound or a clobbered scalar shows.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn fill(
    a: &mut [u32; 16],
    b: &mut [u64; 8],
    k: u32,
    s: u64,
    t: u64,
    u: u64,
    v: u64,
    w: u64,
    x: u64,
) -> u32 {
    let mut i = 0usize;
    while i < 16 {
        a[i] = k.wrapping_mul(i as u32 + 1) ^ ((s >> (i & 31)) as u32);
        i += 1;
    }
    b[0] = s;
    b[1] = t;
    b[2] = u;
    b[3] = v;
    b[4] = w;
    b[5] = x;
    b[6] = s ^ t;
    b[7] = u.wrapping_add(v) ^ w ^ x;
    a[(k % 16) as usize] ^ (b[(k % 8) as usize] as u32)
}

#[inline(never)]
fn slice_mix(s: &mut [u32], t: &[u64], k: u32) -> u64 {
    let mut acc = k as u64;
    let mut i = 0usize;
    while i < s.len() {
        s[i] = s[i].rotate_left(k & 31) ^ (t[i % t.len()] as u32);
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3) ^ s[i] as u64;
        i += 1;
    }
    acc
}

#[inline(never)]
fn swap_halves(a: &mut [u32; 16], r: u32) {
    let mut i = 0usize;
    while i < 8 {
        let j = (i + (r as usize & 7)) & 7;
        a.swap(j, 8 + i);
        i += 1;
    }
}

#[inline(never)]
fn sum(a: &[u32; 16], b: &[u64; 8]) -> u64 {
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < 16 {
        acc = acc.rotate_left(3) ^ (a[i] as u64) ^ b[i & 7].rotate_left(i as u32);
        i += 1;
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 16];
    let mut b = [0u64; 8];
    let x = ((input1 as u64) << 32) | input2 as u64;
    let y = x.rotate_left(21) ^ 0x9e37_79b9_7f4a_7c15;
    let f = fill(
        &mut a,
        &mut b,
        input1,
        x,
        y,
        x ^ y,
        x.rotate_left(3),
        y.wrapping_mul(3),
        x ^ 0xff,
    );
    let e0 = a[(input2 % 16) as usize];
    let lo = (input2 % 8) as usize;
    let hi = lo + 1 + (input1 % 8) as usize;
    let tlo = (input1 % 4) as usize;
    let thi = tlo + 1 + (input2 % 4) as usize;
    let m = slice_mix(&mut a[lo..hi], &b[tlo..thi], input2);
    let e1 = a[(m % 16) as usize];
    swap_halves(&mut a, input1 ^ input2);
    let s = sum(&a, &b);
    let e2 = b[(s % 8) as usize];
    let e3 = a[((s >> 8) % 16) as usize];
    let z = x
        ^ y.rotate_left(7)
        ^ m
        ^ s.rotate_left(13)
        ^ e2
        ^ ((e0 as u64) << 32)
        ^ e1 as u64
        ^ ((e3 as u64) << 16);
    (z as u32) ^ ((z >> 32) as u32) ^ f
}
