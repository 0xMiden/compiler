// Trap parity: `split_at` and `split_at_mut` on a `[u32; 8]` with the split
// point in 0..11, so `mid > 8` panics (`mid > len`) while `mid == 8` (the
// empty right half) and `mid == 0` (the empty left half) must not. The
// shared-borrow split is driven by `input1` and the mutable one by `input2`,
// so each is reachable independently.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut data: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let mid = (input1 % 11) as usize;
    let (lo, hi) = data.split_at(mid);
    let mut acc = ((lo.len() as u32) << 16) ^ (hi.len() as u32);
    for &v in lo {
        acc = acc.wrapping_add(v);
    }
    for &v in hi {
        acc = acc.rotate_left(3) ^ v;
    }
    let mid2 = (input2 % 11) as usize;
    let (left, right) = data.split_at_mut(mid2);
    if let Some(x) = left.first_mut() {
        *x = acc;
    }
    if let Some(y) = right.first_mut() {
        *y = acc.rotate_left(11);
    }
    let mut out = 0u32;
    let mut k = 0usize;
    while k < 8 {
        out = out.rotate_left(5) ^ data[k];
        k += 1;
    }
    acc ^ out
}
