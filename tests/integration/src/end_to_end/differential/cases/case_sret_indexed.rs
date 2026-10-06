// packed_fields x ret_area (campaign 14): helpers return `repr(C, packed)`
// records (a u128 at byte offset 1, a u64 at offset 3) BY VALUE straight
// into runtime-indexed elements of stack arrays of such records, so the
// return-area pointer is a computed unaligned address; the elements are
// read back at other runtime indexes and through a slice helper.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct P1 {
    tag: u8,
    wide: u128,
    tail: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct P3 {
    a: u8,
    b: u16,
    v: u64,
    c: u32,
}

#[inline(never)]
fn make1(x: u64, y: u64, t: u8) -> P1 {
    P1 {
        tag: t,
        wide: ((x as u128) << 64) | y as u128,
        tail: (x ^ y) as u16,
    }
}

#[inline(never)]
fn make3(v: u64, c: u32) -> P3 {
    P3 {
        a: v as u8,
        b: (v >> 8) as u16,
        v: v.rotate_left(c & 63),
        c,
    }
}

#[inline(never)]
fn fold(ps: &[P1], qs: &[P3]) -> u64 {
    let mut acc = 0u64;
    for p in ps {
        let w = p.wide;
        let t = p.tail;
        acc = acc.rotate_left(5) ^ (w as u64) ^ ((w >> 64) as u64) ^ (t as u64) ^ (p.tag as u64) << 40;
    }
    for q in qs {
        let v = q.v;
        let b = q.b;
        let c = q.c;
        acc = acc.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ v ^ (b as u64) << 16 ^ (c as u64) << 32 ^ q.a as u64;
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut ps = [make1(0, 0, 0); 5];
    let mut qs = [make3(0, 0); 4];
    let mut i = 0u32;
    while i < 5 {
        let at = ((input1 >> (i * 3)) % 5) as usize;
        ps[at] = make1(x.rotate_left(i * 7), y ^ i as u64, i as u8);
        if i < 4 {
            let bt = ((input2 >> (i * 5)) % 4) as usize;
            qs[bt] = make3(x ^ y.rotate_left(i * 11), input2.wrapping_add(i));
        }
        i += 1;
    }
    let p = ps[(input2 % 5) as usize];
    let q = qs[(input1 % 4) as usize];
    let pw = p.wide;
    let pt = p.tail;
    let qv = q.v;
    let qc = q.c;
    let qb = q.b;
    let f = fold(&ps[(input1 % 3) as usize..], &qs[..(input2 % 4 + 1) as usize]);
    (pw as u32) ^ ((pw >> 64) as u32) ^ (pt as u32) ^ (p.tag as u32) << 24 ^ (qv as u32) ^ qc.rotate_left(3) ^ (qb as u32) ^ (q.a as u32) << 16 ^ (f as u32) ^ ((f >> 32) as u32)
}
