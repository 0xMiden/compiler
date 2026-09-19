// Iterator pipelines and non-recursive `core` slice algorithms (campaign
// 17, program 16): a `[u32; 64]` xorshift-filled stack array goes through
// `rotate_left` by a runtime count, `reverse`, `split_at_mut` +
// `iter_mut().zip()`, `swap`, `fill`, `copy_from_slice`, a heap sort, and
// then `binary_search`, `windows(2).all`, `chunks_exact().map().fold`,
// `iter().rev().enumerate()`, `max_by_key`, `position` / `rposition`,
// `filter().count()`, `step_by`, `take_while`, `skip`, `cycle().take()`,
// `nth`, `last`, `min`, `any` and `Option` combinators on `checked_*`
// chains; every result and the arrays are folded into the result.
fn fill(arr: &mut [u32; 64], seed: u32) {
    let mut x = seed | 1;
    for (i, slot) in arr.iter_mut().enumerate() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *slot = if i % 5 == 2 { x & 0xffff } else { x };
    }
}

fn sift_down(a: &mut [u32], start: usize, end: usize) {
    let mut root = start;
    while 2 * root + 1 < end {
        let mut child = 2 * root + 1;
        if child + 1 < end && a[child] < a[child + 1] {
            child += 1;
        }
        if a[root] >= a[child] {
            return;
        }
        a.swap(root, child);
        root = child;
    }
}

fn heap_sort(a: &mut [u32]) {
    let n = a.len();
    for start in (0..n / 2).rev() {
        sift_down(a, start, n);
    }
    for end in (1..n).rev() {
        a.swap(0, end);
        sift_down(a, 0, end);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut base = [0u32; 64];
    fill(&mut base, input1 ^ input2.rotate_left(16) ^ 0x2545_f491);
    let mut c = base;
    c.rotate_left((input1 % 64) as usize);
    c.reverse();
    {
        let (lo, hi) = c.split_at_mut(32);
        for (x, y) in lo.iter_mut().zip(hi.iter()) {
            *x = x.wrapping_add(*y);
        }
        hi[..(input2 % 16) as usize].fill(input1);
    }
    c.swap((input1 % 64) as usize, (input2 % 64) as usize);
    let mut a = base;
    heap_sort(&mut a);
    let mut d = [0u32; 64];
    d[..32].copy_from_slice(&a[32..]);
    d[32..].copy_from_slice(&a[..32]);
    let key = base[(input2 % 64) as usize];
    let found = a.binary_search(&key).map(|i| i as u32).unwrap_or(0x1000);
    let missing = a
        .binary_search(&(key ^ 1))
        .map(|i| i as u32 | 0x2000)
        .unwrap_or_else(|i| i as u32 | 0x4000);
    let sorted = a.windows(2).all(|w| w[0] <= w[1]) as u32;
    let zipped = base
        .iter()
        .zip(a.iter())
        .fold(0u32, |acc, (x, y)| acc.rotate_left(1) ^ x.wrapping_add(*y));
    let chunked = base
        .chunks_exact(4)
        .map(|ch| ch[0] ^ ch[1].rotate_left(8) ^ ch[2].rotate_left(16) ^ ch[3].rotate_left(24))
        .fold(0u32, |acc, v| acc.wrapping_mul(31).wrapping_add(v));
    let rev = c
        .iter()
        .rev()
        .enumerate()
        .fold(0u32, |acc, (i, v)| acc ^ v.wrapping_add(i as u32));
    let (max_i, max_v) = base
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| **v)
        .map(|(i, v)| (i as u32, *v))
        .unwrap_or((99, 0));
    let pos = base.iter().position(|&v| v == key).unwrap_or(77) as u32;
    let rpos = base.iter().rposition(|&v| v & 1 == 1).unwrap_or(78) as u32;
    let small = base.iter().filter(|&&v| v < input1).count() as u32;
    let stepped = base
        .iter()
        .step_by(1 + (input2 % 7) as usize)
        .fold(0u32, |acc, &v| acc.wrapping_add(v));
    let prefix = a.iter().take_while(|&&v| v < input2).count() as u32;
    let skipped = a.iter().skip((input1 % 70) as usize).fold(0u32, |acc, &v| acc ^ v);
    let cyc = d
        .iter()
        .cycle()
        .take((input2 % 200) as usize)
        .fold(0u32, |acc, &v| acc.rotate_left(3) ^ v);
    let nth = base.iter().nth((input1 % 80) as usize).copied().unwrap_or(0xaaaa);
    let last = a.iter().rev().find(|&&v| v & 0xff == 0).copied().unwrap_or(0xbbbb);
    let mn = c.iter().copied().min().unwrap_or(1);
    let any = base.iter().any(|&v| v == input2) as u32;
    let chk = input1
        .checked_add(input2)
        .and_then(|s| s.checked_mul(3))
        .and_then(|s| s.checked_sub(input1 >> 1))
        .map(|s| s.rotate_left(4))
        .unwrap_or(0xcccc);
    let pw = 3u32.checked_pow(input2 % 25).unwrap_or(1);
    let flags = sorted | any << 1;
    let mut h = flags.wrapping_mul(0x9e37_79b9) ^ found << 8 ^ missing << 16 ^ pos << 24 ^ rpos;
    for v in [
        zipped, chunked, rev, max_v, max_i, small, stepped, prefix, skipped, cyc, nth, last, mn,
        chk, pw,
    ] {
        h = h.rotate_left(5) ^ v;
    }
    for i in 0..64 {
        h = h.rotate_left(3) ^ a[i] ^ c[63 - i].rotate_left(7) ^ d[i].rotate_left(11);
    }
    h
}
