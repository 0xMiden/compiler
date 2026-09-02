// Packed and over-aligned records. A 21-byte `#[repr(C, packed)]` record
// with u8/u16/u32/u64/i16/i8/u16/u8 fields lives in a runtime-indexed
// `[Packed; 4]`, so every field lands at all four byte offsets within an
// element across the index (21k + field offset mod 4 cycles through
// 0..3): the field loads/stores are unaligned `i32.load16/load/i64.load`
// (byte-space `load_u16`/`load_sw`/`load_dw` at offsets 1/2/3, element
// straddling) and the stores are their `store_*` twins. The array sits in
// an `align(32)` wrapper next to a `#[repr(C)]` record with padding holes
// whose fields are written and read back at a runtime index.
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Packed {
    a: u8,
    b: u16,
    c: u32,
    d: u64,
    e: i16,
    f: i8,
    g: u16,
    h: u8,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Holes {
    a: u8,
    b: u32,
    c: u16,
    d: u64,
    e: u8,
}

#[repr(C, align(32))]
struct Over {
    recs: [Packed; 4],
    holes: [Holes; 3],
    tail: u32,
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let zero = Packed {
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        e: 0,
        f: 0,
        g: 0,
        h: 0,
    };
    let hz = Holes {
        a: 0,
        b: 0,
        c: 0,
        d: 0,
        e: 0,
    };
    let mut o = Over {
        recs: [zero; 4],
        holes: [hz; 3],
        tail: input1 ^ input2,
    };

    // Write every field of every record with per-record values.
    let mut k = 0usize;
    while k < 4 {
        let v = input1.wrapping_mul(0x0101_0101 + k as u32) ^ input2.rotate_left(k as u32 * 7);
        o.recs[k].a = v as u8;
        o.recs[k].b = (v >> 8) as u16;
        o.recs[k].c = v.wrapping_mul(0x9e37_79b9);
        o.recs[k].d = ((v as u64) << 32 | (input2 as u64)).wrapping_mul(0x2545_f491_4f6c_dd1d);
        o.recs[k].e = (v >> 16) as i16;
        o.recs[k].f = (v >> 24) as i8;
        o.recs[k].g = (v ^ 0xffff) as u16;
        o.recs[k].h = (v >> 4) as u8;
        k += 1;
    }

    // Read the record at a runtime index, then rewrite two fields of a
    // neighbour and read every record back through the permuted index.
    let i = (input2 & 3) as usize;
    let r = o.recs[i];
    let c = r.c;
    let d = r.d;
    let b = r.b;
    let e = r.e;
    let f = r.f;
    let g = r.g;
    let mut acc = c ^ (d as u32) ^ ((d >> 32) as u32).rotate_left(3);
    acc = acc.wrapping_add(b as u32).wrapping_add((e as i32) as u32).wrapping_add((f as i32 as u32) << 3);
    acc = acc.wrapping_add((g as u32) << 16).wrapping_add(r.a as u32).wrapping_add((r.h as u32) << 24);

    let n = (i + 1) & 3;
    o.recs[n].c = acc;
    o.recs[n].d = (acc as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    o.recs[n].e = (acc >> 3) as i16;

    let mut m = 0usize;
    while m < 4 {
        let idx = (m + i) & 3;
        let p = o.recs[idx];
        let pc = p.c;
        let pd = p.d;
        let pb = p.b;
        let pe = p.e;
        let pg = p.g;
        acc = acc.rotate_left(5) ^ pc ^ (pd as u32) ^ ((pd >> 32) as u32);
        acc = acc.wrapping_add(pb as u32).wrapping_add(pe as i32 as u32).wrapping_add(pg as u32);
        acc ^= (p.a as u32) | ((p.f as i32 as u32) << 8) | ((p.h as u32) << 16);
        m += 1;
    }

    // Padded record: write fields at a runtime index, read them back.
    let hi = (input1 % 3) as usize;
    o.holes[hi].a = input1 as u8;
    o.holes[hi].b = input2.rotate_left(9);
    o.holes[hi].c = (input1 >> 16) as u16;
    o.holes[hi].d = (input2 as u64) << 20 | input1 as u64;
    o.holes[hi].e = (input2 >> 24) as u8;
    let hj = (hi + 1) % 3;
    o.holes[hj].b = acc;
    let h = o.holes[hi];
    let h2 = o.holes[hj];
    acc = acc.wrapping_add(h.a as u32).wrapping_add(h.b).wrapping_add(h.c as u32);
    acc = acc.wrapping_add(h.d as u32).wrapping_add((h.d >> 32) as u32).wrapping_add(h.e as u32);
    acc ^ h2.b.rotate_left(13) ^ o.tail
}
