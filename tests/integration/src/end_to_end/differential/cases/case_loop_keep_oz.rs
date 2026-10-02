// Small constant-trip loops that LLVM fully unrolls at O2 but keeps as
// loops at -Oz (`--optimize=size-min`): a 4-trip array fill, a 4-trip scan
// with an early `break`, a 3x4 nested counted-loop pair carrying a u64
// accumulator with an in-body conditional, and an 8-trip loop with
// `continue` edges. The loop-carried state travels through wasm locals, so
// each loop reaches cfg-to-scf and the loop-header spill placement with
// shapes the O2 corpus never produced (its versions were unrolled away).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut arr = [0u32; 4];
    let mut i = 0;
    while i < 4 {
        arr[i] = input1.wrapping_mul(i as u32 + 1) ^ input2.rotate_left(i as u32);
        i += 1;
    }
    // Scan with an early exit.
    let mut found = 4u32;
    let mut k = 0;
    while k < 4 {
        if arr[k] & 7 == 3 {
            found = k as u32;
            break;
        }
        k += 1;
    }
    // Nested counted loops with a u64 accumulator and an in-body branch.
    let mut acc: u64 = (input1 as u64) | 1;
    let mut a = 0;
    while a < 3 {
        let mut b = 0;
        while b < 4 {
            acc = acc.wrapping_mul(0x9e37_79b9).wrapping_add((arr[b] as u64) ^ (a as u64));
            if acc & 1 == 0 {
                acc ^= input2 as u64;
            }
            b += 1;
        }
        a += 1;
    }
    // Continue edges.
    let mut s = 0u32;
    let mut t = 0;
    while t < 8 {
        t += 1;
        if (input2 >> t) & 1 == 0 {
            continue;
        }
        s = s.wrapping_add(arr[t & 3].rotate_right(t as u32));
    }
    found ^ (acc as u32) ^ ((acc >> 32) as u32) ^ s
}
