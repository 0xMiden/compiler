// switch_forms x calls (campaign 14): a dense 24-arm `match` (one
// `br_table`) on a loop-carried selector inside a loop, whose arms call
// helpers of every arity shape — zero-arg, zero-result, a 16-felt
// eight-u64 signature, a `(u64, u64)` return area, a fn-pointer dispatch —
// or `continue` / `break` / `return`, with two u64 and the selector
// carried across the loop; a second holey `match` with a hot default
// re-selects the next arm from a call result.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn zero() -> u64 {
    PIN.fetch_add(0, Ordering::Relaxed) as u64 ^ 0x1234_5678
}

#[inline(never)]
fn sink(v: u64) {
    PIN.fetch_add((v as u32) & 0, Ordering::Relaxed);
}

#[inline(never)]
fn eight(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64) -> u64 {
    a.wrapping_add(b).wrapping_mul(c | 1) ^ d.rotate_left(7) ^ e.wrapping_sub(f) ^ g.wrapping_mul(h | 1)
}

#[inline(never)]
fn pair(v: u64, k: u32) -> (u64, u64) {
    (v.rotate_left(k & 63), v.wrapping_mul(k as u64 | 1))
}

type Op = fn(u64, u64) -> u64;

#[inline(never)]
fn op_a(a: u64, b: u64) -> u64 {
    a.wrapping_mul(b | 1)
}

#[inline(never)]
fn op_b(a: u64, b: u64) -> u64 {
    a.rotate_left(11) ^ b
}

static OPS: [Op; 2] = [op_a, op_b];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let mut b = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut sel = input1 % 24;
    let n = input2 % 13 + 1;
    let mut i = 0u32;
    let mut tag = 0u32;
    while i < n {
        i += 1;
        let r = match sel {
            0 => zero(),
            1 => {
                sink(a);
                b
            }
            2 => eight(a, b, a ^ b, a.wrapping_add(b), a >> 3, b << 5, a ^ 1, b | 2),
            3 => {
                let (p, q) = pair(a, input2.wrapping_add(i));
                p ^ q
            }
            4 => OPS[(a & 1) as usize](a, b),
            5 => {
                tag |= 1;
                sel = (sel + 7) % 24;
                continue;
            }
            6 => {
                tag |= 2;
                break;
            }
            7 => {
                if b & 0xf == 0xa {
                    tag |= 4;
                    return tag ^ (a as u32) ^ 0x4000_0000;
                }
                a ^ b
            }
            8 => zero() ^ a,
            9 => eight(b, a, 1, 2, 3, 4, 5, a),
            10 => {
                let (p, _) = pair(b, i);
                p
            }
            11 => OPS[((b >> 5) & 1) as usize](b, a),
            12 => a.rotate_left(3),
            13 => b.rotate_right(5),
            14 => a.wrapping_add(b),
            15 => a.wrapping_sub(b),
            16 => a ^ b.rotate_left(1),
            17 => (a | b).wrapping_mul(3),
            18 => a & b,
            19 => !a ^ b,
            20 => a >> 7,
            21 => b << 9,
            22 => a.swap_bytes(),
            _ => b.reverse_bits(),
        };
        a = b ^ r;
        b = r.rotate_left(13).wrapping_add(i as u64);
        // Holey re-selection with a hot default.
        sel = match (r as u32) % 41 {
            0 => 5,
            3 => 6,
            7 => 7,
            11 => 2,
            13 => 4,
            17 => 9,
            19 => 11,
            23 => 3,
            _ => ((r >> 8) % 24) as u32,
        };
    }
    (a as u32) ^ ((a >> 32) as u32) ^ (b as u32).rotate_left(7) ^ ((b >> 32) as u32) ^ (tag << 28) ^ sel ^ i
}
