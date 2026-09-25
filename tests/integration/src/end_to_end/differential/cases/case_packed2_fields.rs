// `#[repr(C, packed(2))]` records: u32/u64/u128 fields with 2-byte
// alignment, so LLVM promises `align=1` (2 bytes) on every wide access —
// the byte-address-space branch of `enforce_alignment` with a `mod 2`
// assertion — and the fields sit at 2 mod 4 for odd record indexes
// (`load_sw`/`store_sw` at byte offset 2 crossing the element boundary,
// `load_dw`/`store_dw` at offset 2 across three elements, i64 pairs for the
// u128 field). The record array is 14 bytes per element inside an
// `align(64)` wrapper, indexed at runtime; every field is written, then
// read back through a permuted index and folded into the result.
#[derive(Clone, Copy)]
#[repr(C, packed(2))]
struct P2 {
    a: u16,
    b: u32,
    c: u64,
}

#[derive(Clone, Copy)]
#[repr(C, packed(2))]
struct P3 {
    tag: u16,
    w: u128,
    s: i32,
}

#[repr(C, align(64))]
struct Wrap {
    recs: [P2; 5],
    wide: [P3; 3],
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut o = Wrap {
        recs: [P2 { a: 0, b: 0, c: 0 }; 5],
        wide: [P3 { tag: 0, w: 0, s: 0 }; 3],
    };
    let mut k = 0usize;
    while k < 5 {
        let v = input1.wrapping_add((k as u32).wrapping_mul(0x9e37_79b9)) ^ input2.rotate_left(k as u32 * 5);
        o.recs[k].a = v as u16;
        o.recs[k].b = v.rotate_left(13);
        o.recs[k].c = ((v as u64) << 32 | input2 as u64).wrapping_mul(0x2545_f491_4f6c_dd1d);
        k += 1;
    }
    k = 0;
    while k < 3 {
        let v = input2.wrapping_mul(k as u32 + 7) ^ input1;
        o.wide[k].tag = (v >> 3) as u16;
        o.wide[k].w = ((v as u128) << 96) | ((input1 as u128) << 48) | (input2 as u128);
        o.wide[k].s = (v as i32).wrapping_neg();
        k += 1;
    }

    let i = (input2 % 5) as usize;
    let r = o.recs[i];
    let rb = r.b;
    let rc = r.c;
    let mut acc = rb ^ (rc as u32) ^ ((rc >> 32) as u32).rotate_left(7) ^ (r.a as u32);
    let n = (i + 2) % 5;
    o.recs[n].b = acc;
    o.recs[n].c = (acc as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let wi = (input1 % 3) as usize;
    let p = o.wide[wi];
    let pw = p.w;
    let ps = p.s;
    acc = acc.wrapping_add(pw as u32).wrapping_add((pw >> 32) as u32).wrapping_add((pw >> 64) as u32);
    acc = acc.wrapping_add((pw >> 96) as u32).wrapping_add(ps as u32).wrapping_add(p.tag as u32);
    o.wide[(wi + 1) % 3].w = (pw ^ (acc as u128)).wrapping_mul(0x1_0000_0000_0003);
    o.wide[(wi + 2) % 3].s = acc as i32 >> 4;

    let mut m = 0usize;
    while m < 5 {
        let q = o.recs[(m + i) % 5];
        let qb = q.b;
        let qc = q.c;
        acc = acc.rotate_left(3) ^ qb ^ (qc as u32) ^ ((qc >> 32) as u32) ^ ((q.a as u32) << 16);
        m += 1;
    }
    m = 0;
    while m < 3 {
        let q = o.wide[(m + wi) % 3];
        let qw = q.w;
        let qs = q.s;
        acc = acc.rotate_left(5) ^ (qw as u32) ^ ((qw >> 40) as u32) ^ ((qw >> 96) as u32) ^ (qs as u32) ^ (q.tag as u32);
        m += 1;
    }
    acc
}
