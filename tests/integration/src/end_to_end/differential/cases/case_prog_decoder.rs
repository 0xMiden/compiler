// Table-driven decoder (campaign 17, program 6): a canonical Huffman code
// (code lengths in `.rodata`, the 4-bit lookahead table built at runtime
// in a stack array) decodes an input-derived bitstream into LZ-style
// tokens — nibble literals expanded through a `.rodata` table, byte
// literals, short and long back-references (runtime distance and length:
// disjoint ranges go through `copy_within`, overlapping or xor-delta ones
// through a byte loop), a delta register, skips, and dictionary copies
// from a `.rodata` string — writing into a 256-byte stack buffer with a
// bound on tokens and output position; the buffer, the symbol histogram
// and the decoder registers are hashed into the result.
static LENS: [u8; 8] = [2, 2, 3, 3, 4, 4, 4, 4];
static NIBBLE: [u8; 16] = [
    0x20, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
];
static DICT: [u8; 32] = *b"the quick brown fox jumps over t";

const OUT_LEN: usize = 256;

struct Bits {
    data: [u8; 32],
    pos: u32,
}

impl Bits {
    // Reads `n` bits MSB-first; past the end every bit reads as zero.
    fn read(&mut self, n: u32) -> u32 {
        let mut v = 0u32;
        let mut k = 0u32;
        while k < n {
            let p = self.pos;
            let bit = if p < 256 {
                (self.data[(p >> 3) as usize] >> (7 - (p & 7))) & 1
            } else {
                0
            };
            v = (v << 1) | bit as u32;
            self.pos = p + 1;
            k += 1;
        }
        v
    }
}

// Builds the canonical code lookup table: entry = symbol | length << 4.
fn build_lut(lut: &mut [u8; 16]) {
    let mut code = 0u32;
    let mut len = 1u32;
    while len <= 4 {
        let mut sym = 0usize;
        while sym < 8 {
            if LENS[sym] as u32 == len {
                let base = (code << (4 - len)) as usize;
                let span = 1usize << (4 - len);
                let mut k = 0usize;
                while k < span {
                    lut[base + k] = sym as u8 | (len as u8) << 4;
                    k += 1;
                }
                code += 1;
            }
            sym += 1;
        }
        code <<= 1;
        len += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut lut = [0u8; 16];
    build_lut(&mut lut);
    let mut bits = Bits {
        data: [0u8; 32],
        pos: 0,
    };
    let mut x = input1 ^ 0x5bd1_e995 ^ input2.rotate_left(7) | 1;
    let mut i = 0usize;
    while i < 32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        bits.data[i] = (x >> 24) as u8 ^ (input2 >> ((i as u32 * 3) & 31)) as u8;
        i += 1;
    }
    let mut out = [0u8; OUT_LEN];
    let mut hist = [0u32; 8];
    let mut p = 0usize;
    let mut delta = 0u8;
    let mut tokens = 0u32;
    let mut ended = 0u32;
    while tokens < 48 && p + 24 <= OUT_LEN {
        tokens += 1;
        let look = bits.read(4) as usize;
        let e = lut[look];
        let sym = (e & 15) as usize;
        let len = (e >> 4) as u32;
        // Give back the unused lookahead bits.
        bits.pos -= 4 - len;
        hist[sym] += 1;
        match sym {
            0 => {
                let nib = bits.read(4) as usize;
                out[p] = NIBBLE[nib] ^ delta;
                p += 1;
            }
            1 => {
                out[p] = bits.read(8) as u8;
                p += 1;
            }
            2 | 3 => {
                let (len, dist) = if sym == 2 {
                    (2 + bits.read(2) as usize, 1 + bits.read(3) as usize)
                } else {
                    (4 + bits.read(4) as usize, 1 + bits.read(5) as usize)
                };
                let dist = if dist > p { p } else { dist };
                if dist == 0 {
                    continue;
                }
                let src = p - dist;
                if dist >= len && delta == 0 {
                    out.copy_within(src..src + len, p);
                } else {
                    let mut k = 0usize;
                    while k < len {
                        let b = out[src + k] ^ delta;
                        out[p + k] = b;
                        k += 1;
                    }
                }
                p += len;
            }
            4 => {
                delta = bits.read(8) as u8;
            }
            5 => {
                p += 1 + bits.read(2) as usize;
            }
            6 => {
                let off = (bits.read(3) * 3) as usize;
                let len = 4 + bits.read(2) as usize;
                out[p..p + len].copy_from_slice(&DICT[off..off + len]);
                p += len;
            }
            _ => {
                ended = 1;
                break;
            }
        }
    }
    // Hash the produced buffer (a murmur-style mix per byte) and the
    // decoder state.
    let mut h = 0x811c_9dc5u32 ^ (p as u32) << 16 ^ tokens << 8 ^ delta as u32 ^ ended << 31;
    i = 0;
    while i < p {
        h ^= out[i] as u32;
        h = h.wrapping_mul(0x0100_0193).rotate_left(5);
        i += 1;
    }
    i = 0;
    while i < 8 {
        h = h.wrapping_add(hist[i].wrapping_mul(0x9e37_79b9 >> i));
        i += 1;
    }
    h ^ bits.pos
}
