// UTF-8 validator with per-error-class returns (campaign 21, cliff shape:
// return-heavy inner loop nested in an outer loop).  Two 40-byte buffers are
// validated in an outer pass loop; the per-code-point inner loop returns a
// distinct error class for a bad leading byte, a bad continuation byte, an
// overlong encoding, a surrogate and an out-of-range code point, and keeps
// five u64 statistics — a code-point checksum, a length histogram fold, a
// running FNV hash, a maximum and a bit mask of the classes seen — that are
// combined in ONE expression on every error path and folded into the result
// on the success path.  The rotate constants 11, 19, 27 and 37 are used by
// the buffer builder, inside the validation loop and in the final fold.
const R1: u32 = 11;
const R2: u32 = 19;
const R3: u32 = 27;
const R4: u32 = 37;

const FNV: u64 = 0x0100_0000_01b3;

// Encodes a sequence of code points derived from the seed; `flavor` plants a
// specific malformation in the second code point.
fn encode(seed: u32, flavor: u32) -> ([u8; 40], usize) {
    let mut buf = [0u8; 40];
    let mut x = seed | 1;
    let mut n = 0usize;
    let mut k = 0u32;
    while n < 32 && k < 8 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let cp = match (x >> 5) & 3 {
            0 => x & 0x7f,
            1 => 0x80 + (x & 0x6ff),
            2 => 0x800 + (x & 0x7fff),
            _ => 0x1_0000 + (x & 0x3_ffff),
        };
        if k == 1 {
            match flavor {
                1 => {
                    buf[n] = 0xc0;
                    buf[n + 1] = 0x80;
                    n += 2;
                    k += 1;
                    continue;
                }
                2 => {
                    buf[n] = 0xe0;
                    buf[n + 1] = 0x41;
                    n += 2;
                    k += 1;
                    continue;
                }
                3 => {
                    buf[n] = 0xed;
                    buf[n + 1] = 0xa0;
                    buf[n + 2] = 0x80;
                    n += 3;
                    k += 1;
                    continue;
                }
                4 => {
                    buf[n] = 0xf5;
                    buf[n + 1] = 0x80;
                    buf[n + 2] = 0x80;
                    buf[n + 3] = 0x80;
                    n += 4;
                    k += 1;
                    continue;
                }
                5 => {
                    buf[n] = 0x80;
                    n += 1;
                    k += 1;
                    continue;
                }
                _ => {}
            }
        }
        if cp < 0x80 {
            buf[n] = cp as u8;
            n += 1;
        } else if cp < 0x800 {
            buf[n] = 0xc0 | (cp >> 6) as u8;
            buf[n + 1] = 0x80 | (cp & 0x3f) as u8;
            n += 2;
        } else if cp < 0x1_0000 {
            buf[n] = 0xe0 | (cp >> 12) as u8;
            buf[n + 1] = 0x80 | ((cp >> 6) & 0x3f) as u8;
            buf[n + 2] = 0x80 | (cp & 0x3f) as u8;
            n += 3;
        } else {
            buf[n] = 0xf0 | (cp >> 18) as u8;
            buf[n + 1] = 0x80 | ((cp >> 12) & 0x3f) as u8;
            buf[n + 2] = 0x80 | ((cp >> 6) & 0x3f) as u8;
            buf[n + 3] = 0x80 | (cp & 0x3f) as u8;
            n += 4;
        }
        k += 1;
    }
    if flavor == 6 && n < 39 {
        // A three-byte sequence cut off by the end of the buffer.
        buf[n] = 0xe1;
        n += 1;
    }
    (buf, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut sumcp = (input1 as u64).rotate_left(R1);
    let mut widths = (input2 as u64).rotate_left(R2);
    let mut hash = FNV.rotate_left(R3);
    let mut maxcp = 0u64;
    let mut classes = 0u64;

    let mut pass = 0u32;
    while pass < 2 {
        let (buf, len) = encode(input1 ^ (pass.wrapping_mul(0x9e37_79b9)), (input2 >> (pass * 4)) & 7);
        let mut i = 0usize;
        while i < len {
            let b0 = buf[i];
            let width = if b0 < 0x80 {
                1usize
            } else if b0 >= 0xc2 && b0 < 0xe0 {
                2
            } else if b0 >= 0xe0 && b0 < 0xf0 {
                3
            } else if b0 >= 0xf0 && b0 < 0xf5 {
                4
            } else {
                // Bad leading byte (continuation byte in leading position,
                // 0xc0/0xc1 overlong starter, or 0xf5..).
                let probe = (sumcp ^ widths.rotate_left(R1))
                    .wrapping_add(hash ^ maxcp.rotate_left(R2))
                    .wrapping_mul(classes | 1)
                    ^ (b0 as u64).rotate_left(R3);
                return (1u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            };
            if i + width > len {
                let probe = (sumcp.rotate_left(R2) ^ widths)
                    .wrapping_sub(hash.wrapping_add(maxcp))
                    .wrapping_mul(classes | 3)
                    ^ (len as u64).rotate_left(R4);
                return (2u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }
            let mut cp = match width {
                1 => b0 as u64,
                2 => (b0 & 0x1f) as u64,
                3 => (b0 & 0x0f) as u64,
                _ => (b0 & 0x07) as u64,
            };
            let mut j = 1usize;
            while j < width {
                let bj = buf[i + j];
                if bj & 0xc0 != 0x80 {
                    let probe = (sumcp ^ widths.rotate_left(R3))
                        .wrapping_add(hash.rotate_left(R1) ^ maxcp)
                        .wrapping_mul(classes | 5)
                        ^ (bj as u64).rotate_left(R2);
                    return (3u32 << 28)
                        | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
                }
                cp = (cp << 6) | ((bj & 0x3f) as u64);
                j += 1;
            }
            if (0xd800..0xe000).contains(&cp) || cp > 0x10_ffff {
                let probe = (sumcp.rotate_left(R4) ^ widths)
                    .wrapping_add(hash ^ maxcp.rotate_left(R3))
                    .wrapping_mul(classes | 7)
                    ^ cp.rotate_left(R1);
                return (4u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }

            sumcp = sumcp.wrapping_add(cp.rotate_left(R1));
            widths ^= (width as u64).rotate_left(R2).wrapping_mul(FNV);
            hash = (hash ^ cp).wrapping_mul(FNV).rotate_left(R3);
            if cp > maxcp {
                maxcp = cp;
            }
            classes |= 1u64 << ((cp & 0x1f) as u32);
            i += width;
        }
        pass += 1;
    }

    let mut out = sumcp.rotate_left(R1) ^ widths.rotate_left(R2);
    out = out.wrapping_add(hash.rotate_left(R3));
    out ^= maxcp.rotate_left(R4);
    out = out.wrapping_mul(classes | 1);
    (5u32 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}
