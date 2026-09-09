// Bounding sibling of `prog_varint` (campaign 21): the same LEB128 record
// decoder — four in-loop error returns over a 48-byte frame — with TWO
// running values (a running hash and a checksum-like fold) instead of six.
// Largest variant that compiles at the default level; it still panics at
// `--optimize=size-min` (see `prog_varint`).
const S1: u32 = 7;
const S2: u32 = 13;
const S3: u32 = 23;
const S4: u32 = 31;

const PRIME: u64 = 0x0100_0000_01b3;

// Builds a frame of LEB128-encoded values from the inputs; some frames are
// deliberately malformed (truncated tail, overlong zero, ten-byte value).
fn build(seed: u32, flavor: u32) -> ([u8; 48], usize) {
    let mut buf = [0u8; 48];
    let mut x = seed | 1;
    let mut n = 0usize;
    let mut count = 0u32;
    while n < 40 && count < 6 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut v = (x as u64) ^ ((x.rotate_left(S2) as u64) << S3);
        if flavor & 1 == 1 && count == 2 {
            v |= 0x8000_0000_0000_0000;
        }
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                buf[n] = byte;
                n += 1;
                break;
            }
            buf[n] = byte | 0x80;
            n += 1;
            if n >= 46 {
                break;
            }
        }
        if flavor & 8 == 8 && count == 1 && n < 30 {
            // Eleven continuation bytes: a value that cannot fit in 64 bits.
            let mut z = 0usize;
            while z < 11 {
                buf[n] = 0x80;
                n += 1;
                z += 1;
            }
            buf[n] = 0x01;
            n += 1;
        }
        if flavor & 2 == 2 && count == 1 {
            // Overlong encoding of a small value.
            buf[n] = 0x80;
            buf[n + 1] = 0x80;
            buf[n + 2] = 0x00;
            n += 3;
        }
        count += 1;
    }
    if flavor & 4 == 4 && n > 2 {
        // Truncate: leave a continuation byte as the last byte of the frame.
        buf[n - 1] |= 0x80;
    }
    (buf, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let (frame, len) = build(input1, input2 & 15);

    let mut sum = (input1 as u64).rotate_left(S1);
    let mut hash = PRIME.rotate_left(S3);

    let mut pos = 0usize;
    let mut records = 0u32;
    while pos < len {
        let start = pos;
        let mut value = 0u64;
        let mut shift = 0u32;
        loop {
            if pos >= len {
                // Truncated: a continuation byte was the last byte of the frame.
                let probe = (sum ^ sum.rotate_left(S1))
                    .wrapping_add(sum.rotate_left(S2))
                    .wrapping_mul(hash | 1)
                    ^ hash.rotate_left(S3);
                return (1u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }
            let byte = frame[pos];
            pos += 1;
            if shift >= 64 {
                // A value that does not fit in 64 bits.
                let probe = (sum.rotate_left(S2) ^ hash)
                    .wrapping_sub(sum.rotate_left(S1))
                    .wrapping_mul(hash | 3)
                    ^ hash.rotate_left(S3);
                return (2u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }
            value |= ((byte & 0x7f) as u64) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                if byte == 0 && pos - start > 1 {
                    // Overlong encoding: a redundant trailing zero byte.
                    let probe = (sum ^ hash)
                        .wrapping_add(sum.rotate_left(S3))
                        .wrapping_mul(hash | 5)
                        ^ hash.rotate_left(S3);
                    return (3u32 << 28)
                        | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
                }
                break;
            }
        }

        sum = sum.wrapping_add(value.rotate_left(S1));
        hash = hash.wrapping_mul(PRIME) ^ value.rotate_left(S3);

        if (value & 0xff) == 0x7f {
            // Reserved marker record: the stream must not continue past it.
            let probe = (sum ^ hash.rotate_left(S4))
                .wrapping_add(sum)
                .wrapping_mul(hash | 7)
                ^ hash.rotate_left(S3);
            return (4u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
        }
        records += 1;
    }

    let mut out = sum.rotate_left(S1) ^ hash.rotate_left(S2);
    out = out.wrapping_add(sum.rotate_left(S3));
    out = out.wrapping_mul(hash | 1);
    out ^= (records as u64).rotate_left(S1) ^ (len as u64).rotate_left(S2);
    (5u32 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}
