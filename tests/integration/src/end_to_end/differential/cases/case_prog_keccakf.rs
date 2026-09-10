// Keccak-f[1600] reduced to 12 rounds (the TurboSHAKE / KangarooTwelve round
// reduction) over a 25-lane u64 state held in a stack array, absorbing one or
// two 17-lane rate blocks, padding, and squeezing two blocks.  The rho step is
// unrolled with the real 24 rotation offsets the way the reference C
// implementations write it, so TWENTY-FOUR distinct rotate constants live
// across the 12-trip round loop, and the permutation itself is called from the
// absorb loop, the padding step and the squeeze loop.
const RC: [u64; 12] = [
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008a,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000a,
    0x000000008000808b,
    0x800000000000008b,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
];

fn keccak_f12(a: &mut [u64; 25]) {
    let mut round = 0usize;
    while round < 12 {
        // theta
        let mut c = [0u64; 5];
        let mut x = 0usize;
        while x < 5 {
            c[x] = a[x] ^ a[x + 5] ^ a[x + 10] ^ a[x + 15] ^ a[x + 20];
            x += 1;
        }
        x = 0;
        while x < 5 {
            let d = c[(x + 4) % 5] ^ c[(x + 1) % 5].rotate_left(1);
            let mut y = 0usize;
            while y < 25 {
                a[x + y] ^= d;
                y += 5;
            }
            x += 1;
        }
        // rho + pi, unrolled with the real offsets
        let mut t = a[1];
        let bc = a[10];
        a[10] = t.rotate_left(1);
        t = bc;
        let bc = a[7];
        a[7] = t.rotate_left(3);
        t = bc;
        let bc = a[11];
        a[11] = t.rotate_left(6);
        t = bc;
        let bc = a[17];
        a[17] = t.rotate_left(10);
        t = bc;
        let bc = a[18];
        a[18] = t.rotate_left(15);
        t = bc;
        let bc = a[3];
        a[3] = t.rotate_left(21);
        t = bc;
        let bc = a[5];
        a[5] = t.rotate_left(28);
        t = bc;
        let bc = a[16];
        a[16] = t.rotate_left(36);
        t = bc;
        let bc = a[8];
        a[8] = t.rotate_left(45);
        t = bc;
        let bc = a[21];
        a[21] = t.rotate_left(55);
        t = bc;
        let bc = a[24];
        a[24] = t.rotate_left(2);
        t = bc;
        let bc = a[4];
        a[4] = t.rotate_left(14);
        t = bc;
        let bc = a[15];
        a[15] = t.rotate_left(27);
        t = bc;
        let bc = a[23];
        a[23] = t.rotate_left(41);
        t = bc;
        let bc = a[19];
        a[19] = t.rotate_left(56);
        t = bc;
        let bc = a[13];
        a[13] = t.rotate_left(8);
        t = bc;
        let bc = a[12];
        a[12] = t.rotate_left(25);
        t = bc;
        let bc = a[2];
        a[2] = t.rotate_left(43);
        t = bc;
        let bc = a[20];
        a[20] = t.rotate_left(62);
        t = bc;
        let bc = a[14];
        a[14] = t.rotate_left(18);
        t = bc;
        let bc = a[22];
        a[22] = t.rotate_left(39);
        t = bc;
        let bc = a[9];
        a[9] = t.rotate_left(61);
        t = bc;
        let bc = a[6];
        a[6] = t.rotate_left(20);
        t = bc;
        a[1] = t.rotate_left(44);
        // chi
        let mut y = 0usize;
        while y < 25 {
            let b0 = a[y];
            let b1 = a[y + 1];
            let b2 = a[y + 2];
            let b3 = a[y + 3];
            let b4 = a[y + 4];
            a[y] = b0 ^ ((!b1) & b2);
            a[y + 1] = b1 ^ ((!b2) & b3);
            a[y + 2] = b2 ^ ((!b3) & b4);
            a[y + 3] = b3 ^ ((!b4) & b0);
            a[y + 4] = b4 ^ ((!b0) & b1);
            y += 5;
        }
        // iota
        a[0] ^= RC[round];
        round += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Message lanes derived from the inputs by a xorshift.
    let mut msg = [0u64; 34];
    let mut x = (input1 as u64) | 1;
    let mut i = 0usize;
    while i < 34 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        msg[i] = x ^ (input2 as u64);
        i += 1;
    }

    let mut st = [0u64; 25];
    let blocks = 1 + (input2 % 2) as usize;
    let mut b = 0usize;
    while b < blocks {
        let mut l = 0usize;
        while l < 17 {
            st[l] ^= msg[17 * b + l];
            l += 1;
        }
        keccak_f12(&mut st);
        b += 1;
    }

    // Padding: the domain byte at a runtime lane, the final bit in the last
    // rate lane.
    st[(input1 % 17) as usize] ^= 0x1f;
    st[16] ^= 0x8000_0000_0000_0000;
    keccak_f12(&mut st);

    // Squeeze two 17-lane blocks with one permutation in between.
    let mut out = 0u64;
    let mut s = 0usize;
    while s < 2 {
        let mut l = 0usize;
        while l < 17 {
            out = out.rotate_left(7) ^ st[l];
            l += 1;
        }
        if s == 0 {
            keccak_f12(&mut st);
        }
        s += 1;
    }

    (out as u32) ^ ((out >> 32) as u32)
}
