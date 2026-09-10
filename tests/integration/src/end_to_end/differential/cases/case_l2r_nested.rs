// One local reassigned in the body of an INNER loop and read again in the
// outer body, plus a per-iteration temporary that is stored and loaded once
// inside the inner body with no intervening control flow — the only shape in
// this nest that satisfies Local2Reg's same-block/no-control-flow rule. The
// loop-carried locals are stored on every backedge, so promotion can never
// take them; erasing the temporary's store/load pair must not change which
// value the backedge carries.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let outer = (input1 % 11) + 2;
    let inner = (input2 % 7) + 2;

    let mut carry: u64 = ((input1 as u64) << 19) | 5;
    let mut tag: u32 = input2 | 1;

    let mut o: u32 = 0;
    while o < outer {
        let mut i: u32 = 0;
        while i < inner {
            // Stored once, loaded once, no control flow in between.
            let step = tag.rotate_left((i & 15) + 1) ^ (o.wrapping_mul(0x9e37_79b9));
            carry = carry.rotate_left(7) ^ (step as u64);
            tag = tag.wrapping_add(step ^ (carry as u32));
            i = i.wrapping_add(1);
        }
        // The inner loop's last value of `tag` is read again out here.
        carry = carry.wrapping_add((tag as u64) << 11);
        tag ^= (carry >> 32) as u32;
        o = o.wrapping_add(1);
    }
    tag ^ (carry as u32) ^ ((carry >> 32) as u32)
}
