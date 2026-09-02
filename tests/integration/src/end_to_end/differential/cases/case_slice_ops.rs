// Pointer arithmetic and slicing over a stack buffer and a static sorted
// table: runtime start/length sub-slices (`&buf[a..a + len]`), iteration
// over `chunks_exact(3)` and `windows(5)`, `split_at` at a runtime point,
// reversed in-place iteration writing back, element `swap`s, and
// `binary_search` over the static table. Slice rotation is deliberately
// absent: core's `ptr_rotate` moves overlapping ranges with `memmove`,
// which is the known overlapping `memory.copy` bug.
static SORTED: [u32; 24] = [
    2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987, 1597, 2584, 4181, 6765, 10946, 17711,
    28657, 46368, 75025, 121393,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 48];
    let mut k = 0usize;
    while k < 48 {
        buf[k] = (input1.wrapping_mul(k as u32 + 3) ^ (input2 >> (k & 7))) as u8;
        k += 1;
    }

    // Runtime sub-slice.
    let a = (input1 % 20) as usize;
    let len = (input2 % 24) as usize;
    let sub = &buf[a..a + len];
    let mut acc = 0u32;
    for (n, &v) in sub.iter().enumerate() {
        acc = acc.rotate_left(3) ^ (v as u32).wrapping_mul(n as u32 | 1);
    }

    // chunks_exact(3) and windows(5) over another runtime sub-slice.
    let b = (input2 % 8) as usize;
    let part = &buf[b..b + 33];
    for c in part.chunks_exact(3) {
        acc = acc.wrapping_add((c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32);
    }
    let rem = part.chunks_exact(3).remainder();
    acc ^= rem.len() as u32;
    for w in part.windows(5) {
        acc = acc.rotate_left(1) ^ (w[0] as u32 ^ w[4] as u32) ^ ((w[2] as u32) << 8);
    }

    // split_at at a runtime point, reversed in-place rewrite of the tail.
    let mid = (input1 % 40) as usize + 4;
    let (head, tail) = buf.split_at_mut(mid);
    let hsum = head.iter().fold(0u32, |s, &v| s.wrapping_add(v as u32));
    for (n, v) in tail.iter_mut().rev().enumerate() {
        *v = v.wrapping_add((hsum >> (n & 7)) as u8);
    }
    let tlen = tail.len();
    acc = acc.wrapping_add(hsum.rotate_left(5)).wrapping_add(tail[tlen - 1] as u32);

    // Element swaps at runtime indexes.
    let i = (input1 % 48) as usize;
    let j = (input2 % 48) as usize;
    buf.swap(i, j);
    buf.swap((i + 17) % 48, (j + 29) % 48);
    acc = acc.wrapping_add(buf[i] as u32).wrapping_add((buf[j] as u32) << 8);

    // binary_search over the static sorted table (hits and misses).
    let probe = input1 % 130000;
    let bs = match SORTED.binary_search(&probe) {
        Ok(p) => (p as u32) | 0x100,
        Err(p) => p as u32,
    };
    let bs2 = match SORTED.binary_search(&SORTED[(input2 % 24) as usize]) {
        Ok(p) => p as u32,
        Err(_) => 0xdead,
    };
    acc = acc.wrapping_add(bs.rotate_left(9)).wrapping_add(bs2 << 20);

    k = 0;
    while k < 48 {
        acc = acc.rotate_left(1) ^ (buf[k] as u32);
        k += 1;
    }
    acc
}
