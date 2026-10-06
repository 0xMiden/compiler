// Non-cryptographic hash mixers (campaign 17, program 3): MurmurHash3-x86-32,
// xxHash32 and FNV-1a over a 64-byte stack buffer filled by an xorshift
// generator seeded from the inputs, each hashing a runtime-length prefix
// (17..=64 bytes, so the 4-byte block loops, the 16-byte xxHash stripes and
// every tail length 0..3 are reached) and a second runtime-offset window
// whose 4-byte reads are unaligned; the three digests and an equality flag
// (the same window hashed twice must agree) are folded into the result.
fn murmur3_32(data: &[u8], seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    let len = data.len();
    let mut h = seed;
    let nblocks = len / 4;
    let mut i = 0usize;
    while i < nblocks {
        let mut k =
            u32::from_le_bytes([data[4 * i], data[4 * i + 1], data[4 * i + 2], data[4 * i + 3]]);
        k = k.wrapping_mul(C1);
        k = k.rotate_left(15);
        k = k.wrapping_mul(C2);
        h ^= k;
        h = h.rotate_left(13);
        h = h.wrapping_mul(5).wrapping_add(0xe654_6b64);
        i += 1;
    }
    let tail = nblocks * 4;
    let mut k1 = 0u32;
    let rem = len & 3;
    if rem >= 3 {
        k1 ^= (data[tail + 2] as u32) << 16;
    }
    if rem >= 2 {
        k1 ^= (data[tail + 1] as u32) << 8;
    }
    if rem >= 1 {
        k1 ^= data[tail] as u32;
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(C2);
        h ^= k1;
    }
    h ^= len as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h
}

const P1: u32 = 2_654_435_761;
const P2: u32 = 2_246_822_519;
const P3: u32 = 3_266_489_917;
const P4: u32 = 668_265_263;
const P5: u32 = 374_761_393;

fn xxh_round(acc: u32, input: u32) -> u32 {
    acc.wrapping_add(input.wrapping_mul(P2)).rotate_left(13).wrapping_mul(P1)
}

fn read_le(data: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]])
}

fn xxh32(data: &[u8], seed: u32) -> u32 {
    let len = data.len();
    let mut i = 0usize;
    let mut h = if len >= 16 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while i + 16 <= len {
            v1 = xxh_round(v1, read_le(data, i));
            v2 = xxh_round(v2, read_le(data, i + 4));
            v3 = xxh_round(v3, read_le(data, i + 8));
            v4 = xxh_round(v4, read_le(data, i + 12));
            i += 16;
        }
        v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18))
    } else {
        seed.wrapping_add(P5)
    };
    h = h.wrapping_add(len as u32);
    while i + 4 <= len {
        h = h.wrapping_add(read_le(data, i).wrapping_mul(P3));
        h = h.rotate_left(17).wrapping_mul(P4);
        i += 4;
    }
    while i < len {
        h = h.wrapping_add((data[i] as u32).wrapping_mul(P5));
        h = h.rotate_left(11).wrapping_mul(P1);
        i += 1;
    }
    h ^= h >> 15;
    h = h.wrapping_mul(P2);
    h ^= h >> 13;
    h = h.wrapping_mul(P3);
    h ^= h >> 16;
    h
}

fn fnv1a(data: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 64];
    let mut x = input1 ^ 0x2545_f491 ^ input2.rotate_left(11) | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = (x >> ((i as u32 & 3) * 8)) as u8;
        i += 1;
    }
    let len = (17 + input2 % 48) as usize;
    let off = (1 + input1 % 13) as usize;
    let m = murmur3_32(&buf[..len], input2);
    let xh = xxh32(&buf[..len], input1);
    let f = fnv1a(&buf[off..len]);
    // A runtime-offset window: unaligned 4-byte reads.
    let m2 = murmur3_32(&buf[off..len], m);
    let x2 = xxh32(&buf[off..len], xh);
    let again = xxh32(&buf[off..len], xh);
    let same = (again == x2) as u32;
    m ^ xh.rotate_left(7)
        ^ f.rotate_left(13)
        ^ m2.rotate_left(19)
        ^ x2.rotate_left(27)
        ^ (same << 31)
}
