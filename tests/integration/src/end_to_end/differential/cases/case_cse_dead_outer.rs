// W4 second shape: a region op whose nested ops use values defined in the same
// block BEFORE the region op. `erase_tree` must drop only the nested uses, not
// the outer definitions -- if it walked the outer block too, the captured
// values would go with it.
//
// Every loop below captures `base` and `salt`, both defined before the loop
// and both still used after it, so an over-eager erase shows up as a wrong
// value rather than a panic. The empty `continue` arms are what make the
// exit-dispatch columns die, which is what erases the rebuilt region op.

#[inline(never)]
fn blend(x: u32, y: u32) -> u32 {
    x.rotate_left(y & 31) ^ y.wrapping_mul(0x85eb_ca6b)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let base = blend(input1, input2);
    let salt = base.rotate_left(13) ^ input1;
    let mut acc = 0u32;

    let mut k = 0u32;
    while k < 8 {
        if input2 >> k & 1 != 0 {
            k += 1;
            continue;
        }
        acc = acc.wrapping_add(blend(base, salt.wrapping_add(k)));
        k += 1;
    }

    let mut j = 0u32;
    while j < 8 {
        if input1 >> j & 1 != 0 {
            j += 1;
            continue;
        }
        let t = base ^ salt;
        acc ^= blend(t, acc.wrapping_add(j));
        j += 1;
    }

    // `base` and `salt` must survive both loops.
    acc.wrapping_add(base).wrapping_sub(salt)
}
