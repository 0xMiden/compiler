// Workaround variant of `case_prog_tlv.rs` (campaign 22): identical program,
// except that the deepest arm's eight-word mixing expression moves into an
// `#[inline(never)] fn mix(&[u64; 8], u64)` helper.  Same answer on every
// input; compiles at the default level, where the original panics at every
// optimization level (F6, frontier.rs:123).
const D0: u32 = 7;
const D1: u32 = 19;
const D2: u32 = 31;
const D3: u32 = 43;

const SEED: u64 = 0xd6e8_feb8_6659_fd93;

#[inline(never)]
fn mix(w: &[u64; 8], v: u64) -> u64 {
    (w[0] ^ v.rotate_left(D0))
        .wrapping_add(w[1] ^ v.rotate_left(D1))
        .wrapping_mul(w[2] | 1)
        ^ w[3].rotate_left(D2)
        ^ (w[4].wrapping_sub(v) ^ w[5].rotate_left(D3))
        ^ w[6].wrapping_add(w[7].rotate_left(D0))
}

#[inline(always)]
fn words_inline(input1: u32, input2: u32) -> [u64; 8] {
    let d0 = ((input1 as u64) << 32 | input2 as u64).wrapping_mul(SEED);
    let d1 = d0.rotate_left(D0) ^ SEED;
    let d2 = d1.wrapping_add(d0.rotate_left(D1));
    let d3 = (d2 ^ d1.rotate_left(D2)).wrapping_mul(SEED | 3);
    let d4 = d3.rotate_left(D3) ^ d0;
    let d5 = (d4.wrapping_sub(d2)).rotate_left(D0) ^ d1;
    let d6 = (d5 ^ d3.rotate_left(D1)).wrapping_add(d4);
    let d7 = (d6.rotate_left(D2) ^ d0.rotate_left(D3)).wrapping_mul(SEED | 1);
    [d0, d1, d2, d3, d4, d5, d6, d7]
}

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
    let w = words_inline(input1, input2);
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
                    let mixed = mix(&w, v);
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
