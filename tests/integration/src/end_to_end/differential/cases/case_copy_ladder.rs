// Runtime-length copies and fills at misaligned offsets. The length comes
// from a table indexed by the inputs (0, 1, 3, 4, 5, 7, 8, 9, 15, 16, 17,
// 31, 32, 33, 2, 6 — every element-count boundary the memcpy/memset
// lowerings split on) and the source/destination byte offsets take all
// values 0..3, so each `memory.copy` sees every (src%4, dst%4, len%4)
// combination: static -> stack, stack -> stack into the middle of a
// larger buffer, and a disjoint in-buffer `copy_within`, plus a non-zero
// `fill` of runtime length. Both destination buffers are hashed whole, so
// a copy of the wrong length/direction or a fill spilling past its range
// changes the result through the untouched neighbours.
static LENS: [u8; 16] = [0, 1, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 2, 6];

static SRC: [u8; 48] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00,
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x07, 0x18, 0x29, 0x3a, 0x4b, 0x5c, 0x6d, 0x7e, 0x8f, 0x90,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let len = LENS[(input1 & 15) as usize] as usize;
    let len2 = LENS[((input1 >> 4) & 15) as usize] as usize;
    let so = (input2 & 3) as usize;
    let dof = ((input2 >> 2) & 3) as usize;

    let mut a = [0u8; 80];
    let mut b = [0u8; 96];
    let mut k = 0usize;
    while k < 80 {
        a[k] = (input1 >> (k & 7)) as u8 ^ (k as u8).wrapping_mul(3);
        k += 1;
    }
    k = 0;
    while k < 96 {
        b[k] = (input2 >> (k & 7)) as u8 ^ (k as u8).wrapping_mul(5);
        k += 1;
    }

    // Static -> stack at a misaligned destination.
    a[dof..dof + len].copy_from_slice(&SRC[so..so + len]);
    // Stack -> stack into the middle of the larger buffer.
    b[40 + dof..40 + dof + len].copy_from_slice(&a[so..so + len]);
    // Disjoint in-buffer copy: src ends by 36, dst starts at 44.
    a.copy_within(so..so + len, 44 + dof);
    // Non-zero fill of runtime length in the low half of `b`.
    b[8 + so..8 + so + len2].fill((input1 ^ 0xa5) as u8);
    // Element-typed copy of runtime length (byte count multiple of 4).
    let mut c = [0u32; 12];
    let mut d = [0u32; 12];
    k = 0;
    while k < 12 {
        d[k] = input1.wrapping_mul(k as u32 + 1) ^ input2;
        k += 1;
    }
    let n = (len % 9) + (len2 % 3);
    c[..n].copy_from_slice(&d[..n]);

    let mut acc = 0u32;
    k = 0;
    while k < 80 {
        acc = acc.rotate_left(3) ^ (a[k] as u32).wrapping_mul(k as u32 | 1);
        k += 1;
    }
    k = 0;
    while k < 96 {
        acc = acc.rotate_left(5) ^ (b[k] as u32).wrapping_mul(0x0101_0101);
        k += 1;
    }
    k = 0;
    while k < 12 {
        acc = acc.wrapping_add(c[k].rotate_left(k as u32));
        k += 1;
    }
    acc
}
