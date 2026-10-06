// calls x everything (campaign 14): a zero-trip-capable state-machine loop
// (`while i < input2 % 17`, four states dispatched through a `br_table`,
// one `continue` arm, a call-decided early `return` and `break`) whose
// arms (a) call a pinned 16-felt helper (7 u64 + u32 + u128 return area)
// and store the u128 result into a `repr(C, packed)` frame array at a
// runtime index through a `&mut` array helper, (b) dispatch a
// runtime-indexed fn pointer with three u64 arguments, (c) run a
// runtime-length in-buffer `copy_within` over a `&mut` byte buffer in a
// helper (destination range always disjoint from the source), and (d) mix
// the accumulator in a helper; four u64 values stay live across the whole
// loop and every packed slot, buffer byte and carried value is folded by a
// final helper. Every arm body is an `#[inline(never)]` helper so that the
// entrypoint's loop holds only calls, locals and `& 3` masks: any constant
// shift count LLVM synthesizes in the entrypoint (even an `x * 7` lowered
// to `x << 3`) forms a CSE-merged band that crosses the four-arm join and
// hits the known F6 spill defect (`frontier.rs:123`). Exit tag = top nibble.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Slot {
    tag: u8,
    wide: u128,
    pad: u16,
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
fn store_slot(buf: &mut [Slot; 4], k: usize, v: u128, t: u8) -> u32 {
    buf[k & 3].wide = v;
    buf[k & 3].tag = t;
    let w = buf[(k + 1) & 3].wide;
    (w as u32) ^ ((w >> 64) as u32)
}

#[inline(never)]
fn fold_slot(acc: u64, s: u128, w: u32) -> u64 {
    acc ^ (s as u64) ^ ((s >> 64) as u64).rotate_left(21) ^ (w as u64)
}

type Op = fn(u64, u64, u64) -> u64;

#[inline(never)]
fn op_a(a: u64, b: u64, c: u64) -> u64 {
    a.wrapping_mul(b | 1) ^ c.rotate_left(13)
}

#[inline(never)]
fn op_b(a: u64, b: u64, c: u64) -> u64 {
    (a ^ b).wrapping_add(c.rotate_right(9))
}

static OPS: [Op; 2] = [op_a, op_b];

#[inline(never)]
fn pick(acc: u64) -> Op {
    OPS[((acc >> 3) & 1) as usize]
}

#[inline(never)]
fn next_state(r: u64) -> u32 {
    ((r >> 7) & 3) as u32
}

#[inline(never)]
fn copy_arm(bytes: &mut [u8; 64], i: u32, acc: u64) -> u64 {
    let len = (acc & 15) as usize + 1;
    let so = (i & 3) as usize;
    let dof = ((acc >> 4) & 3) as usize;
    bytes.copy_within(so..so + len, 32 + dof);
    let e = u64::from_le_bytes(bytes[32 + dof..40 + dof].try_into().unwrap());
    acc.rotate_left(25) ^ e
}

#[inline(never)]
fn mix_arm(acc: u64, i: u32) -> u64 {
    acc.wrapping_sub(acc.rotate_left(27)) ^ (i as u64)
}

#[inline(never)]
fn fill(bytes: &mut [u8; 64], input1: u32, input2: u32) -> u64 {
    let mut k = 0usize;
    while k < 64 {
        bytes[k] = (input1 >> (k & 7)) as u8
            ^ (k as u8).wrapping_mul(157)
            ^ input2.wrapping_mul(k as u32 + 1) as u8;
        k += 1;
    }
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc: u64 = y | 1;
    acc ^= y.rotate_left(5);
    acc = acc.wrapping_add(y.rotate_left(11));
    acc.wrapping_sub(y.rotate_left(17))
}

#[inline(never)]
fn fold_all(
    slots: &[Slot; 4],
    bytes: &[u8; 64],
    acc: u64,
    k1: u64,
    k2: u64,
    k3: u64,
    k4: u64,
) -> u32 {
    let mut z = acc ^ k1 ^ k2.rotate_left(3) ^ k3.rotate_left(7) ^ k4.rotate_left(9);
    z ^= acc.rotate_left(37);
    z = z.wrapping_add(acc.rotate_left(39));
    z = z.wrapping_sub(acc.rotate_left(41));
    let mut h = (z as u32) ^ ((z >> 32) as u32);
    let mut k = 0usize;
    while k < 4 {
        let s = slots[k];
        let (st, sw, sp) = (s.tag, s.wide, s.pad);
        h = h.rotate_left(3)
            ^ (sw as u32)
            ^ ((sw >> 32) as u32)
            ^ ((sw >> 64) as u32)
            ^ ((sw >> 96) as u32);
        h = h.wrapping_add(st as u32).wrapping_add(sp as u32);
        k += 1;
    }
    k = 0;
    while k < 64 {
        h = h.rotate_left(1) ^ (bytes[k] as u32).wrapping_mul(k as u32 | 1);
        k += 1;
    }
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let zero = Slot {
        tag: 0,
        wide: 0,
        pad: 0,
    };
    let mut slots = [zero; 4];
    let mut bytes = [0u8; 64];
    let mut acc = fill(&mut bytes, input1, input2);
    let k1 = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ acc;
    let k2 = k1.rotate_left(23) ^ 0xbf58_476d_1ce4_e5b9;
    let k3 = k1.wrapping_mul(0x94d0_49bb_1331_11eb) ^ k2;
    let k4 = k2.wrapping_add(k3.rotate_left(29)) | 1;
    let mut state = input1 & 3;
    let mut tag = 1u32;
    let n = input2 % 17;
    let mut i = 0u32;
    while i < n {
        match state {
            0 => {
                let s =
                    sret16(k1, k2, k3, k4, acc, k1 ^ acc, k2.wrapping_add(i as u64), input1 ^ i);
                let w = store_slot(&mut slots, i as usize, s, i as u8);
                acc = fold_slot(acc, s, w);
                state = (w & 3) ^ 1;
            }
            1 => {
                let f = pick(acc);
                let r = f(acc, k3, k4 ^ (i as u64));
                acc = acc.wrapping_add(r) ^ k3;
                state = next_state(r);
            }
            2 => {
                acc = copy_arm(&mut bytes, i, acc);
                if acc & 0xff == 0x3c {
                    tag = 2;
                    break;
                }
                state = 3;
            }
            _ => {
                acc = mix_arm(acc, i);
                state = (acc & 3) as u32;
                i = i.wrapping_add(1);
                continue;
            }
        }
        if acc & 0x1ff == 0x155 {
            return (3 << 28) | (((acc as u32) ^ state) & 0x0fff_ffff);
        }
        i = i.wrapping_add(1);
    }
    let h = fold_all(&slots, &bytes, acc, k1, k2, k3, k4);
    (tag << 28) | ((h ^ state) & 0x0fff_ffff)
}
