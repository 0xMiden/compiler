// The equality forms that DO link in a guest (campaign 27, Part A): a
// constant-size `[u8; 4] == [u8; 4]` and a derived `PartialEq` on a struct
// holding a `[u8; 16]` (both compiled to inline compares at opt-level 1, 2
// and 3 — but NOT at `-Oz`, where they become the `memcmp` libcall, which is
// what the `core_eq_reach_oz` twin pins), a `[u32; 8] == [u32; 8]`, plus the
// three hand-written replacements a user needs when the compare IS outlined:
// `Iterator::eq`, `zip(..).all(..)` and `eq_ignore_ascii_case` (which lowers
// to an element loop, not a libcall), and `[u8]::contains`, which compares
// elements rather than slices.

#[derive(PartialEq, Eq)]
struct Block {
    id: u32,
    data: [u8; 16],
}

fn text(i: u32) -> &'static str {
    match i % 4 {
        0 => "Alpha",
        1 => "ALPHA",
        2 => "beta",
        _ => "",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = 0u32;

    // Constant-size array equality: inlined, no libcall.
    let x = input1.to_le_bytes();
    let y = input2.to_le_bytes();
    acc += (x == y) as u32;
    acc += ((x != y) as u32) << 1;

    // Derived `PartialEq` over a struct with a 16-byte array field.
    let mut p = Block { id: input1, data: [0u8; 16] };
    let mut q = Block { id: input2, data: [0u8; 16] };
    let mut i = 0usize;
    while i < 16 {
        p.data[i] = (input1 >> (i as u32 & 7)) as u8;
        q.data[i] = (input2 >> (i as u32 & 7)) as u8;
        i += 1;
    }
    acc += ((p == q) as u32) << 2;

    // A 32-byte constant-size array equality.
    let mut u = [0u32; 8];
    let mut v = [0u32; 8];
    let mut j = 0usize;
    while j < 8 {
        u[j] = input1.wrapping_add(j as u32);
        v[j] = input2.wrapping_add(j as u32);
        j += 1;
    }
    acc += ((u == v) as u32) << 3;

    // The replacements for a runtime-length slice compare.
    let n = 1 + (input2 % 4) as usize;
    acc += ((x[..n].iter().eq(y[..n].iter())) as u32) << 4;
    acc += ((x.iter().zip(y.iter()).all(|(a, b)| a == b)) as u32) << 5;
    acc += ((x[..n].iter().cmp(y[..n].iter()) == core::cmp::Ordering::Less) as u32) << 6;

    // Case-insensitive comparison of both `str` and `[u8]` — element loops.
    let s = text(input1);
    let t = text(input2);
    acc += ((s.eq_ignore_ascii_case(t)) as u32) << 7;
    acc += ((s.as_bytes().eq_ignore_ascii_case(t.as_bytes())) as u32) << 8;
    acc += ((s.len() == t.len()) as u32) << 9;

    // Element membership, not slice equality.
    acc += ((x.contains(&(input2 as u8))) as u32) << 10;
    acc += ((s.as_bytes().iter().any(|&c| c == input1 as u8)) as u32) << 11;

    acc.wrapping_add(p.id ^ q.id).wrapping_add(u[7] ^ v[0])
}
