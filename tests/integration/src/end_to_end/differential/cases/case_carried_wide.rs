// carried_sets x wide x calls (campaign 14): three u128 values (twelve
// felts) carried by a bottom-test loop across a pinned direct call per
// trip (the call takes two of them by value — four i64 limbs — and returns
// a u128 through a return area), a full rotation of the three carried
// values every trip, and a call result deciding the loop's `break`; the
// values are consumed after the loop with all four limbs.
#[inline(never)]
fn step(a: u128, b: u128, k: u32) -> u128 {
    a.wrapping_mul(b | 1).rotate_left(k & 127) ^ (b >> (k & 63))
}

#[inline(never)]
fn done(v: u128) -> bool {
    (v >> 100) as u32 & 0xf == 0x9
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut a: u128 = ((x as u128) << 64) | y as u128;
    let mut b: u128 = ((y as u128) << 64) | x as u128;
    let mut c: u128 = a ^ b.rotate_left(33);
    let n = input1 % 9 + 1;
    let mut i = 0u32;
    let mut exit = 0u32;
    loop {
        let r = step(a, b, input2.wrapping_add(i));
        // Rotate the carried set: (a, b, c) <- (b, c ^ r, a + r).
        let na = b;
        let nb = c ^ r;
        let nc = a.wrapping_add(r);
        a = na;
        b = nb;
        c = nc;
        i += 1;
        if done(r) {
            exit = 1;
            break;
        }
        if i >= n {
            break;
        }
    }
    let z = a ^ b.rotate_left(7) ^ c.rotate_right(11);
    (z as u32) ^ ((z >> 32) as u32) ^ ((z >> 64) as u32) ^ ((z >> 96) as u32) ^ (exit << 31) ^ i
}
