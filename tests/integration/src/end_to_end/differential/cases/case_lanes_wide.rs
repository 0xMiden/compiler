// C13 packed lanes x C12 wide arithmetic (campaign 14): a 35-byte
// `#[repr(C, packed)]` record with u8/u16/u32/u64/u128/i16/i8/u8 fields
// lives in a runtime-indexed `[Rec; 4]`, so every field is loaded and stored
// at all four byte offsets within an element across the index (35k + field
// offset mod 4 cycles through 0/3/2/1). The unaligned loads feed a 4-limb
// u128 product (mul_wide chain), dynamic-count 128-bit shifts (libcalls)
// with counts from the lanes, an i128 `checked_div` whose divisor reaches 0
// and -1 through the i8 lane, sext/trunc chains from the i8/i16 lanes, and a
// u32 -> u64 -> u128 width tree with a rippling carry; the wide results are
// stored back through the neighbouring record's unaligned fields before
// every field of every record is folded.
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Rec {
    a: u8,
    b: u16,
    c: u32,
    d: u64,
    e: u128,
    f: i16,
    g: i8,
    h: u8,
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let zero = Rec {
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        e: 0,
        f: 0,
        g: 0,
        h: 0,
    };
    let mut recs = [zero; 4];
    let mut k = 0usize;
    while k < 4 {
        let v = input1.wrapping_mul(0x0101_0101 + k as u32) ^ input2.rotate_left(k as u32 * 7);
        let d = (((v as u64) << 32) | input2 as u64).wrapping_mul(0x2545_f491_4f6c_dd1d);
        recs[k].a = v as u8;
        recs[k].b = (v >> 8) as u16;
        recs[k].c = v.wrapping_mul(0x9e37_79b9);
        recs[k].d = d;
        recs[k].e = ((d as u128) << 64) | ((v as u128) << 32) | input1 as u128;
        recs[k].f = (v >> 16) as i16;
        recs[k].g = (v >> 24) as i8;
        recs[k].h = (v >> 4) as u8;
        k += 1;
    }
    let i = (input2 & 3) as usize;
    let r = recs[i];
    let (a, b, c, d, e, f, g, h) = (r.a, r.b, r.c, r.d, r.e, r.f, r.g, r.h);
    // 4-limb product from the unaligned u128 and u64.
    let p = e.wrapping_mul((d as u128) | 1);
    // Dynamic-count 128-bit shifts with counts from the lanes.
    let sh = (e >> (c & 127)) ^ (p << (b as u32 & 127));
    // i128 checked division: the i8 lane gives divisors 0 and -1.
    let q = (e as i128).checked_div(g as i128).map_or(0x1111_2222_3333_4444, |q| q as u128);
    // sext/trunc chains from the i8/i16 lanes through 32/64/128 bits.
    let s = ((g as i32) as i64 as u64).rotate_left(a as u32 & 63)
        ^ (((f as i64) as i128 as u128) >> 3) as u64;
    // u32 -> u64 -> u128 width tree with a carry across the 64-bit limb.
    let t: u128 = ((c as u64 | 0xffff_ffff_0000_0000) as u128)
        .wrapping_add(((d as u128) << 32) | (h as u128));
    let acc128 = p ^ sh.rotate_left(17) ^ q.wrapping_add(t) ^ ((s as u128) << 64);
    // Store the wide results back through the neighbour's unaligned fields.
    let n = (i + 1) & 3;
    recs[n].e = acc128;
    recs[n].d = ((acc128 >> 64) as u64) ^ s;
    recs[n].c = (acc128 >> 32) as u32;
    recs[n].b = (acc128 >> 96) as u16;
    recs[n].f = (sh >> 64) as i16;
    recs[n].g = (q >> 5) as i8;
    // Fold every field of every record through the permuted index.
    let mut acc = 0u32;
    let mut m = 0usize;
    while m < 4 {
        let idx = (m + i) & 3;
        let rr = recs[idx];
        let (ra, rb, rc, rd, re, rf, rg, rh) = (rr.a, rr.b, rr.c, rr.d, rr.e, rr.f, rr.g, rr.h);
        acc = acc.rotate_left(5) ^ rc ^ (rd as u32) ^ ((rd >> 32) as u32);
        acc = acc
            .wrapping_add(re as u32)
            .wrapping_add((re >> 32) as u32)
            .wrapping_add((re >> 64) as u32)
            .wrapping_add((re >> 96) as u32);
        acc = acc
            .wrapping_add(rb as u32)
            .wrapping_add(rf as i32 as u32)
            .wrapping_add(rg as i32 as u32)
            .wrapping_add(ra as u32 | ((rh as u32) << 8));
        m += 1;
    }
    acc
}
