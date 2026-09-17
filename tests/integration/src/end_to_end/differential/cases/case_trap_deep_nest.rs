// Trap parity inside a nested region: four nested loops, each carrying its
// own accumulator out, with the bounds check at the innermost level. The
// escaping values become region-op result columns when cfg-to-scf lifts the
// nest, so the trapping edge has to survive the same lifting that produces
// the wide result columns behind the known spill classes. The trap fires
// when `input1 % 9` plus the nest's own counters leaves the `[u32; 12]`,
// which only the innermost trips can reach.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let t: [u32; 12] = [
        0x01, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0xff, 0x1ff, 0x3ff, 0x7ff, 0xfff,
    ];
    let base = input1 % 9;
    let mut w = input2 | 1;
    let mut x = 0u32;
    let mut y = 0u32;
    let mut z = 0u32;
    let mut a = 0u32;
    while a < 2 {
        let mut b = 0u32;
        while b < 2 {
            let mut c = 0u32;
            while c < 2 {
                let mut d = 0u32;
                while d < 2 {
                    let k = (base + a + b * 2 + c * 2 + d) as usize;
                    w = w.rotate_left(3) ^ t[k];
                    x = x.wrapping_add(w ^ (d + 1));
                    d += 1;
                }
                y = y.rotate_left(5) ^ x;
                c += 1;
            }
            z = z.wrapping_add(y ^ b);
            b += 1;
        }
        a += 1;
    }
    w ^ x ^ y ^ z
}
