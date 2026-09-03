// ret_area x exit_values (campaign 14): wide by-value helper results decide
// and CARRY the exits of a zero-trip-capable loop nest. `split` returns a
// `(u64, u64)`-shaped record through a return area, `widen` an
// `Option<u128>` and `narrow` a `Result<u64, u32>`; the u128 payloads leave
// the nest through a labeled `break 'outer <u128>` from the inner loop, a
// value-carrying break after the inner loop and the outer header exit, so
// the lifted exit dispatch threads four-felt result columns, while an
// `Err` arm may `return` early. Exit tags: the header exit (input2 % 61 ==
// 0), the `None` labeled break, the `Err` return, the post-inner break.
#[repr(C)]
struct Pair {
    lo: u64,
    hi: u64,
}

#[inline(never)]
fn split(a: u64, b: u64) -> Pair {
    Pair {
        lo: a.wrapping_mul(b | 1),
        hi: a ^ b.rotate_left(7),
    }
}

#[inline(never)]
fn widen(p: &Pair, k: u32) -> Option<u128> {
    if k % 5 == 3 {
        None
    } else {
        Some((((p.hi as u128) << 64) | p.lo as u128).wrapping_mul(0x1_0000_0000_0000_0003))
    }
}

#[inline(never)]
fn narrow(w: u128) -> Result<u64, u32> {
    if ((w >> 64) as u64) & 7 == 5 {
        Err((w as u32) | 1)
    } else {
        Ok((w as u64) ^ ((w >> 64) as u64))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input2 % 61;
    let mut a = input1 as u64 | 1;
    let mut b = input2 as u64 ^ 0x5555;
    let mut i = 0u32;
    let mut tag = 0u32;
    let wide: u128 = 'outer: loop {
        if i >= n {
            // Header exit carrying a value built from the carried state.
            break 'outer ((a as u128) << 64) | b as u128;
        }
        let p = split(a, b.wrapping_add(i as u64));
        let m = (input1 >> (i & 7)) % 5 + 1;
        let mut j = 0u32;
        let acc: u128 = loop {
            match widen(&p, j.wrapping_add(i)) {
                Some(w) => {
                    match narrow(w) {
                        Ok(v) => a = a.rotate_left(5) ^ v,
                        Err(e) => {
                            tag = tag.wrapping_add(e);
                            if e & 0x10 != 0 {
                                return tag ^ (a as u32) ^ 0x4000_0000;
                            }
                        }
                    }
                    if j + 1 >= m {
                        break w;
                    }
                }
                None => break 'outer (p.lo as u128 | ((p.hi as u128) << 64)) ^ (j as u128),
            }
            j += 1;
        };
        b = b.wrapping_add(acc as u64) ^ ((acc >> 64) as u64);
        if (acc as u32) & 0x3f == 0x2a {
            break 'outer acc;
        }
        i += 1;
    };
    (wide as u32) ^ ((wide >> 32) as u32) ^ ((wide >> 64) as u32) ^ ((wide >> 96) as u32) ^ tag ^ i
}
