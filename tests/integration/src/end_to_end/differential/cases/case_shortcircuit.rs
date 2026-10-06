// Short-circuit boolean lattices with SIDE EFFECTS in the operands: every
// condition is a `#[inline(never)]` probe that adds its own weight to an
// atomic counter, so the counter read at the end records exactly which
// operands were evaluated (the short-circuit order and the branch
// polarity). Shapes: a six-operand `&&` chain, a six-operand `||` chain,
// the `a && (b || c) && !d` diamond, a mixed lattice materialized as a
// bool value, and a lattice used as a loop exit. The counter is reset
// before returning (the native cdylib is reused across inputs).
use core::sync::atomic::{AtomicU32, Ordering};

static COUNT: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn probe(weight: u32, v: u32, mask: u32) -> bool {
    COUNT.fetch_add(weight, Ordering::Relaxed);
    v & mask != 0
}

#[inline(never)]
fn and_chain(a: u32, b: u32) -> u32 {
    if probe(1, a, 1)
        && probe(2, b, 1)
        && probe(4, a, 2)
        && probe(8, b, 2)
        && probe(16, a, 4)
        && probe(32, b, 4)
    {
        a.wrapping_add(b)
    } else {
        a ^ b
    }
}

#[inline(never)]
fn or_chain(a: u32, b: u32) -> u32 {
    if probe(1 << 6, a, 8)
        || probe(1 << 7, b, 8)
        || probe(1 << 8, a, 16)
        || probe(1 << 9, b, 16)
        || probe(1 << 10, a, 32)
        || probe(1 << 11, b, 32)
    {
        a.rotate_left(3)
    } else {
        b.rotate_right(3)
    }
}

#[inline(never)]
fn diamond(a: u32, b: u32) -> u32 {
    if probe(1 << 12, a, 64)
        && (probe(1 << 13, b, 64) || probe(1 << 14, a, 128))
        && !probe(1 << 15, b, 128)
    {
        a.wrapping_mul(3)
    } else if probe(1 << 16, a, 256) || !(probe(1 << 17, b, 256) && probe(1 << 18, a, 512)) {
        b.wrapping_mul(5)
    } else {
        a ^ b.rotate_left(9)
    }
}

#[inline(never)]
fn materialized(a: u32, b: u32) -> u32 {
    let p = (probe(1 << 19, a, 1024) || probe(1 << 20, b, 1024))
        && (probe(1 << 21, a, 2048) || probe(1 << 22, b, 2048))
        && !(probe(1 << 23, a, 4096) && probe(1 << 24, b, 4096));
    let q = probe(1 << 25, a, 8192) != probe(1 << 26, b, 8192);
    (p as u32) | ((q as u32) << 1) | ((p && q) as u32) << 2 | ((p || q) as u32) << 3
}

#[inline(never)]
fn exit_lattice(a: u32, b: u32) -> u32 {
    let mut x = a;
    let mut i = 0u32;
    // Loop exit decided by a lattice with side effects; bounded by `i`.
    while i < 6
        && (probe(1 << 27, x, 1) || probe(1 << 28, b, 1 << (i & 31)))
        && !probe(1 << 29, x, 2)
    {
        x = x.wrapping_mul(0x9e37_79b9).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    x ^ i
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let r = and_chain(input1, input2)
        ^ or_chain(input2, input1).rotate_left(7)
        ^ diamond(input1, input2).rotate_left(14)
        ^ materialized(input2, input1).rotate_left(21)
        ^ exit_lattice(input1, input2).rotate_left(28);
    let evaluated = COUNT.swap(0, Ordering::Relaxed);
    r ^ evaluated.wrapping_mul(0x0101_0101)
}
