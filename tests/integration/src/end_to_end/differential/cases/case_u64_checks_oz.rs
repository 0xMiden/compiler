// u64 checked/saturating/overflowing arithmetic, unsigned and signed compare
// chains, sign extensions, and guarded unsigned div/rem inside a 6-trip loop
// that -Oz (`--optimize=size-min`) keeps as a loop (O2 unrolls it into one
// straight-line block). The legalized compare+select sequences and the
// wide-arithmetic ops therefore run through loop-carried state with the
// operand-stack pressure the un-unrolled shape leaves behind.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x: u64 = ((input1 as u64) << 32) | (input2 as u64);
    let mut y: u64 = (input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (input1 as u64);
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < 6 {
        acc = acc.wrapping_add(match x.checked_add(y) {
            Some(v) => v as u32,
            None => 1,
        });
        acc ^= x.saturating_sub(y) as u32;
        let (m, of) = x.overflowing_mul(y | 1);
        acc = acc.wrapping_add(m as u32).wrapping_add(of as u32);
        let sx = (input1 as i32) as i64 ^ (x as i64);
        let sy = (y as i64) >> (i as u32);
        acc ^= (x < y) as u32
            | ((x <= y) as u32) << 1
            | ((sx < sy) as u32) << 2
            | ((sx >= sy) as u32) << 3
            | ((x == y.rotate_left(1)) as u32) << 4;
        let d = y | 1;
        acc ^= (x / d) as u32 ^ (x % (d.rotate_left(13) | 1)) as u32;
        acc = acc.wrapping_add((x.wrapping_sub(y).wrapping_neg() >> 32) as u32);
        x = x.rotate_left(7) ^ y;
        y = y.wrapping_add(acc as u64).wrapping_add(sx as u64 & 0xffff);
        i += 1;
    }
    acc
}
