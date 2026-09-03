// Sequence-alignment DPs (campaign 17, program 11): two byte strings of
// input-derived lengths (0..=32) over a four-letter alphabet, generated
// by an xorshift stream, compared by a Levenshtein edit distance, an LCS
// length and a Needleman-Wunsch alignment score (match +2, mismatch -1,
// gap -2, signed), each as a two-row DP on the stack, plus a Hamming
// distance over the common prefix; the distances, the last DP rows and
// the two strings are folded into the result.
static ALPHABET: [u8; 4] = *b"ACGT";

fn gen_str(buf: &mut [u8; 32], seed: u32, len: usize) {
    let mut x = seed | 1;
    let mut i = 0usize;
    while i < len {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        buf[i] = ALPHABET[((x >> 13) & 3) as usize];
        i += 1;
    }
}

fn levenshtein(a: &[u8], b: &[u8], last: &mut [u32; 33]) -> u32 {
    let mut prev = [0u32; 33];
    let mut cur = [0u32; 33];
    let mut j = 0usize;
    while j <= b.len() {
        prev[j] = j as u32;
        j += 1;
    }
    let mut i = 1usize;
    while i <= a.len() {
        cur[0] = i as u32;
        j = 1;
        while j <= b.len() {
            let sub = prev[j - 1] + (a[i - 1] != b[j - 1]) as u32;
            let del = prev[j] + 1;
            let ins = cur[j - 1] + 1;
            let mut m = sub;
            if del < m {
                m = del;
            }
            if ins < m {
                m = ins;
            }
            cur[j] = m;
            j += 1;
        }
        core::mem::swap(&mut prev, &mut cur);
        i += 1;
    }
    *last = prev;
    prev[b.len()]
}

fn lcs(a: &[u8], b: &[u8]) -> u32 {
    let mut prev = [0u32; 33];
    let mut cur = [0u32; 33];
    let mut i = 1usize;
    while i <= a.len() {
        let mut j = 1usize;
        while j <= b.len() {
            cur[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1] + 1
            } else if prev[j] >= cur[j - 1] {
                prev[j]
            } else {
                cur[j - 1]
            };
            j += 1;
        }
        core::mem::swap(&mut prev, &mut cur);
        i += 1;
    }
    prev[b.len()]
}

fn needleman(a: &[u8], b: &[u8]) -> i32 {
    let mut prev = [0i32; 33];
    let mut cur = [0i32; 33];
    let mut j = 0usize;
    while j <= b.len() {
        prev[j] = -2 * j as i32;
        j += 1;
    }
    let mut i = 1usize;
    while i <= a.len() {
        cur[0] = -2 * i as i32;
        j = 1;
        while j <= b.len() {
            let s = if a[i - 1] == b[j - 1] { 2 } else { -1 };
            let diag = prev[j - 1] + s;
            let up = prev[j] - 2;
            let left = cur[j - 1] - 2;
            let mut m = diag;
            if up > m {
                m = up;
            }
            if left > m {
                m = left;
            }
            cur[j] = m;
            j += 1;
        }
        core::mem::swap(&mut prev, &mut cur);
        i += 1;
    }
    prev[b.len()]
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let la = (input1 % 33) as usize;
    let lb = (input2 % 33) as usize;
    let mut sa = [0u8; 32];
    let mut sb = [0u8; 32];
    gen_str(&mut sa, input1 ^ 0x9e37_79b9, la);
    gen_str(&mut sb, input2 ^ 0x9e37_79b9, lb);
    let a = &sa[..la];
    let b = &sb[..lb];
    let mut last = [0u32; 33];
    let lev = levenshtein(a, b, &mut last);
    let lev_rev = levenshtein(b, a, &mut [0u32; 33]);
    let l = lcs(a, b);
    let nw = needleman(a, b);
    let mut ham = 0u32;
    let mut i = 0usize;
    let common = if la < lb { la } else { lb };
    while i < common {
        ham += (a[i] != b[i]) as u32;
        i += 1;
    }
    // Sanity relations folded as flags: symmetry, lcs bound, lev bound.
    let flags = (lev == lev_rev) as u32
        | ((l as usize <= common) as u32) << 1
        | ((lev as usize >= (la as isize - lb as isize).unsigned_abs()) as u32) << 2
        | ((lev <= ham + (la as isize - lb as isize).unsigned_abs() as u32) as u32) << 3;
    let mut acc = lev << 8 ^ l << 16 ^ (nw as u32) << 20 ^ ham ^ flags << 28;
    i = 0;
    while i <= lb {
        acc = acc.rotate_left(5) ^ last[i];
        i += 1;
    }
    i = 0;
    while i < 32 {
        acc = acc.rotate_left(3) ^ (sa[i] as u32) ^ (sb[i] as u32) << 8;
        i += 1;
    }
    acc
}
