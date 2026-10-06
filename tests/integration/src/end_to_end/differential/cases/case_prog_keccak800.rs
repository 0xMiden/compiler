// Keccak-f[800] sponge (campaign 17, program 2): the 25-lane u32 Keccak
// permutation (theta / rho-pi / chi / iota, 22 rounds, .rodata round
// constants and rotation / lane-permutation tables) driven as a sponge of
// rate 16 lanes: absorb a 16-word block derived from the inputs by an
// xorshift generator, permute, absorb a second block that also depends on
// the first squeeze and the message length, permute, squeeze twice with a
// permutation in between. The whole state is folded, so every lane, every
// rotation count and every round constant matters.
static RC: [u32; 22] = [
    0x00000001, 0x00008082, 0x0000808a, 0x80008000, 0x0000808b, 0x80000001, 0x80008081, 0x00008009,
    0x0000008a, 0x00000088, 0x80008009, 0x8000000a, 0x8000808b, 0x0000008b, 0x00008089, 0x00008003,
    0x00008002, 0x00000080, 0x0000800a, 0x8000000a, 0x80008081, 0x00008080,
];

static ROTC: [u32; 24] = [
    1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
];

static PILN: [u8; 24] = [
    10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
];

// The Keccak-f[800] permutation on a 5x5 array of u32 lanes (row-major,
// `st[x + 5 * y]`).
fn keccak_f800(st: &mut [u32; 25]) {
    let mut round = 0usize;
    while round < 22 {
        // theta
        let mut bc = [0u32; 5];
        let mut i = 0usize;
        while i < 5 {
            bc[i] = st[i] ^ st[i + 5] ^ st[i + 10] ^ st[i + 15] ^ st[i + 20];
            i += 1;
        }
        i = 0;
        while i < 5 {
            let t = bc[(i + 4) % 5] ^ bc[(i + 1) % 5].rotate_left(1);
            let mut j = 0usize;
            while j < 25 {
                st[j + i] ^= t;
                j += 5;
            }
            i += 1;
        }
        // rho pi
        let mut t = st[1];
        i = 0;
        while i < 24 {
            let j = PILN[i] as usize;
            let tmp = st[j];
            st[j] = t.rotate_left(ROTC[i] % 32);
            t = tmp;
            i += 1;
        }
        // chi
        let mut j = 0usize;
        while j < 25 {
            i = 0;
            while i < 5 {
                bc[i] = st[j + i];
                i += 1;
            }
            i = 0;
            while i < 5 {
                st[j + i] ^= (!bc[(i + 1) % 5]) & bc[(i + 2) % 5];
                i += 1;
            }
            j += 5;
        }
        // iota
        st[0] ^= RC[round];
        round += 1;
    }
}

fn xorshift(x: &mut u32) -> u32 {
    let mut v = *x;
    v ^= v << 13;
    v ^= v >> 17;
    v ^= v << 5;
    *x = v;
    v
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut st = [0u32; 25];
    let mut seed = input1 ^ 0x9e37_79b9 ^ input2.rotate_left(16) | 1;
    // Absorb block 1: 16 generated words, the last one carrying the
    // message length in lanes (1..=16, input-derived).
    let mlen = (input2 % 16 + 1) as usize;
    let mut i = 0usize;
    while i < mlen {
        st[i] ^= xorshift(&mut seed);
        i += 1;
    }
    st[mlen - 1] ^= 0x06;
    st[15] ^= 0x8000_0000;
    keccak_f800(&mut st);
    let sq1 = st[0] ^ st[1].rotate_left(3) ^ st[2].rotate_left(11);
    // Absorb block 2: input words mixed with the first squeeze.
    i = 0;
    while i < 16 {
        let w = if i & 1 == 0 { input1 } else { input2 };
        st[i] ^= w.rotate_left((i as u32 * 7) & 31) ^ sq1.wrapping_mul(i as u32 | 1);
        i += 1;
    }
    keccak_f800(&mut st);
    let mut acc = sq1;
    i = 0;
    while i < 25 {
        acc = acc.rotate_left(7) ^ st[i];
        i += 1;
    }
    keccak_f800(&mut st);
    i = 0;
    while i < 8 {
        acc = acc.wrapping_add(st[i].rotate_left(i as u32 * 3));
        i += 1;
    }
    acc
}
