// Trap parity: the `Option`-returning accessors panicking through `unwrap`,
// against a `static` table (a data segment rather than a stack array).
// `TABLE.get(i).unwrap()` and `TABLE.iter().nth(j).unwrap()` both panic on
// `Option::None` rather than through a bounds check, so this is a different
// panic site than plain `table[i]` for the same out-of-range index.
static TABLE: [u32; 9] = [
    0x0000_0001,
    0x0000_0020,
    0x0000_0300,
    0x0000_4000,
    0x0005_0000,
    0x0060_0000,
    0x0700_0000,
    0x8000_0000,
    0xffff_ffff,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = (input1 % 12) as usize;
    let j = (input2 % 12) as usize;
    // Must not trap: the defaulting sibling of the same lookup.
    let lax = TABLE.get(i.wrapping_add(j)).copied().unwrap_or(0xdead_beef);
    let a = *TABLE.get(i).unwrap();
    let b = *TABLE.iter().nth(j).unwrap();
    a.wrapping_mul(3) ^ b.rotate_left(7) ^ lax
}
