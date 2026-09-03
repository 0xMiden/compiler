// Caller pressure meeting callee pressure: the caller keeps six u64 values
// (twelve felts) live across two pinned `#[inline(never)]` calls whose
// callee itself evaluates a right-leaning non-reassociable tree over ten
// u64 leaves (twenty felts, single-block spills inside the callee's own
// frame) and returns a `(u64, u64)` pair through a return area, then a
// second callee with a 16-felt signature (7 u64 + 2 u32) that spills
// internally as well. Both callees' spill slots and the caller's live
// state must not alias across the frame boundary.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn tree(a: u64, b: u64) -> (u64, u64) {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    let a = a ^ p;
    let r = a.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(1)
        ^ b.wrapping_add(0x0)
        ^ (a.wrapping_mul(0x9e3779b97f4a7c17).rotate_left(8) ^ b.wrapping_add(0x123456789))
            .wrapping_sub(
                (a.wrapping_mul(0x9e3779b97f4a7c19).rotate_left(15) ^ b.wrapping_add(0x2468acf12))
                    .rotate_left(
                        ((a.wrapping_mul(0x9e3779b97f4a7c1b).rotate_left(22)
                            ^ b.wrapping_add(0x369d0369b)
                            ^ (a.wrapping_mul(0x9e3779b97f4a7c1d).rotate_left(29)
                                ^ b.wrapping_add(0x48d159e24))
                            .wrapping_sub(
                                (a.wrapping_mul(0x9e3779b97f4a7c1f).rotate_left(36)
                                    ^ b.wrapping_add(0x5b05b05ad))
                                .rotate_left(
                                    ((a.wrapping_mul(0x9e3779b97f4a7c21).rotate_left(43)
                                        ^ b.wrapping_add(0x6d3a06d36)
                                        ^ (a.wrapping_mul(0x9e3779b97f4a7c23).rotate_left(50)
                                            ^ b.wrapping_add(0x7f6e5d4bf))
                                        .wrapping_sub(
                                            (a.wrapping_mul(0x9e3779b97f4a7c25).rotate_left(57)
                                                ^ b.wrapping_add(0x91a2b3c48))
                                            .rotate_left(
                                                ((a.wrapping_mul(0x9e3779b97f4a7c27).rotate_left(2)
                                                    ^ b.wrapping_add(0xa3d70a3d1))
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
    (r, r.rotate_left(17) ^ a)
}

#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn wide(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u32, i: u32) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    // A second in-callee spill: every parameter is used twice.
    let x = (a ^ p).wrapping_mul(b | 1)
        ^ c.rotate_left(h & 63)
        ^ d.wrapping_add(e)
        ^ f.wrapping_sub(g).rotate_left(i & 63);
    let y = a.rotate_left(3)
        ^ b.rotate_left(5)
        ^ c.rotate_left(7)
        ^ d.rotate_left(9)
        ^ e.rotate_left(11)
        ^ f.rotate_left(13)
        ^ g.rotate_left(15);
    x.wrapping_mul(y | 1) ^ (h as u64) ^ ((i as u64) << 32)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n;
    let v1 = n.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ n.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let (t0, t1) = tree(v0, v1);
    let (u0, u1) = tree(v2 ^ t0, v3);
    let w = wide(v4, v5, t0, t1, u0, u1, v0 ^ v1, input1, input2);
    let r = v0
        ^ v1.rotate_left(1)
        ^ v2.rotate_left(2)
        ^ v3.rotate_left(4)
        ^ v4.rotate_left(6)
        ^ v5.rotate_left(8)
        ^ t0
        ^ t1.rotate_left(10)
        ^ u0.rotate_left(12)
        ^ u1
        ^ w.rotate_left(14);
    (r as u32) ^ ((r >> 32) as u32)
}
