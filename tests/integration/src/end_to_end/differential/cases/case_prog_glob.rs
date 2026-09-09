// Iterative glob matcher (campaign 21, cliff shape: return-heavy inner loop
// nested in an outer loop).  Three patterns built from the inputs (`*`, `?`,
// `[a-z]` classes and literals) are matched against a 32-byte text with the
// classic backtracking loop; the loop returns early on a match, on a
// malformed class, on an exhausted backtracking budget and on a text that
// runs out, and five u64 matcher statistics — a position hash, a star mask, a
// class fold, a wildcard counter and a step checksum — are combined in ONE
// expression at every one of those exits and folded into the result after the
// outer loop.  The rotate constants 5, 21, 33 and 47 are shared by the text
// builder, the matching loop and the final fold.
const RA: u32 = 5;
const RB: u32 = 21;
const RC: u32 = 33;
const RD: u32 = 47;

const K: u64 = 0x9e37_79b9_7f4a_7c15;

fn text_of(seed: u32) -> [u8; 32] {
    let mut t = [0u8; 32];
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < 32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        t[i] = b'a' + ((x >> 7) % 6) as u8;
        i += 1;
    }
    t
}

fn pattern_of(seed: u32, which: u32) -> ([u8; 16], usize) {
    let mut p = [0u8; 16];
    let mut n = 0usize;
    let mut x = seed.rotate_left(which * 8) | 1;
    let mut i = 0u32;
    while i < 7 && n + 4 <= 16 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match (x >> 9) % 5 {
            0 => {
                p[n] = b'*';
                n += 1;
            }
            1 => {
                p[n] = b'?';
                n += 1;
            }
            2 => {
                p[n] = b'[';
                p[n + 1] = b'a';
                p[n + 2] = b'-';
                p[n + 3] = b'c';
                n += 4;
                if which == 2 {
                    // A class the caller mis-typed: no range separator.
                    p[n - 2] = b'+';
                }
            }
            _ => {
                p[n] = b'a' + ((x >> 3) % 6) as u8;
                n += 1;
            }
        }
        i += 1;
    }
    (p, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let text = text_of(input1);
    let tlen = 24 + (input2 % 9) as usize;

    let mut poshash = (input1 as u64).rotate_left(RA);
    let mut stars = (input2 as u64).rotate_left(RB);
    let mut classes = K.rotate_left(RC);
    let mut wilds = 0u64;
    let mut steps = K.rotate_left(RD);

    let mut which = 0u32;
    while which < 3 {
        let (pat, plen) = pattern_of(input2 ^ input1.rotate_left(which), which);
        let mut pi = 0usize;
        let mut ti = 0usize;
        let mut star_pi = usize::MAX;
        let mut star_ti = 0usize;
        let mut budget = 64u32;
        loop {
            steps = steps.wrapping_mul(K) ^ (ti as u64).rotate_left(RA);
            if budget == 0 {
                let probe = (poshash ^ stars.rotate_left(RA))
                    .wrapping_add(classes ^ wilds.rotate_left(RB))
                    .wrapping_mul(steps | 1)
                    ^ (pi as u64).rotate_left(RC);
                return (1u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }
            budget -= 1;
            if pi < plen && pat[pi] == b'[' {
                if pi + 3 >= plen || pat[pi + 2] != b'-' {
                    // Malformed character class.
                    let probe = (poshash.rotate_left(RB) ^ stars)
                        .wrapping_sub(classes.wrapping_add(wilds))
                        .wrapping_mul(steps | 3)
                        ^ (plen as u64).rotate_left(RD);
                    return (2u32 << 28)
                        | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
                }
                if ti < tlen && text[ti] >= pat[pi + 1] && text[ti] <= pat[pi + 3] {
                    classes ^= (text[ti] as u64).rotate_left(RC);
                    pi += 4;
                    ti += 1;
                    continue;
                }
            } else if pi < plen && (pat[pi] == b'?' || (ti < tlen && pat[pi] == text[ti])) {
                if pat[pi] == b'?' {
                    wilds = wilds.wrapping_add(1);
                }
                if ti >= tlen {
                    // The pattern still wants a character the text does not have.
                    let probe = (poshash ^ stars.rotate_left(RC))
                        .wrapping_add(classes.rotate_left(RA) ^ wilds)
                        .wrapping_mul(steps | 5)
                        ^ (ti as u64).rotate_left(RB);
                    return (3u32 << 28)
                        | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
                }
                poshash = poshash.wrapping_add((text[ti] as u64).rotate_left(RA));
                pi += 1;
                ti += 1;
                continue;
            } else if pi < plen && pat[pi] == b'*' {
                stars ^= (pi as u64).rotate_left(RB).wrapping_mul(K);
                star_pi = pi;
                pi += 1;
                star_ti = ti;
                continue;
            }
            if star_pi != usize::MAX && star_ti < tlen {
                star_ti += 1;
                ti = star_ti;
                pi = star_pi + 1;
                continue;
            }
            if pi >= plen && ti >= tlen {
                let probe = (poshash.rotate_left(RD) ^ stars)
                    .wrapping_add(classes ^ wilds.rotate_left(RC))
                    .wrapping_mul(steps | 7)
                    ^ (which as u64).rotate_left(RA);
                return (4u32 << 28) | (((probe as u32) ^ ((probe >> 32) as u32)) & 0x0fff_ffff);
            }
            break;
        }
        poshash = poshash.rotate_left(RA) ^ (which as u64).wrapping_mul(K);
        stars = stars.wrapping_add(stars.rotate_left(RB));
        which += 1;
    }

    let mut out = poshash.rotate_left(RA) ^ stars.rotate_left(RB);
    out = out.wrapping_add(classes.rotate_left(RC));
    out ^= wilds.rotate_left(RD);
    out = out.wrapping_mul(steps | 1);
    (5u32 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}
