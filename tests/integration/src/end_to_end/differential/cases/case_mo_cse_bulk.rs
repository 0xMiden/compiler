// Campaign 30 / W1: the opaque write between the two loads is a BULK op with
// a runtime destination and a runtime length — `copy_from_slice` from a
// static, a disjoint `copy_within`, `fill`, `ptr::write_bytes` and
// `ptr::copy_nonoverlapping` — so the HIR gets a `hir.memcpy`
// (Read(source) + Write(destination)) or a `hir.memset` (Write(destination))
// between two `hir.load`s of the same address. One probe uses an opaquely
// zero length (the cross-modulus contradiction `h % 6 == 5 && h % 3 == 0`)
// so a zero-length range reaches the VM instead of being folded away. Every
// pre/post pair and the whole buffer are folded into the result.
static SRC: [u8; 24] = [
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x01,
    0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = [0u8; 64];
    let mut k = 0usize;
    while k < 64 {
        b[k] = (input1.rotate_left(k as u32 & 31) ^ input2.wrapping_mul(k as u32 + 3)) as u8;
        k += 1;
    }
    let o = (input2 & 31) as usize; // probed byte
    let d = ((input2 >> 5) & 31) as usize; // bulk destination
    let len = ((input1 >> 5) % 9) as usize; // 0..=8 bytes
    let fillv = (input1 >> 16) as u8;
    // Opaquely zero: no modulus-6 residue 5 is divisible by 3.
    let zlen = ((input1 % 6 == 5 && input1 % 3 == 0) as usize) * ((input2 as usize) & 7);
    let mut acc = 0u32;

    // copy_from_slice from a static (`hir.memcpy`, rodata source).
    let a1 = b[o];
    b[d..d + len].copy_from_slice(&SRC[..len]);
    let c1 = b[o];
    acc = acc.rotate_left(3) ^ (a1 as u32).wrapping_mul(0x0100_0193) ^ (c1 as u32);

    // fill over the same runtime range (`hir.memset`).
    let a2 = b[o];
    b[d..d + len].fill(fillv);
    let c2 = b[o];
    acc = acc.rotate_left(5) ^ (a2 as u32) ^ (c2 as u32).wrapping_mul(7);

    // write_bytes through a raw pointer (`hir.memset` again, no slice bound).
    let a3 = b[o];
    unsafe { core::ptr::write_bytes(b.as_mut_ptr().add(d), fillv ^ 0x3c, len) };
    let c3 = b[o];
    acc = acc.rotate_left(7) ^ (a3 as u32).wrapping_mul(31) ^ (c3 as u32);

    // copy_nonoverlapping from the far half into the runtime destination.
    let a4 = b[o];
    unsafe {
        let base = b.as_mut_ptr();
        core::ptr::copy_nonoverlapping(base.add(32 + (d & 15)), base.add(d & 15), len);
    }
    let c4 = b[o];
    acc = acc.rotate_left(11) ^ (a4 as u32) ^ (c4 as u32).wrapping_mul(13);

    // Disjoint copy_within: source in the second half, destination in the
    // first, never overlapping (the overlapping shapes are `mem_overlap` /
    // `copy_same_pos`).
    let a5 = b[o];
    let s5 = 40 + (d & 7);
    b.copy_within(s5..s5 + len, d & 15);
    let c5 = b[o];
    acc = acc.rotate_left(13) ^ (a5 as u32).wrapping_mul(17) ^ (c5 as u32);

    // Opaquely zero-length fill: the range is empty, so both loads must
    // agree — a divergence here means the empty memset wrote something.
    let a6 = b[o];
    b[d..d + zlen].fill(fillv);
    let c6 = b[o];
    acc = acc.rotate_left(17) ^ (a6 as u32) ^ (c6 as u32).wrapping_mul(19);

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 64 {
        s = s.rotate_left(3).wrapping_add(b[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}
