// At-limit and near-limit flat call signatures of MIXED widths, plus
// return-area (sret) helpers whose hidden result pointer counts against the
// same 16-felt window: `mix16` (7 u64 + 2 u32 = 16 felts), `narrow16` (14
// u32 + 1 u64 = 16), `sret16` (7 u64 + 1 u32 + the u128 return-area pointer
// = 16), `quad15` (3 u128 scalarized to i64 pairs + u64 + u32 = 15) and
// `pair15` (3 u128 + u64 + a (u64, u64) return-area pointer = 15). Four u64
// values stay live across every call and every result is consumed after the
// last call, so the caller schedules full-window argument lists under
// spilled state and reloads the return areas afterwards. The `fetch_add(0)`
// side effect pins the calls in place.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn mix16(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u32, i: u32) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    (a ^ p)
        .wrapping_mul(b | 1)
        .wrapping_add(c.rotate_left(h & 63))
        .wrapping_sub(d ^ e.rotate_left(i & 63))
        ^ f.wrapping_add(g)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn narrow16(
    a: u32,
    b: u32,
    c: u32,
    d: u32,
    e: u32,
    f: u32,
    g: u32,
    h: u32,
    i: u32,
    j: u32,
    k: u32,
    l: u32,
    m: u32,
    n: u32,
    w: u64,
) -> u32 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    let lo = a.wrapping_add(b.rotate_left(1))
        ^ c.wrapping_mul(d | 1)
        ^ e.rotate_left(2)
        ^ f.wrapping_add(g)
        ^ h.rotate_left(3)
        ^ i.wrapping_mul(j | 1)
        ^ k.rotate_left(4)
        ^ l.wrapping_add(m)
        ^ n.rotate_left(5)
        ^ p;
    lo ^ (w as u32) ^ ((w >> 32) as u32)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn sret16(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u32) -> u128 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    let lo = (a ^ p).wrapping_mul(b | 1) ^ c.rotate_left(h & 63);
    let hi = d.wrapping_add(e) ^ f.wrapping_sub(g).rotate_left(h >> 26);
    ((hi as u128) << 64) | lo as u128
}

#[inline(never)]
fn quad15(p: u128, q: u128, r: u128, s: u64, t: u32) -> u64 {
    let m = p.wrapping_mul(q | 1).wrapping_add(r) ^ ((s as u128) << (t & 127));
    (m as u64) ^ ((m >> 64) as u64).rotate_left(t & 63)
}

#[inline(never)]
fn pair15(p: u128, q: u128, r: u128, s: u64) -> (u64, u64) {
    let m = p ^ q.rotate_left(17) ^ r.wrapping_add(s as u128);
    (m as u64, ((m >> 64) as u64) ^ s)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let k1 = ((input1 as u64) << 32) | input2 as u64;
    let k2 = k1.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    let k3 = k1.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ k2;
    let k4 = k2.wrapping_add(k3.rotate_left(29)) | 1;
    let m = mix16(k1, k2, k3, k4, k1 ^ 5, k2.wrapping_add(6), k3 ^ 7, input1, input2 ^ 9);
    let n = narrow16(
        input1,
        input2,
        input1 ^ 1,
        input2 ^ 2,
        input1.wrapping_add(3),
        input2 ^ 4,
        input1 ^ 5,
        input2.wrapping_add(6),
        input1 ^ 7,
        input2 ^ 8,
        input1.rotate_left(9),
        input2 ^ 10,
        input1 ^ 11,
        input2.rotate_left(12),
        k4,
    );
    let s = sret16(k4, k3, k2, k1, k4 ^ m, k3.wrapping_add(m), k2 ^ n as u64, input2);
    let p: u128 = ((k1 as u128) << 64) | k2 as u128;
    let q: u128 = ((k3 as u128) << 64) | k4 as u128;
    let r: u128 = s ^ ((m as u128) << 64);
    let qd = quad15(p, q, r, k1 ^ k4, input1.wrapping_add(input2));
    let (t0, t1) = pair15(q, r, p, k2 ^ qd);
    // k1..k4 are still live here: spilled around every call above.
    let z = k1
        ^ k2.rotate_left(3)
        ^ k3.rotate_left(5)
        ^ k4.rotate_left(7)
        ^ m
        ^ (s as u64)
        ^ ((s >> 64) as u64)
        ^ qd.rotate_left(11)
        ^ t0
        ^ t1.rotate_left(13);
    (z as u32) ^ ((z >> 32) as u32) ^ n
}
