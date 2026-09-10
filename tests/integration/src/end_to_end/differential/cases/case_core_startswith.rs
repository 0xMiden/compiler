// Prefix / suffix tests (campaign 27, Part A): `[u8]::starts_with`,
// `[u8]::ends_with` and `str::starts_with(&str)` all compare a sub-slice
// with `==` internally, so they lower to the `memcmp` libcall the guest
// cannot link, at every optimization level. The linkable replacement is an
// element loop — `a.iter().zip(prefix).all(|(x, y)| x == y)` — or, for a
// one-character prefix, `str::starts_with(char)` / a first-byte test.

fn text(i: u32) -> &'static str {
    match i % 3 {
        0 => "alpha-beta",
        1 => "alp",
        _ => "beta",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1.to_le_bytes();
    let b = input2.to_le_bytes();
    let n = 1 + (input2 % 3) as usize;
    let starts = a.starts_with(&b[..n]) as u32;
    let ends = a.ends_with(&b[..n]) as u32;
    let str_starts = text(input1).starts_with(text(input2)) as u32;
    starts | (ends << 1) | (str_starts << 2)
}
