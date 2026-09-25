// SipHash-2-4 over a message of runtime length (up to 64 bytes) with the
// length byte in the final word, plus the standard two-key initialisation.
// FIVE distinct rotation constants (13, 16, 17, 21, 32) live in the four
// state words across the absorb loop, the two compression rounds per word,
// the tail word and the four finalization rounds.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Message bytes.
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

    let k0 = ((input1 as u64) << 32) | (input2 as u64);
    let k1 = ((input2 as u64) << 32) | (input1 as u64) | 1;

    let mut v0 = k0 ^ 0x736f_6d65_7073_6575;
    let mut v1 = k1 ^ 0x646f_7261_6e64_6f6d;
    let mut v2 = k0 ^ 0x6c79_6765_6e65_7261;
    let mut v3 = k1 ^ 0x7465_6462_7974_6573;

    let words = len / 8;
    let mut w = 0usize;
    while w < words {
        // Little-endian word load.
        let mut m = 0u64;
        let mut b = 0usize;
        while b < 8 {
            m |= (bytes[8 * w + b] as u64) << (8 * b);
            b += 1;
        }
        v3 ^= m;
        let mut r = 0usize;
        while r < 2 {
            v0 = v0.wrapping_add(v1);
            v1 = v1.rotate_left(13);
            v1 ^= v0;
            v0 = v0.rotate_left(32);
            v2 = v2.wrapping_add(v3);
            v3 = v3.rotate_left(16);
            v3 ^= v2;
            v0 = v0.wrapping_add(v3);
            v3 = v3.rotate_left(21);
            v3 ^= v0;
            v2 = v2.wrapping_add(v1);
            v1 = v1.rotate_left(17);
            v1 ^= v2;
            v2 = v2.rotate_left(32);
            r += 1;
        }
        v0 ^= m;
        w += 1;
    }

    // Tail word: the remaining bytes plus the length byte on top.
    let mut tail = ((len as u64) & 0xff) << 56;
    let rest = len % 8;
    let mut t = 0usize;
    while t < rest {
        tail |= (bytes[8 * words + t] as u64) << (8 * t);
        t += 1;
    }
    v3 ^= tail;
    let mut r = 0usize;
    while r < 2 {
        v0 = v0.wrapping_add(v1);
        v1 = v1.rotate_left(13);
        v1 ^= v0;
        v0 = v0.rotate_left(32);
        v2 = v2.wrapping_add(v3);
        v3 = v3.rotate_left(16);
        v3 ^= v2;
        v0 = v0.wrapping_add(v3);
        v3 = v3.rotate_left(21);
        v3 ^= v0;
        v2 = v2.wrapping_add(v1);
        v1 = v1.rotate_left(17);
        v1 ^= v2;
        v2 = v2.rotate_left(32);
        r += 1;
    }
    v0 ^= tail;

    // Finalization: four rounds with v2 ^= 0xff.
    v2 ^= 0xff;
    let mut f = 0usize;
    while f < 4 {
        v0 = v0.wrapping_add(v1);
        v1 = v1.rotate_left(13);
        v1 ^= v0;
        v0 = v0.rotate_left(32);
        v2 = v2.wrapping_add(v3);
        v3 = v3.rotate_left(16);
        v3 ^= v2;
        v0 = v0.wrapping_add(v3);
        v3 = v3.rotate_left(21);
        v3 ^= v0;
        v2 = v2.wrapping_add(v1);
        v1 = v1.rotate_left(17);
        v1 ^= v2;
        v2 = v2.rotate_left(32);
        f += 1;
    }

    let h = v0 ^ v1 ^ v2 ^ v3;
    (h as u32) ^ ((h >> 32) as u32)
}
