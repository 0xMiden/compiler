// W4: reverse-order erasure of a region-bearing op. `Rewriter::erase_op`'s
// `erase_tree` now walks each nested block's ops BACK TO FRONT, so a region op
// whose nested definitions feed later nested ops is torn down users-first.
//
// The producer is the cfg-to-scf exit-dispatch cascade: a loop with an EMPTY
// `continue` arm leaves a result column with no consumer, `WhileUnusedResult`
// drops the loop result and `IndexSwitchRemoveUnusedResults` rebuilds the
// `scf.index_switch` without that column -- both of which erase the ORIGINAL
// region op, regions and all. Each body here carries a nested `if` whose
// intermediate values are used by later ops inside the same nested block, so
// the erase order is exercised rather than just the outer op.

#[inline(never)]
fn step(x: u32, k: u32) -> u32 {
    x.wrapping_mul(0x0100_0193) ^ k.rotate_left(k & 31)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1;
    // Loop 1: empty `continue` arm plus a nested `if` that defines two values
    // used by a third op inside the same arm.
    let mut k = 0u32;
    while k < 8 {
        if input2 >> k & 1 != 0 {
            k += 1;
            continue;
        }
        if acc & 1 == 0 {
            let t = step(acc, k);
            let u = t.rotate_left(3);
            acc = t ^ u.wrapping_add(k);
        } else {
            let t = step(acc ^ k, k);
            let u = t.rotate_left(5);
            acc = t.wrapping_sub(u);
        }
        k += 1;
    }
    // Loop 2: the same cascade two levels deep.
    let mut j = 0u32;
    while j < 4 {
        let mut i = 0u32;
        while i < 4 {
            if input2 >> (i + 4) & 1 != 0 {
                i += 1;
                continue;
            }
            let t = step(acc, i ^ j);
            acc = t ^ t.rotate_left(7);
            i += 1;
        }
        j += 1;
    }
    // Loop 3: a dense `match` whose arms feed one carried value, with an empty
    // `continue` arm so the switch loses a result column.
    let mut n = 0u32;
    while n < 6 {
        match n {
            0 => acc = acc.rotate_left(1),
            1 => {
                n += 1;
                continue;
            }
            2 => acc = step(acc, 2),
            3 => acc ^= input2,
            4 => {
                let t = step(acc, 4);
                acc = t.wrapping_add(t.rotate_left(11));
            }
            _ => acc = acc.wrapping_sub(input1),
        }
        n += 1;
    }
    acc
}
