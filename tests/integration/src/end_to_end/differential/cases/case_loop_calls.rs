// Calls inside loops with loop-carried values across them: a
// zero-trip-capable outer `while i < input2 % 41` (LLVM keeps the guard and
// a bypass edge) carries four u64 and two u32 values across two calls per
// trip of a pinned `#[inline(never)]` helper whose three u64 arguments are
// reused after the call (Copy-constrained exec operands); a `match` inside
// the loop calls the helper in one arm only, `continue`s in another and
// lets a third arm decide an early `return` from a call result; a
// bottom-test inner loop calls a second helper on the carried state; a call
// result feeds the outer loop's break condition. Exit tag = top nibble.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn mixer(a: u64, b: u64, c: u64) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    (a ^ p).wrapping_mul(0x2545_f491_4f6c_dd1d) ^ b.rotate_left(13) ^ c.wrapping_add(a)
}

#[inline(never)]
fn twist(a: u64, k: u32) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    a.rotate_left((k ^ p) & 63) ^ (k as u64).wrapping_mul(0x9e37_79b9)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = (input1 | 1) as u64;
    let mut b = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut c = a.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ b;
    let mut d = b.rotate_left(29) ^ a;
    let mut s = input1 ^ input2;
    let mut tag = 1u32;
    let n = input2 % 41;
    let mut i = 0u32;
    while i < n {
        let t = mixer(a, b, c);
        // a, b, c reused after the call.
        a = a.rotate_left(1) ^ t;
        b = b.wrapping_add(c ^ t);
        match s & 3 {
            0 => {
                let u = mixer(b, d, a);
                c = c ^ u ^ d.rotate_left(3);
                d = d.wrapping_sub(u);
            }
            1 => {
                s = s.rotate_left(5) ^ (a as u32);
                i = i.wrapping_add(1);
                continue;
            }
            2 => {
                let v = twist(c ^ d, s);
                if v & 0xff == 0x3c {
                    return (4 << 28) | (((v as u32) ^ ((v >> 32) as u32)) & 0x0fff_ffff);
                }
                c = v;
            }
            _ => {
                d = d.rotate_left(7) ^ (s as u64);
            }
        }
        let m = (s % 5).wrapping_add(1);
        let mut j = 0u32;
        while j < m {
            d = twist(d, j ^ s);
            c = c.wrapping_add(d.rotate_left(j & 63));
            j = j.wrapping_add(1);
        }
        s = s.wrapping_mul(0x0101_0101) ^ (c as u32);
        if mixer(c, d, b) & 0x1ff == 0x155 {
            tag = 2;
            break;
        }
        i = i.wrapping_add(1);
    }
    let r = a
        ^ b.rotate_left(11)
        ^ c.rotate_left(22)
        ^ d.rotate_left(33)
        ^ (s as u64)
        ^ ((i as u64) << 40);
    (tag << 28) | (((r as u32) ^ ((r >> 32) as u32)) & 0x0fff_ffff)
}
