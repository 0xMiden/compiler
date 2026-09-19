// Wide results returned BY VALUE through hidden return-area pointers: a
// padded `#[repr(C)]` record whose u128 field lands word-aligned in the
// caller's frame, a `#[repr(C, packed)]` record whose u128 sits at byte
// offset 1 (unaligned i64 store pairs into the return area), a 13-byte
// array, a `(u64, u64)` tuple, `Option<u128>` / `Result<u64, u32>` (tag +
// payload), a u128 accumulator rebuilt by a helper on every trip of a loop
// (one return area reused per iteration), and a forwarding helper that
// passes its own return-area pointer straight to the callee. Every field is
// read back (array elements at runtime positions) so a misplaced store or a
// stale return area changes the result.
#[derive(Clone, Copy)]
#[repr(C)]
struct Padded {
    a: u64,
    b: u32,
    c: u8,
    d: u128,
    e: u16,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Tight {
    a: u8,
    b: u128,
    c: u16,
}

#[inline(never)]
fn padded(x: u64, k: u32) -> Padded {
    Padded {
        a: x.rotate_left(k & 63),
        b: (x as u32) ^ k,
        c: (x >> 40) as u8,
        d: ((x as u128) << 64) | (x.wrapping_mul(0x9e37_79b9_7f4a_7c15) as u128),
        e: (k >> 16) as u16,
    }
}

#[inline(never)]
fn tight(x: u64, k: u32) -> Tight {
    Tight {
        a: k as u8,
        b: ((x.wrapping_add(k as u64) as u128) << 64) | (x ^ 0x5555_5555_5555_5555) as u128,
        c: (x >> 48) as u16 ^ k as u16,
    }
}

// The callee's return area IS this function's own return-area pointer.
#[inline(never)]
fn forward(x: u64, k: u32) -> Padded {
    padded(x ^ 0x0f0f_0f0f_0f0f_0f0f, k.rotate_left(5))
}

#[inline(never)]
fn thirteen(x: u64, k: u32) -> [u8; 13] {
    let mut out = [0u8; 13];
    let mut i = 0usize;
    while i < 13 {
        out[i] = (x >> ((i * 5) & 63)) as u8 ^ (k as u8).wrapping_mul(i as u8 | 1);
        i += 1;
    }
    out
}

#[inline(never)]
fn pair(x: u64, y: u64) -> (u64, u64) {
    (x.wrapping_mul(y | 1), x ^ y.rotate_left(23))
}

#[inline(never)]
fn maybe(x: u64, k: u32) -> Option<u128> {
    if k & 7 == 3 {
        None
    } else {
        Some((((x as u128) << 64) | (k as u128)) ^ ((k as u128) << 32))
    }
}

#[inline(never)]
fn either(x: u64, k: u32) -> Result<u64, u32> {
    if k & 3 == 1 {
        Err(k ^ 0xdead)
    } else {
        Ok(x.rotate_left(k & 63))
    }
}

#[inline(never)]
fn step(acc: u128, i: u32) -> u128 {
    acc.wrapping_mul(0x0000_0001_0000_0003_0000_0005_0000_0007)
        .wrapping_add(i as u128)
        ^ (acc >> 64)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = ((input1 as u64) << 32) | input2 as u64;
    let p = padded(x, input2);
    let f = forward(x.rotate_left(9), input1);
    let t = tight(x ^ 0xa5a5_a5a5_a5a5_a5a5, input1 ^ input2);
    let arr = thirteen(x, input1);
    let (q0, q1) = pair(x, p.a ^ f.a);
    let opt = maybe(q0, input2 >> 3);
    let res = either(q1, input1 >> 5);
    let mut acc: u128 = p.d ^ f.d.rotate_left(7);
    let n = input1 % 13;
    let mut i = 0u32;
    while i < n {
        acc = step(acc, i);
        i = i.wrapping_add(1);
    }
    let tb = t.b;
    let tc = t.c;
    let ai = arr[(input2 % 13) as usize] as u32;
    let aj = arr[((input1 >> 4) % 13) as usize] as u32;
    let o = match opt {
        Some(v) => (v as u64) ^ ((v >> 64) as u64),
        None => 0x1111,
    };
    let r = match res {
        Ok(v) => v,
        Err(e) => e as u64 ^ 0x2222_0000,
    };
    let z = p.a
        ^ p.b as u64
        ^ ((p.c as u64) << 8)
        ^ p.e as u64
        ^ (p.d as u64)
        ^ ((p.d >> 64) as u64)
        ^ f.a.rotate_left(3)
        ^ f.b as u64
        ^ (f.d as u64).rotate_left(5)
        ^ t.a as u64
        ^ (tb as u64)
        ^ ((tb >> 64) as u64).rotate_left(11)
        ^ tc as u64
        ^ q0
        ^ q1.rotate_left(13)
        ^ o
        ^ r.rotate_left(17)
        ^ (acc as u64)
        ^ ((acc >> 64) as u64).rotate_left(19);
    (z as u32) ^ ((z >> 32) as u32) ^ ai ^ aj.rotate_left(8)
}
