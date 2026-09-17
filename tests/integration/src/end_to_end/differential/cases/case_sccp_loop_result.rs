// A loop with three `break`s that all fall through to one tail expression,
// which is also the function's result. LLVM tail-duplicates that expression
// into a `return` inside the loop, so the loop's own `end` becomes unreachable
// and has to be typed to satisfy the function signature: the wasm carries
// `loop (result i32)`. That is the only construct in this corpus for which the
// wasm frontend builds a block WITH AN ARGUMENT, so it is the only shape whose
// merge could reach SCCP as a block-argument lattice at all.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut acc = 0x243f_6a88u32;
    let mut i = 0u32;
    let n = (input2 % 53).wrapping_add(1);
    loop {
        x = x.wrapping_mul(0x85eb_ca6b) ^ i;
        i = i.wrapping_add(1);
        if x & 15 == 0 {
            acc = acc.wrapping_add(x >> 4);
            if i >= n {
                break;
            }
            continue;
        }
        if x & 0xf0 == 0x50 {
            acc = acc.rotate_right(3);
            break;
        }
        if x & 3 == 2 {
            acc = acc.rotate_left(5);
        } else {
            acc = acc.wrapping_sub(x & 0x1ff);
        }
        if i >= n {
            break;
        }
    }
    acc ^ x.wrapping_add(i)
}
