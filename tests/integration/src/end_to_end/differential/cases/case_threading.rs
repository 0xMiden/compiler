// Jump-threading and tail-duplication sources: a condition tested twice
// with code between the tests, `match` arms whose different guards all
// jump to one shared block, a `loop { if x { .. continue } if y { break }
// .. }` body with duplicated tails, and a two-exit loop whose exits carry
// two different constants (the if-to-select canonicalization target). The
// exit tag of the last loop sits in the top nibble.

// A condition tested twice (LLVM threads the second test).
#[inline(never)]
fn tested_twice(a: u32, b: u32) -> u32 {
    let c = a > b;
    let mut p = a ^ 0x5555;
    let mut q = b;
    if c {
        p = p.wrapping_add(1);
    }
    q = q.wrapping_mul(3) ^ p;
    if c {
        p ^= q;
    } else {
        q = q.rotate_left(5);
    }
    p.wrapping_add(q)
}

// Match arms with different guards jumping to one shared block.
#[inline(never)]
fn shared_target(a: u32, b: u32) -> u32 {
    let mut x = a;
    let mut hits = 0u32;
    let n = (b % 31).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        x = x.wrapping_mul(0x0100_0193) ^ i;
        let go = match x % 4 {
            0 => a & 1 == 1,
            1 => b & 2 == 2,
            2 => x > b,
            _ => i & 1 == 0,
        };
        if go {
            // Shared block reached from four arms under four conditions.
            hits = hits.wrapping_add(1);
            x = x.rotate_left(3);
        } else {
            x = x.wrapping_sub(hits);
        }
        i = i.wrapping_add(1);
    }
    x ^ hits.wrapping_mul(0x0101_0101)
}

// `loop { if x { .. continue } if y { break } .. }` with duplicated tails.
#[inline(never)]
fn goto_like(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut acc = 0u32;
    let mut i = 0u32;
    let n = (b % 71).wrapping_add(1);
    loop {
        x = x.wrapping_mul(0x9e37_79b9) ^ i;
        i = i.wrapping_add(1);
        if x & 7 == 0 {
            acc = acc.wrapping_add(x >> 3);
            if i >= n {
                break; // duplicated exit test
            }
            continue;
        }
        if x & 0x70 == 0x30 {
            acc ^= x;
            break;
        }
        if x & 7 == 3 {
            acc = acc.rotate_left(1);
        } else {
            acc = acc.wrapping_sub(x & 0xff);
        }
        if i >= n {
            break;
        }
    }
    acc ^ x ^ i
}

// Exactly two exits with two different constants.
#[inline(never)]
fn two_consts(a: u32, b: u32) -> u32 {
    let mut x = a | 1;
    let mut i = 0u32;
    let n = b % 89; // zero-trip-capable
    let tag = loop {
        if i >= n {
            break 1u32;
        }
        x = x.wrapping_mul(0x0808_8405).wrapping_add(i);
        if x & 0xf00 == 0x300 {
            break 2u32;
        }
        i = i.wrapping_add(1);
    };
    (tag << 28) | ((x ^ i) & 0x0fff_ffff)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let low = tested_twice(input1, input2)
        ^ shared_target(input2, input1).rotate_left(7)
        ^ goto_like(input1, input2).rotate_left(14);
    let t = two_consts(input2, input1);
    (t & 0xf000_0000) | ((t ^ low) & 0x0fff_ffff)
}
