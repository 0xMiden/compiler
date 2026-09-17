// Trap parity under register pressure: eight u64 state words live across a
// six-trip ARX loop, then a bounds check whose index depends on the loop's
// result. The trap therefore cannot be hoisted above the work, and the
// trapping edge is created inside the region the spill analysis and the
// operand scheduler have to solve — the machinery behind the known
// compile-time classes. `input1 % 13` plus the low bit of the final state
// reaches 0..13 over a `[u32; 10]`, so the trap-or-value decision is decided
// by the loop.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let table: [u32; 10] = [
        0x1111_1111,
        0x2222_2222,
        0x3333_3333,
        0x4444_4444,
        0x5555_5555,
        0x6666_6666,
        0x7777_7777,
        0x8888_8888,
        0x9999_9999,
        0xaaaa_aaaa,
    ];
    let mut a = (input1 as u64) | 1;
    let mut b = ((input2 as u64) << 3) | 1;
    let mut c = a ^ 0x9e37_79b9_7f4a_7c15;
    let mut d = b.rotate_left(17) | 1;
    let mut e = a.wrapping_mul(0x0000_0100_0000_01b3);
    let mut f = b ^ c;
    let mut g = c.rotate_right(29) | 1;
    let mut h = d ^ e;
    let mut i = 0u32;
    while i < 6 {
        a = a.rotate_left(7) ^ b;
        b = b.wrapping_add(c);
        c = c.rotate_left(13) ^ d;
        d = d.wrapping_add(e);
        e = e.rotate_left(29) ^ f;
        f = f.wrapping_add(g);
        g = g.rotate_left(11) ^ h;
        h = h.wrapping_add(a);
        i += 1;
    }
    let idx = (input1 % 13) as usize + (h & 1) as usize;
    let v = table[idx];
    v ^ (a as u32)
        ^ (b as u32)
        ^ (c as u32)
        ^ (d as u32)
        ^ (e as u32)
        ^ (f as u32)
        ^ (g as u32)
        ^ (h as u32)
}
