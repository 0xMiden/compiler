// Zero/boundary-length memory ops, all copy ranges disjoint by construction.
// Edge relations asserted by the pinned grid: u32-element copies of runtime
// length 0 and 1 (memory.copy byte count 0/4 through the element fast path —
// u32 arrays are always 4-aligned), byte copies of length 0..=5 crossing the
// `count % 4` fastpath/byte-tail boundary of the memcpy lowering (0/4 may
// take memcopy_elements, 1/2/3/5 take the byte fallback loop), and a byte
// `fill` of runtime length 0 and 1 (memory.fill count 0). Fixed-index reads
// of the destination cells assert that length-0 ops wrote NOTHING.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 8];
    let mut i = 0u32;
    while i < 8 {
        a[i as usize] = input1.wrapping_add(i.wrapping_mul(0x0101_0101)) ^ input2;
        i += 1;
    }

    // u32-element copy into a zeroed buffer, runtime length 0..=4 elements.
    let n = (input2 % 5) as usize;
    let mut b = [0u32; 8];
    b[..n].copy_from_slice(&a[..n]);

    // Disjoint in-buffer element copy: src 0..n (n <= 4), dst 4..4+n.
    a.copy_within(0..n, 4);

    // Byte copy of length 0..=5 across the %4 fastpath boundary; src [0, m)
    // and dst [16, 16+m) are disjoint for every m <= 5.
    let mut bytes = [0u8; 24];
    let mut j = 0u32;
    while j < 12 {
        bytes[j as usize] = (input1 >> (j % 8)) as u8;
        j += 1;
    }
    let m = (input1 % 6) as usize;
    bytes.copy_within(0..m, 16);

    // Byte fill of runtime length 0..=4 over [8, 12).
    let flen = (input2 % 5) as usize;
    bytes[8..8 + flen].fill(input2 as u8);

    // Fixed-index probes: each destination cell distinguishes "wrote" from
    // "length 0 left it alone".
    let p1 = bytes[16] as u32; // 0 unless the byte copy ran (m >= 1)
    let p2 = bytes[8] as u32; // j-loop value unless the fill ran (flen >= 1)
    let p3 = b[0]; // 0 unless the element copy ran (n >= 1)
    let p4 = a[4]; // original a[4] unless copy_within ran (n >= 1)

    p1.wrapping_add(p2.rotate_left(5))
        .wrapping_add(p3.rotate_left(9))
        .wrapping_add(p4.rotate_left(13))
        .wrapping_add(b[(input1 % 8) as usize].rotate_left(17))
        .wrapping_add(bytes[(input2 % 24) as usize] as u32)
}
