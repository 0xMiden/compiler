// C10 spills x C13 memory copies (campaign 14): ten u64 values (twenty
// felts) produced by a pinned `#[inline(never)]` helper before three
// runtime-length, misaligned `memory.copy`s (static -> stack, stack ->
// stack, disjoint in-buffer `copy_within`; the destination never equals the
// source) and a runtime-length `fill`, stay live across all of them (the
// memcpy/memset intrinsic execs are scheduled under spilled state), and are
// consumed afterwards by a right-leaning 12-leaf u64 tree over the copied
// words (24 felts in one block) and a walk over both buffers.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);
static LENS: [u8; 16] = [1, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 2, 6, 12];
static SRC: [u8; 48] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
];

#[inline(never)]
fn pin(a: u64, k: u64) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    (a ^ p).wrapping_mul(k | 1)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = pin(m, 0x9e37_79b9_7f4a_7c15);
    let v1 = pin(n, 0xbf58_476d_1ce4_e5b9);
    let v2 = pin(v0.rotate_left(17), n);
    let v3 = pin(v1.rotate_left(23), m);
    let v4 = pin(v2.wrapping_add(v0), 0xa076_1d64_78bd_642f);
    let v5 = pin(v3.wrapping_sub(v1), 0xe703_7ed1_a0b4_28db);
    let v6 = pin(v4.rotate_left(9), v2);
    let v7 = pin(v5.rotate_left(13), v3);
    let v8 = pin(v6.wrapping_add(v4), n.rotate_left(3));
    let v9 = pin(v7.wrapping_sub(v5), m.rotate_left(5));

    let len = LENS[(input1 & 15) as usize] as usize;
    let len2 = LENS[((input1 >> 4) & 15) as usize] as usize;
    let so = (input2 & 3) as usize;
    let dof = ((input2 >> 2) & 3) as usize;
    let mut a = [0u8; 80];
    let mut b = [0u8; 96];
    let mut k = 0usize;
    while k < 80 {
        a[k] = (input1 >> (k & 7)) as u8 ^ (k as u8).wrapping_mul(3);
        k += 1;
    }
    k = 0;
    while k < 96 {
        b[k] = (input2 >> (k & 7)) as u8 ^ (k as u8).wrapping_mul(5);
        k += 1;
    }
    // Static -> stack, stack -> stack, disjoint in-buffer copy, fill.
    a[dof..dof + len].copy_from_slice(&SRC[so..so + len]);
    b[40 + dof..40 + dof + len].copy_from_slice(&a[so..so + len]);
    a.copy_within(so..so + len, 44 + dof);
    b[8 + so..8 + so + len2].fill((input1 ^ 0xa5) as u8);

    let e0 = u64::from_le_bytes(a[dof..dof + 8].try_into().unwrap());
    let e1 = u64::from_le_bytes(b[40 + dof..48 + dof].try_into().unwrap());
    let e2 = u64::from_le_bytes(a[44 + dof..52 + dof].try_into().unwrap());
    let e3 = u64::from_le_bytes(b[8 + so..16 + so].try_into().unwrap());
    // Right-leaning tree over the live u64s and the copied words.
    let t = v0
        ^ e0
        ^ (v1 ^ e1).wrapping_sub(
            (v2 ^ e2).rotate_left(
                ((v3 ^ e3
                    ^ (v4 ^ e0.rotate_left(8)).wrapping_sub(
                        (v5 ^ e1.rotate_left(16)).rotate_left(
                            ((v6 ^ e2.rotate_left(24)
                                ^ (v7 ^ e3.rotate_left(32)).wrapping_sub(
                                    (v8 ^ e0.rotate_left(40)).rotate_left(
                                        ((v9 ^ e1.rotate_left(48)
                                            ^ (v0 ^ e2.rotate_left(56))
                                                .wrapping_sub(v1 ^ e3.rotate_left(1)))
                                            as u32)
                                            & 31,
                                    ),
                                )) as u32)
                                & 31,
                        ),
                    )) as u32)
                    & 31,
            ),
        );
    let mut acc = (t as u32) ^ ((t >> 32) as u32);
    k = 0;
    while k < 80 {
        acc = acc.rotate_left(3) ^ (a[k] as u32).wrapping_mul(k as u32 | 1);
        k += 1;
    }
    k = 0;
    while k < 96 {
        acc = acc.rotate_left(5) ^ (b[k] as u32).wrapping_mul(0x0101_0101);
        k += 1;
    }
    acc ^ (v2 as u32) ^ ((v5 >> 32) as u32) ^ (v8 as u32).rotate_left(7)
}
