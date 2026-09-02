// Large stack frame: a `[u64; 200]` and a `[u8; 900]` local (2.5 KiB of
// shadow-stack frame, zero-filled by `memory.fill`) whose addresses escape
// to an `#[inline(never)]` helper by reference, alongside promotable
// scalars kept live across the call; a second helper takes its own 512-byte
// frame below the caller's (frame pointer bookkeeping through the
// `__stack_pointer` global), reads through the caller's arrays at runtime
// indexes and writes them back. Every element of both arrays is folded into
// the result, so a frame slot reused while live or a mis-sized frame shows.
#[inline(never)]
fn touch(big: &mut [u64; 200], small: &mut [u8; 900], k: u32) -> u32 {
    let mut i = 0usize;
    while i < 200 {
        big[i] = (k as u64).wrapping_mul(i as u64 + 1) ^ ((i as u64) << 40);
        i += 1;
    }
    i = 0;
    while i < 900 {
        small[i] = (k >> (i & 7)) as u8 ^ (i as u8);
        i += 1;
    }
    let a = big[(k % 200) as usize];
    let b = small[(k % 900) as usize] as u32;
    (a as u32) ^ ((a >> 32) as u32).rotate_left(5) ^ b
}

#[inline(never)]
fn below(big: &[u64; 200], small: &mut [u8; 900], k: u32) -> u32 {
    let mut own = [0u32; 128];
    let mut i = 0usize;
    while i < 128 {
        let v = big[(i * 3 + (k % 200) as usize) % 200];
        own[i] = (v as u32) ^ ((v >> 32) as u32).wrapping_mul(k | 1);
        i += 1;
    }
    let mut acc = 0u32;
    i = 0;
    while i < 128 {
        let j = (i.wrapping_mul(7) + (k & 127) as usize) & 127;
        acc = acc.rotate_left(3) ^ own[j];
        small[(j * 7) % 900] = acc as u8;
        i += 1;
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut big = [0u64; 200];
    let mut small = [0u8; 900];
    // Promotable scalars live across both calls.
    let x = input1.rotate_left(7) ^ input2;
    let y = (input1 as u64) << 32 | (input2 as u64).rotate_left(3);
    let t = touch(&mut big, &mut small, input1 ^ input2.rotate_left(13));
    let u = below(&big, &mut small, input2);
    let mut acc = t ^ u.rotate_left(9) ^ x ^ (y as u32) ^ ((y >> 32) as u32);
    let mut i = 0usize;
    while i < 200 {
        acc = acc.rotate_left(1) ^ (big[i] as u32) ^ ((big[i] >> 32) as u32);
        i += 1;
    }
    i = 0;
    while i < 900 {
        acc = acc.wrapping_add((small[i] as u32).wrapping_mul(i as u32 | 1));
        i += 1;
    }
    acc ^ x.wrapping_mul(y as u32 | 1)
}
