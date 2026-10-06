// Trap parity where the trap IS a loop exit: the loop walks an index forward
// by `input1 % 3 + 1` and leaves either through the `acc % 7 == 3` break or
// by running off the end of the `[u32; 12]`. For the inputs that never hit
// the break, the bounds-check panic is the loop's only exit edge, which is
// the shape the control-flow lifting has to keep as an `unreachable`
// terminator inside the region rather than as a normal successor. The loop
// always terminates on both targets: the index strictly increases.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let t: [u32; 12] = [
        0x0000_0003,
        0x0000_0011,
        0x0000_0037,
        0x0000_0059,
        0x0000_0083,
        0x0000_00b1,
        0x0000_00d3,
        0x0000_0101,
        0x0000_012b,
        0x0000_0157,
        0x0000_0185,
        0x0000_01b3,
    ];
    let step = ((input1 % 3) + 1) as usize;
    let mut idx = (input2 % 4) as usize;
    let mut acc = input2 | 1;
    loop {
        acc = acc.rotate_left(5) ^ t[idx];
        if acc % 7 == 3 {
            break;
        }
        idx += step;
    }
    acc ^ (step as u32)
}
