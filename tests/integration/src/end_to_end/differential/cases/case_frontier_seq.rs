// Minimal `frontier.rs:123` reproducer without a zero-trip-capable loop
// (campaign 21, B1): two sequential bottom-tested loops where FOUR masked
// rotate count bands are used before the first loop and again only inside the
// second one.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= n.rotate_left(7);
    let mut out = acc;
    let mut j: u32 = 0;
    while j < (input1 % 7) + 2 {
        out ^= out.rotate_left(2);
        j = j.wrapping_add(1);
    }
    let mut k2: u32 = 0;
    while k2 < (input2 % 7) + 2 {
        out ^= acc.rotate_left(1);
        out = out.wrapping_add(out.rotate_left(3));
        out ^= acc.rotate_left(5);
        out = out.wrapping_add(out.rotate_left(7));
        k2 = k2.wrapping_add(1);
    }
    (out as u32) ^ ((out >> 32) as u32)
}
