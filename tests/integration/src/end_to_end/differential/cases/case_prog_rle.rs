// Run-length + delta encoder with an escape arm (campaign 21, cliff shape:
// asymmetric diamond in a hot loop).  A 64-sample signal is encoded into a
// token buffer: the common arm extends the current run and touches nothing
// but the run registers, while the escape arm — a literal block that cannot
// be run-encoded — reads all five u64 encoder statistics (dictionary hash,
// entropy fold, delta accumulator, escape mask and a checksum), rewrites
// them and emits the literals.  The statistics are seeded before the loop
// and folded into the result after it, and the rotate constants 5, 15, 25,
// 35 and 45 are shared by the signal builder, the escape arm and the fold.
const E0: u32 = 5;
const E1: u32 = 15;
const E2: u32 = 25;
const E3: u32 = 35;
const E4: u32 = 45;

const HK: u64 = 0xff51_afd7_ed55_8ccd;

fn signal_of(seed: u32, flat: u32) -> [u8; 64] {
    let mut s = [0u8; 64];
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < 64 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        // `flat` controls how run-heavy the signal is.
        s[i] = if (x >> 9) % 8 < flat { s[i.saturating_sub(1)] } else { (x >> 3) as u8 };
        i += 1;
    }
    s
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let sig = signal_of(input1, input2 % 8);

    let mut dict = HK.rotate_left(E0) ^ (input1 as u64);
    let mut entropy = HK.rotate_left(E1) ^ (input2 as u64);
    let mut delta = HK.rotate_left(E2);
    let mut escapes = 0u64;
    let mut check = HK.rotate_left(E3);

    let mut tokens = [0u8; 96];
    let mut nt = 0usize;
    let mut run = 1u32;
    let mut prev = sig[0];
    let mut i = 1usize;
    while i < 64 {
        let cur = sig[i];
        if cur == prev && run < 63 {
            // Common arm: extend the run.
            run += 1;
        } else if run >= 3 {
            // Emit a run token.
            if nt + 2 <= 96 {
                tokens[nt] = 0x80 | (run as u8);
                tokens[nt + 1] = prev;
                nt += 2;
            }
            run = 1;
            prev = cur;
        } else {
            // Escape arm: a literal block, and the statistics that pay for it.
            let lit = prev as u64;
            dict = (dict ^ lit.rotate_left(E0)).wrapping_mul(HK);
            entropy = entropy
                .wrapping_add(lit.rotate_left(E1))
                .wrapping_mul(dict | 1)
                ^ delta.rotate_left(E2);
            delta = delta
                .wrapping_sub((cur as u64).wrapping_sub(lit).rotate_left(E3))
                ^ entropy.rotate_left(E4);
            escapes = escapes.wrapping_add(1) ^ check.rotate_left(E0);
            check = (check ^ dict.rotate_left(E1))
                .wrapping_add(entropy.rotate_left(E2))
                .wrapping_mul(delta | 3);
            let mut r = 0u32;
            while r < run && nt < 96 {
                tokens[nt] = prev;
                nt += 1;
                r += 1;
            }
            run = 1;
            prev = cur;
        }
        i += 1;
    }
    if nt + 2 <= 96 {
        tokens[nt] = 0x80 | (run as u8);
        tokens[nt + 1] = prev;
        nt += 2;
    }

    // Verification pass over the token buffer.
    let mut sum = 0u64;
    let mut j = 0usize;
    while j < nt {
        sum = sum.wrapping_mul(HK) ^ (tokens[j] as u64).rotate_left(E0);
        j += 1;
    }

    let mut out = dict.rotate_left(E0) ^ entropy.rotate_left(E1);
    out = out.wrapping_add(delta.rotate_left(E2));
    out ^= escapes.rotate_left(E3);
    out = out.wrapping_mul(check | 1) ^ sum.rotate_left(E4);
    out ^= (nt as u64).wrapping_mul(HK);
    (out as u32) ^ ((out >> 32) as u32)
}
