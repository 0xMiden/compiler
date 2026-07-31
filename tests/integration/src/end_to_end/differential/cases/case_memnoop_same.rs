// DELIBERATE PROBE: a zero-length copy at an IDENTICAL src == dst position.
// Length-0 ranges cannot overlap, so native is a no-op; the copy still
// reaches a runtime `memory.copy` because both the length and the dst offset
// are opaquely zero (impossible cross-modulus guards LLVM cannot fold, and
// src/dst stay syntactically distinct so the memmove is not elided). At
// runtime the 4-aligned zero count takes the element fast path and execs
// miden-core-lib `memcopy_elements` with count 0 and read_ptr == write_ptr —
// asserting its range-overlap assert accepts the degenerate len-0 case.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2;
    // Opaquely always-zero length: h % 6 == 5 implies h % 3 == 2, never 0.
    let zlen = ((h % 6 == 5 && h % 3 == 0) as usize) * ((input2 & 3) as usize + 1);
    // Opaquely always-zero dst offset: h % 10 == 7 implies h % 5 == 2, never 1.
    let zoff = ((h % 10 == 7 && h % 5 == 1) as usize) * 2;

    let mut a = [0u32; 8];
    let mut i = 0u32;
    while i < 8 {
        a[i as usize] = input1.wrapping_add(i).rotate_left(i % 7) ^ input2;
        i += 1;
    }

    let p = (input1 % 4) as usize;
    // Dynamically always copy_within(p..p, p): src == dst, len == 0.
    a.copy_within(p..p + zlen, p + zoff);

    a[(input2 % 8) as usize].wrapping_add(a[p])
}
