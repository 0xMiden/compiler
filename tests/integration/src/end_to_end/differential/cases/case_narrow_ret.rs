// narrow64 x ret_area x lane_bytes (campaign 14): helpers return records
// with u8 / u16 / u32 / u64 fields by value (return-area stores of
// truncated i64 values: i64.store8 / store16 / store32 into the sret area),
// a `repr(C)` record with padding and a `repr(C, packed)` record with its
// u64 at byte offset 1, the results are collected into stack arrays filled
// in a loop, read back at runtime indexes (sub-word loads at every lane)
// and folded by a helper taking slices of both arrays.
#[repr(C)]
#[derive(Clone, Copy)]
struct Rec {
    a: u8,
    b: u16,
    c: u32,
    d: u64,
    e: u8,
    f: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct PRec {
    a: u8,
    d: u64,
    b: u16,
    c: u32,
    e: u8,
}

#[inline(never)]
fn make(x: u64, k: u32) -> Rec {
    Rec {
        a: x as u8,
        b: (x >> 8) as u16,
        c: (x >> 24) as u32,
        d: x.rotate_left(k & 63),
        e: (x >> 56) as u8,
        f: (x >> 40) as u16,
    }
}

#[inline(never)]
fn make_packed(x: u64, y: u64) -> PRec {
    PRec {
        a: y as u8,
        d: x ^ y,
        b: (y >> 16) as u16,
        c: (x >> 32) as u32,
        e: (y >> 48) as u8,
    }
}

#[inline(never)]
fn fold(rs: &[Rec], ps: &[PRec]) -> u64 {
    let mut acc = 0u64;
    for r in rs {
        acc = acc.wrapping_mul(31).wrapping_add(
            r.a as u64 ^ ((r.b as u64) << 8) ^ ((r.c as u64) << 24) ^ r.d ^ ((r.e as u64) << 56) ^ ((r.f as u64) << 40),
        );
    }
    for p in ps {
        let d = p.d;
        let c = p.c;
        let b = p.b;
        acc = acc.rotate_left(9) ^ d ^ (c as u64) ^ ((b as u64) << 3) ^ (p.a as u64) ^ ((p.e as u64) << 11);
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = (input1 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ input2 as u64;
    let y = (input2 as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ input1 as u64;
    let mut recs = [make(x, 0); 4];
    let mut packed = [make_packed(y, x); 3];
    let mut i = 0u32;
    while i < 4 {
        recs[i as usize] = make(x.rotate_left(i * 13) ^ y, input2.wrapping_add(i));
        if i < 3 {
            packed[i as usize] = make_packed(x.wrapping_add(i as u64), y.rotate_left(i * 5));
        }
        i += 1;
    }
    let ri = (input1 % 4) as usize;
    let pi = (input2 % 3) as usize;
    let r = recs[ri];
    let p = packed[pi];
    let pd = p.d;
    let pc = p.c;
    let pb = p.b;
    let pick = (r.a as u32) ^ ((r.b as u32) << 8) ^ r.c ^ (r.d as u32) ^ ((r.e as u32) << 24) ^ ((r.f as u32) << 16);
    let ppick = (pd as u32) ^ ((pd >> 32) as u32) ^ pc ^ (pb as u32) ^ (p.a as u32) ^ ((p.e as u32) << 8);
    let f = fold(&recs[..], &packed[pi..]);
    pick ^ ppick.rotate_left(7) ^ (f as u32) ^ ((f >> 32) as u32)
}
