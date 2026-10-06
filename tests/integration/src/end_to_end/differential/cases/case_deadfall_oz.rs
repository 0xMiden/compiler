// A loop whose only exits are FIVE in-loop `return`s (plus a `break` to the
// enclosing loop), nested inside a kept outer loop, with four masked rotate
// count bands crossing both. At `--optimize=size-min` this is the
// dead-fallthrough `loop (result i32)` frame LLVM's end-of-function fixup
// types when a loop never falls through, and it is the corpus's only producer
// of the `SimplifyPassthroughCondBr` cf canonicalization (ten rewrites here,
// paired with twelve `SplitCriticalEdges`) -- see the test's doc comment.
// Exit tags in the top nibble mark which return site ran.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = ((input1 | 1) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    acc ^= ((input2 | 2) as u64).rotate_left(3);
    acc = acc.wrapping_add(acc.rotate_left(9));
    acc ^= acc.rotate_left(17);
    acc = acc.wrapping_sub(acc.rotate_left(23));
    let outer = (input2 % 61) + 2;
    let mut o: u32 = 0;
    while o < outer {
        let mut k: u32 = 0;
        loop {
            acc = acc.rotate_left(3) ^ (k as u64);
            acc = acc.wrapping_add(acc.rotate_left(9));
            acc ^= acc.rotate_left(17);
            acc = acc.wrapping_sub(acc.rotate_left(23));
            if (acc & 0x00ff_0000) == 0x0042_0000 {
                return (1 << 28) | ((acc as u32) & 0x0fff_ffff);
            }
            if ((acc >> 40) & 0xff) == 0x37 {
                return (2 << 28) | ((acc.rotate_left(3) as u32) & 0x0fff_ffff);
            }
            if (acc & 0x1f) == 9 && k > 2 {
                return (3 << 28) | ((acc.rotate_left(9) as u32) & 0x0fff_ffff);
            }
            if k > 30 {
                return (4 << 28) | ((acc.rotate_left(17) as u32) & 0x0fff_ffff);
            }
            if ((acc ^ (o as u64)) % 53) == 7 {
                break;
            }
            if (acc & 0x3ff) == 0x155 {
                return (5 << 28) | ((acc.rotate_left(23) as u32) & 0x0fff_ffff);
            }
            k = k.wrapping_add(1);
        }
        o = o.wrapping_add(1);
    }
    (6 << 28) | (((acc ^ acc.rotate_left(23)) as u32) & 0x0fff_ffff)
}
