// Sponge hash over eight 64-bit lanes (campaign 21, cliff shape: sequential
// loops sharing constants).  The absorb loop XORs message blocks into the
// rate lanes and permutes; the squeeze loop then extracts four output words
// with the SAME eight rotation offsets the permutation uses — so every
// rotation constant is live from the state initialisation, across the absorb
// loop, and into the squeeze loop.  The eight lanes are the u64 state; no
// 128-bit arithmetic is involved.
const R0: u32 = 5;
const R1: u32 = 13;
const R2: u32 = 21;
const R3: u32 = 29;
const R4: u32 = 37;
const R5: u32 = 45;
const R6: u32 = 53;
const R7: u32 = 61;

const RC: [u64; 6] = [
    0x0000_0000_0000_008b,
    0x0000_0000_8000_8089,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8080,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8008,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Message: up to 12 blocks of two lanes each.
    let mut msg = [0u64; 24];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 24 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        msg[i] = ((x as u64) << 32) ^ (x.rotate_left(11) as u64) ^ (input2 as u64);
        i += 1;
    }
    let blocks = 1 + (input2 % 12) as usize;

    let mut s0 = (input1 as u64).rotate_left(R0) | 1;
    let mut s1 = (input2 as u64).rotate_left(R1) | 2;
    let mut s2 = RC[0].rotate_left(R2);
    let mut s3 = RC[1].rotate_left(R3);
    let mut s4 = RC[2].rotate_left(R4);
    let mut s5 = RC[3].rotate_left(R5);
    let mut s6 = RC[4].rotate_left(R6);
    let mut s7 = RC[5].rotate_left(R7);

    // Absorb.
    let mut b = 0usize;
    while b < blocks {
        s0 ^= msg[2 * b];
        s1 ^= msg[2 * b + 1];
        let mut r = 0usize;
        while r < 6 {
            // theta-like column mix
            let c0 = s0 ^ s2 ^ s4 ^ s6;
            let c1 = s1 ^ s3 ^ s5 ^ s7;
            let d0 = c1 ^ c0.rotate_left(R0);
            let d1 = c0 ^ c1.rotate_left(R1);
            s0 ^= d0;
            s2 ^= d0;
            s4 ^= d0;
            s6 ^= d0;
            s1 ^= d1;
            s3 ^= d1;
            s5 ^= d1;
            s7 ^= d1;
            // rho-like lane rotations
            s1 = s1.rotate_left(R2);
            s2 = s2.rotate_left(R3);
            s3 = s3.rotate_left(R4);
            s4 = s4.rotate_left(R5);
            s5 = s5.rotate_left(R6);
            s6 = s6.rotate_left(R7);
            s7 = s7.rotate_left(R0);
            // chi-like nonlinearity
            let t0 = s0 ^ (!s1 & s2);
            let t1 = s1 ^ (!s2 & s3);
            let t2 = s2 ^ (!s3 & s4);
            let t3 = s3 ^ (!s4 & s5);
            let t4 = s4 ^ (!s5 & s6);
            let t5 = s5 ^ (!s6 & s7);
            let t6 = s6 ^ (!s7 & s0);
            let t7 = s7 ^ (!s0 & s1);
            s0 = t0 ^ RC[r];
            s1 = t1;
            s2 = t2;
            s3 = t3;
            s4 = t4;
            s5 = t5;
            s6 = t6;
            s7 = t7;
            r += 1;
        }
        b += 1;
    }

    // Squeeze: the same rotation offsets, one output word per iteration.
    let mut out = 0u64;
    let mut o = 0usize;
    while o < 4 {
        out ^= s0.rotate_left(R0) ^ s1.rotate_left(R1);
        out = out.wrapping_add(s2.rotate_left(R2) ^ s3.rotate_left(R3));
        out ^= s4.rotate_left(R4).wrapping_sub(s5.rotate_left(R5));
        out = out.wrapping_mul(s6.rotate_left(R6) | 1) ^ s7.rotate_left(R7);
        // One more permutation round between squeezes.
        let c0 = s0 ^ s2 ^ s4 ^ s6;
        let c1 = s1 ^ s3 ^ s5 ^ s7;
        s0 ^= c1 ^ c0.rotate_left(R0);
        s1 ^= c0 ^ c1.rotate_left(R1);
        s2 = s2.rotate_left(R3) ^ s0;
        s3 = s3.rotate_left(R4) ^ s1;
        s4 = s4.rotate_left(R5) ^ s2;
        s5 = s5.rotate_left(R6) ^ s3;
        s6 = s6.rotate_left(R7) ^ s4;
        s7 = s7.rotate_left(R2) ^ s5;
        o += 1;
    }

    (out as u32) ^ ((out >> 32) as u32)
}
