// Bounding sibling of `prog_histogram` (campaign 21): the same two-pass
// entropy-coder front-end — a histogram pass and an emission pass sharing the
// bucket-selection shift constants — with SIX distinct shift constants
// instead of eight.
const B0: u32 = 3;
const B1: u32 = 7;
const B2: u32 = 11;
const B3: u32 = 17;
const B4: u32 = 23;
const B5: u32 = 29;
const B6: u32 = B0;
const B7: u32 = B1;

const MULT: u64 = 0xc4ce_b9fe_1a85_ec53;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut data = [0u8; 96];
    let mut x = input1 | 1;
    let mut i = 0usize;
    while i < 96 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        data[i] = ((x >> 5) ^ (input2 >> (i as u32 & 15))) as u8;
        i += 1;
    }
    let len = 32 + (input2 % 65) as usize;

    // Statistics shared by both passes.
    let mut total = (input1 as u64).rotate_left(B0);
    let mut spread = (input2 as u64).rotate_left(B1);
    let mut hash = MULT.rotate_left(B2);
    let mut weight = MULT.rotate_left(B3);
    let mut peak = 0u64;
    let mut cost = MULT.rotate_left(B4);

    // Pass 1: histogram.
    let mut hist = [0u32; 16];
    let mut p = 0usize;
    while p < len {
        let b = data[p] as u64;
        let bucket = ((b.rotate_left(B0) ^ b.rotate_left(B1)) >> B2) & 15;
        hist[bucket as usize] += 1;
        total = total.wrapping_add(b.rotate_left(B3));
        spread ^= b.rotate_left(B4).wrapping_mul(MULT);
        hash = (hash ^ b).wrapping_mul(MULT).rotate_left(B5);
        weight = weight.wrapping_add(bucket.rotate_left(B6));
        if b > peak {
            peak = b;
        }
        cost = cost.wrapping_sub(b.rotate_left(B7));
        p += 1;
    }

    // Cumulative offsets over the same buckets.
    let mut offs = [0u32; 17];
    let mut c = 0u32;
    let mut k = 0usize;
    while k < 16 {
        offs[k] = c;
        c += hist[k];
        k += 1;
    }
    offs[16] = c;

    // Pass 2: emission, reusing every constant of pass 1.
    let mut codes = [0u32; 96];
    let mut emitted = 0usize;
    let mut q = 0usize;
    while q < len {
        let b = data[q] as u64;
        let bucket = ((b.rotate_left(B0) ^ b.rotate_left(B1)) >> B2) & 15;
        let base = offs[bucket as usize];
        let width = (hist[bucket as usize] | 1).ilog2() + 1;
        let code = (base << 8) | ((b as u32) & 0xff) | (width << 24);
        codes[emitted] = code;
        emitted += 1;

        let commit = (total ^ spread.rotate_left(B3))
            .wrapping_add(hash ^ weight.rotate_left(B4))
            .wrapping_mul(peak | 1)
            ^ cost.rotate_left(B5);
        total = total.wrapping_add(commit.rotate_left(B6));
        spread ^= (code as u64).rotate_left(B7);
        hash = hash.wrapping_mul(MULT) ^ commit.rotate_left(B0);
        weight = weight.wrapping_sub((width as u64).rotate_left(B1));
        cost ^= commit.rotate_left(B2);
        q += 1;
    }

    let mut out = total.rotate_left(B0) ^ spread.rotate_left(B1);
    out = out.wrapping_add(hash.rotate_left(B2));
    out ^= weight.rotate_left(B3);
    out = out.wrapping_sub(peak.rotate_left(B4));
    out ^= cost.rotate_left(B5);
    out = out.wrapping_mul((emitted as u64) | 1) ^ (offs[16] as u64).rotate_left(B6);
    out ^= (codes[emitted - 1] as u64).rotate_left(B7);
    (out as u32) ^ ((out >> 32) as u32)
}
