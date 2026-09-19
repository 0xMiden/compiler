// Nested TLV record validator (campaign 21, cliff shape: three-level diamond
// nest whose deepest arm is the only consumer of the accumulated words).  A
// 64-byte container is parsed as tag / length / value records; the outer
// check accepts the container, the middle check accepts the record header and
// only the innermost success path — a well-formed value of a known type —
// touches the eight u64 digest words that were computed before the checks.
// The rotate constants 7, 19, 31 and 43 are shared by the container builder,
// the digest words and the final fold.
const D0: u32 = 7;
const D1: u32 = 19;
const D2: u32 = 31;
const D3: u32 = 43;

const SEED: u64 = 0xd6e8_feb8_6659_fd93;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 64];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = (x >> 7) as u8;
        i += 1;
    }
    // Container header: magic, version, record count.
    buf[0] = if input2 & 1 == 0 { 0x54 } else { 0x00 };
    buf[1] = if input2 & 2 == 0 { 0x4c } else { 0xff };
    buf[2] = 2 + ((input2 >> 2) & 3) as u8;
    buf[3] = ((input2 >> 4) & 7) as u8;

    // Eight digest words computed up front; only the deepest accepted record
    // ever reads them.
    let d0 = ((input1 as u64) << 32 | input2 as u64).wrapping_mul(SEED);
    let d1 = d0.rotate_left(D0) ^ SEED;
    let d2 = d1.wrapping_add(d0.rotate_left(D1));
    let d3 = (d2 ^ d1.rotate_left(D2)).wrapping_mul(SEED | 3);
    let d4 = d3.rotate_left(D3) ^ d0;
    let d5 = (d4.wrapping_sub(d2)).rotate_left(D0) ^ d1;
    let d6 = (d5 ^ d3.rotate_left(D1)).wrapping_add(d4);
    let d7 = (d6.rotate_left(D2) ^ d0.rotate_left(D3)).wrapping_mul(SEED | 1);

    let mut digest = SEED.rotate_left(D0) ^ (input1 as u64);
    let mut accepted = 0u32;
    let mut rejected = 0u32;
    let mut pos = 4usize;
    let count = buf[2] as usize;
    let mut rec = 0usize;

    while rec < count && pos + 3 <= 64 {
        let tag = buf[pos];
        let len = (buf[pos + 1] & 15) as usize;
        let kind = buf[pos + 2];
        pos += 3;

        if buf[0] == 0x54 && buf[1] == 0x4c {
            // The container is well formed.
            if tag & 0xc0 == 0x40 && pos + len <= 64 {
                // The record header is well formed.
                if kind < 4 && len >= 2 {
                    // The value is of a known type: the only path that reads
                    // the digest words.
                    let mut v = 0u64;
                    let mut j = 0usize;
                    while j < len {
                        v = (v << 8) | (buf[pos + j] as u64);
                        j += 1;
                    }
                    let mixed = (d0 ^ v.rotate_left(D0))
                        .wrapping_add(d1 ^ v.rotate_left(D1))
                        .wrapping_mul(d2 | 1)
                        ^ d3.rotate_left(D2)
                        ^ (d4.wrapping_sub(v) ^ d5.rotate_left(D3))
                        ^ d6.wrapping_add(d7.rotate_left(D0));
                    digest = digest.wrapping_mul(SEED) ^ mixed;
                    accepted += 1;
                } else {
                    digest ^= (kind as u64).rotate_left(D1);
                    rejected += 1;
                }
            } else {
                digest = digest.wrapping_add((tag as u64).rotate_left(D2));
                rejected += 1;
            }
        } else {
            digest ^= (pos as u64).rotate_left(D3);
            rejected += 1;
        }

        pos += len;
        rec += 1;
    }

    let mut out = digest.rotate_left(D0);
    out ^= (accepted as u64).rotate_left(D1);
    out = out.wrapping_add((rejected as u64).rotate_left(D2));
    out ^= (pos as u64).rotate_left(D3);
    (out as u32) ^ ((out >> 32) as u32)
}
