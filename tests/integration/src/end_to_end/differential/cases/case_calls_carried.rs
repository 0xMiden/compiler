// call_live x carried_sets (campaign 14): nine loop-carried u32 values with
// a full rotation on every trip plus three loop-carried u64 values, two
// pinned `#[inline(never)]` calls per trip whose three u64 arguments are all
// used again after each call (Copy-constrained exec operands under twenty
// felts of carried state), a partial per-arm update of the rotated set
// driven by a carried selector, and a `<< 32` / `>> 32` count band shared
// by the code before, inside and after the loop. The rotated set and the
// u64 triple are folded after the loop, so a stale rotation slot or a
// clobbered argument changes the result.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn mixer(a: u64, b: u64, c: u64) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    (a ^ p).wrapping_mul(0x2545_f491_4f6c_dd1d) ^ b.rotate_left(13) ^ c.wrapping_add(a)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let (mut v0, mut v1, mut v2, mut v3, mut v4, mut v5, mut v6, mut v7, mut v8) = (
        input1,
        input2,
        input1 ^ 0x1111,
        input2 ^ 0x2222,
        input1.rotate_left(3),
        input2.rotate_left(5),
        input1.wrapping_add(input2),
        input1.wrapping_sub(input2),
        0x0f0f_0f0fu32,
    );
    let mut a = ((input1 as u64) << 32) | input2 as u64;
    let mut b = a.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    let mut c = a.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ b;
    let mut sel = input2;
    let n = (input1 % 23).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        // Full rotation of the nine carried u32s.
        let t = v0;
        v0 = v1;
        v1 = v2;
        v2 = v3;
        v3 = v4;
        v4 = v5;
        v5 = v6;
        v6 = v7;
        v7 = v8;
        v8 = t;
        let p = mixer(a, b, c);
        // a, b, c reused after the call.
        a = a ^ p ^ (v0 as u64);
        b = b.rotate_left(v1 & 63) ^ c;
        c = c.wrapping_add(p) ^ ((v2 as u64) << 32);
        match sel % 5 {
            0 => v0 = v0.wrapping_add(v4) ^ (a as u32),
            1 => v3 ^= v7 ^ ((b >> 32) as u32),
            2 => {
                v5 = v5.rotate_left(1);
                v8 = v8.wrapping_sub(v1);
            }
            3 => core::mem::swap(&mut v2, &mut v6),
            _ => {}
        }
        let q = mixer(c, a, b);
        a = a.wrapping_add(q);
        b ^= q.rotate_left(7) ^ (v3 as u64);
        c = c.rotate_left(9) ^ q ^ a;
        sel = sel.wrapping_mul(0x9e37_79b9) ^ v0 ^ (q as u32);
        i = i.wrapping_add(1);
    }
    let z = a ^ b.rotate_left(11) ^ c.rotate_left(22);
    let mut acc = sel ^ (z as u32) ^ ((z >> 32) as u32);
    let v = [v0, v1, v2, v3, v4, v5, v6, v7, v8];
    let mut k = 0usize;
    while k < 9 {
        acc = acc.rotate_left(3) ^ v[k];
        k += 1;
    }
    acc
}
