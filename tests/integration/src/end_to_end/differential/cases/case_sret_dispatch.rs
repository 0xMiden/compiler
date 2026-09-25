// ret_area x call_indirect (campaign 14): fn pointers whose signatures
// RETURN wide values by value — `fn(u64, u64) -> u128`, `fn(u32, u64) ->
// (u64, u64)` and `fn(u64) -> Option<u128>` — dispatched from
// runtime-indexed tables, so the hidden return-area pointer travels as the
// first argument of `hir.exec_indirect` and the callee writes the result
// through it; dispatched in a loop with the u128 result feeding the next
// trip's table index and arguments, under low live pressure.
type Wide = fn(u64, u64) -> u128;
type Pair = fn(u32, u64) -> (u64, u64);
type Opt = fn(u64) -> Option<u128>;

#[inline(never)]
fn mul_wide(a: u64, b: u64) -> u128 {
    (a as u128).wrapping_mul(b as u128) ^ ((a as u128) << 64)
}

#[inline(never)]
fn cat(a: u64, b: u64) -> u128 {
    ((a as u128) << 64 | b as u128).rotate_left(17)
}

#[inline(never)]
fn split(k: u32, v: u64) -> (u64, u64) {
    (v.rotate_left(k & 63), v ^ (k as u64) << 32)
}

#[inline(never)]
fn swap(k: u32, v: u64) -> (u64, u64) {
    (v >> (k & 31), v.wrapping_mul(k as u64 | 1))
}

#[inline(never)]
fn some_sq(v: u64) -> Option<u128> {
    if v & 0xff == 0x7e { None } else { Some((v as u128) * (v as u128)) }
}

#[inline(never)]
fn some_neg(v: u64) -> Option<u128> {
    if v >> 60 == 3 { None } else { Some((!(v as u128)).wrapping_add(v as u128)) }
}

static WIDES: [Wide; 2] = [mul_wide, cat];
static PAIRS: [Pair; 2] = [split, swap];
static OPTS: [Opt; 2] = [some_sq, some_neg];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let mut b = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut sel = (input1 & 1) as usize;
    let mut none = 0u32;
    let n = input2 % 6 + 1;
    let mut i = 0u32;
    while i < n {
        let w = WIDES[sel](a, b);
        let (p, q) = PAIRS[((w >> 64) as usize) & 1](input1.wrapping_add(i), w as u64);
        match OPTS[(p as usize) & 1](q ^ p) {
            Some(s) => {
                a = (s as u64) ^ p;
                b = ((s >> 64) as u64).wrapping_add(q);
            }
            None => {
                none += 1;
                a = a.rotate_left(7) ^ q;
                b ^= p;
            }
        }
        sel = ((w >> 3) as usize) & 1;
        i += 1;
    }
    (a as u32) ^ ((a >> 32) as u32) ^ (b as u32).rotate_left(5) ^ ((b >> 32) as u32) ^ (none << 28) ^ (sel as u32)
}
