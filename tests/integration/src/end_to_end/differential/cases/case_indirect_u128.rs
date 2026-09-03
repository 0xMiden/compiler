// div128_guards x indirect_wide (campaign 14): fn pointers taking TWO u128
// parameters (scalarized to four i64 = eight felts) and returning a u128
// (return-area pointer), dispatched in a loop whose carried state is three
// u128s; the callees run u128 / i128 division and remainder with divisors
// reaching 0 / -1 / MIN through `checked_*` forms, and 128-bit shifts by
// runtime counts, each result feeding the next trip's dispatch. Two plain
// u128 locals is the boundary: a THREE-u128 signature (twelve argument
// felts), or two u128s with one argument computed in place, hits the
// `indirect_spill` class in the same loop (17-felt `arith.shl`
// `NoSolution` while the limbs are loaded for the dispatch; probes
// deleted).
type Duo = fn(u128, u128) -> u128;

#[inline(never)]
fn div_u(a: u128, b: u128) -> u128 {
    let c = a.rotate_left(64) ^ b;
    let q = a.checked_div(b).unwrap_or(a ^ c);
    let r = c.checked_rem(b | 1).unwrap_or(0);
    q ^ r.rotate_left((a as u32) & 127)
}

#[inline(never)]
fn div_s(a: u128, b: u128) -> u128 {
    let c = b.rotate_right(37) ^ a;
    let sa = a as i128;
    let sb = b as i128;
    let q = sa.checked_div(sb).unwrap_or(sa.wrapping_neg());
    let r = sa.checked_rem(sb).unwrap_or(c as i128);
    (q.wrapping_sub(r) as u128) ^ (c >> ((b as u32) & 127))
}

#[inline(never)]
fn shifts(a: u128, b: u128) -> u128 {
    let c = a.wrapping_add(b);
    let k = (b as u32) & 127;
    (a << k) ^ (a >> (127 - k)) ^ ((c as i128) >> ((a as u32) & 127)) as u128 ^ b.wrapping_mul(c | 1)
}

static DUOS: [Duo; 3] = [div_u, div_s, shifts];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut a = ((x as u128) << 64) | y as u128;
    // Divisors that reach 0, -1 (all ones) and i128::MIN from the inputs.
    let mut b = match input2 % 5 {
        0 => 0,
        1 => u128::MAX,
        2 => 1u128 << 127,
        3 => (y as u128) << ((input1 & 63) as u32),
        _ => x as u128 | 1,
    };
    let mut c = (y as u128).wrapping_mul(x as u128) ^ (1u128 << 127);
    let n = input1 % 5 + 1;
    let mut i = 0u32;
    while i < n {
        // `ac` is used after the dispatch too, so it stays a local and the
        // dispatch takes plain locals (an argument computed in place is the
        // `indirect_spill_args` panic).
        let ac = a ^ c;
        let f = DUOS[((a as u32).wrapping_add(i) % 3) as usize];
        let r = f(ac, b);
        c = ac.rotate_left(9) ^ r;
        a = r.wrapping_add(b);
        b = if i & 1 == 0 { b } else { r >> 64 };
        i += 1;
    }
    let z = a ^ (a >> 64) ^ (c >> 32) ^ b;
    (z as u32) ^ ((z >> 32) as u32) ^ ((z >> 64) as u32) ^ ((z >> 96) as u32)
}
