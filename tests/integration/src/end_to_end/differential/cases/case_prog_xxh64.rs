// xxHash64 over a 64-byte buffer with a runtime length, followed by the
// MurmurHash3-x64 finalizer over the digest.  ELEVEN distinct rotate/shift
// constants by construction: the stripe round's 31, the convergence
// rotations 1, 7, 12 and 18, the 8-, 4- and 1-byte tail rotations 27, 23 and
// 11, and the avalanche shifts 33, 29 and 32 (the Murmur finalizer reuses
// 33).  They cross the stripe loop, the tail loops and the finalization.
// Sixty-four-bit multiplies only -- no u64 x u64 -> u128 products.
const P1: u64 = 0x9e37_79b1_85eb_ca87;
const P2: u64 = 0xc2b2_ae3d_27d4_eb4f;
const P3: u64 = 0x1656_67b1_9e37_79f9;
const P4: u64 = 0x85eb_ca77_c2b2_ae63;
const P5: u64 = 0x27d4_eb2f_1656_67c5;

const M1: u64 = 0xff51_afd7_ed55_8ccd;
const M2: u64 = 0xc4ce_b9fe_1a85_ec53;

fn round(acc: u64, val: u64) -> u64 {
    acc.wrapping_add(val.wrapping_mul(P2))
        .rotate_left(31)
        .wrapping_mul(P1)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut bytes = [0u8; 64];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        bytes[i] = (x ^ input2) as u8;
        i += 1;
    }
    let len = (input2 % 65) as usize;
    let seed = ((input1 as u64) << 32) | (input2 as u64);

    let mut h;
    let mut pos = 0usize;
    if len >= 32 {
        let mut v1 = seed.wrapping_add(P1).wrapping_add(P2);
        let mut v2 = seed.wrapping_add(P2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(P1);
        while pos + 32 <= len {
            let mut lane = [0u64; 4];
            let mut l = 0usize;
            while l < 4 {
                let mut w = 0u64;
                let mut b = 0usize;
                while b < 8 {
                    w |= (bytes[pos + 8 * l + b] as u64) << (8 * b);
                    b += 1;
                }
                lane[l] = w;
                l += 1;
            }
            v1 = round(v1, lane[0]);
            v2 = round(v2, lane[1]);
            v3 = round(v3, lane[2]);
            v4 = round(v4, lane[3]);
            pos += 32;
        }
        h = v1
            .rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18));
        h = (h ^ round(0, v1)).wrapping_mul(P1).wrapping_add(P4);
        h = (h ^ round(0, v2)).wrapping_mul(P1).wrapping_add(P4);
        h = (h ^ round(0, v3)).wrapping_mul(P1).wrapping_add(P4);
        h = (h ^ round(0, v4)).wrapping_mul(P1).wrapping_add(P4);
    } else {
        h = seed.wrapping_add(P5);
    }
    h = h.wrapping_add(len as u64);

    // Eight-byte tail chunks.
    while pos + 8 <= len {
        let mut w = 0u64;
        let mut b = 0usize;
        while b < 8 {
            w |= (bytes[pos + b] as u64) << (8 * b);
            b += 1;
        }
        h ^= round(0, w);
        h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        pos += 8;
    }
    // Four-byte tail chunk.
    if pos + 4 <= len {
        let mut w = 0u64;
        let mut b = 0usize;
        while b < 4 {
            w |= (bytes[pos + b] as u64) << (8 * b);
            b += 1;
        }
        h ^= w.wrapping_mul(P1);
        h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        pos += 4;
    }
    // Remaining bytes.
    while pos < len {
        h ^= (bytes[pos] as u64).wrapping_mul(P5);
        h = h.rotate_left(11).wrapping_mul(P1);
        pos += 1;
    }

    // xxHash64 avalanche.
    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^= h >> 32;

    // MurmurHash3-x64 finalizer over the digest.
    let mut k = h ^ seed;
    k ^= k >> 33;
    k = k.wrapping_mul(M1);
    k ^= k >> 33;
    k = k.wrapping_mul(M2);
    k ^= k >> 33;

    ((h ^ k) as u32) ^ (((h ^ k) >> 32) as u32)
}
