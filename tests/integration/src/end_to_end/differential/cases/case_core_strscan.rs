// The `core::str` surface that links at every optimization level (campaign
// 27, Part A): `from_utf8` validation of an input-built buffer,
// `char_indices` / `chars().rev()`, `trim` / `trim_start_matches` /
// `trim_matches`, `split_whitespace`, `is_char_boundary`, `get`, `len_utf8`,
// `char::encode_utf8`, `parse::<u32>` and
// `u32::from_str_radix` / `i32::from_str_radix`. `parse::<i64>` is
// deliberately NOT here: it miscompiles (see `core_parse_i64`), and keeping
// it would make this guard red for a reason that has nothing to do with the
// rest of the surface. The pattern-based splitters
// (`split`, `split_once`, `find` with a `char`) are NOT here: whether their
// `SplitInternal::next` keeps an outlined `memcmp` depends on the opt level
// and on how many of them one function contains (see `core_str_patterns`
// and `core_str_find`).

fn text(i: u32) -> &'static str {
    match i % 5 {
        0 => "  12345  ",
        1 => "-9007199 254740993",
        2 => "xx7fffffff",
        3 => "",
        _ => "  ",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = text(input1);
    let mut acc = s.len() as u32;

    let t = s.trim();
    acc = acc.wrapping_add(t.len() as u32 * 3);
    acc = acc.wrapping_add(s.trim_start().len() as u32);
    acc = acc.wrapping_add(s.trim_end_matches(' ').len() as u32);
    acc = acc.wrapping_add(t.trim_matches('x').len() as u32);

    for (i, c) in t.char_indices() {
        acc = acc.wrapping_mul(31).wrapping_add(i as u32 ^ c as u32);
        acc = acc.wrapping_add(c.is_ascii_digit() as u32);
        acc = acc.wrapping_add(c.to_digit(10).unwrap_or(11));
        acc = acc.wrapping_add(c.len_utf8() as u32);
    }
    acc = acc.wrapping_add(t.chars().rev().take(3).map(|c| c as u32).sum::<u32>());
    acc = acc.wrapping_add(t.bytes().fold(7u32, |h, b| h.rotate_left(3) ^ b as u32));

    let mut words = 0u32;
    for w in s.split_whitespace() {
        words += 1;
        acc = acc.wrapping_mul(17).wrapping_add(w.len() as u32);
        acc = acc.wrapping_add(w.parse::<u32>().unwrap_or(1));
        acc = acc.wrapping_add(u32::from_str_radix(w, 16).unwrap_or(3));
    }
    acc = acc.wrapping_add(words * 101);

    let cut = (input2 % 12) as usize;
    acc = acc.wrapping_add(s.is_char_boundary(cut) as u32 * 5);
    acc = acc.wrapping_add(s.get(..cut).map_or(9, |z| z.len() as u32));
    acc = acc.wrapping_add(s.as_bytes().iter().filter(|&&b| b == b' ').count() as u32);
    acc = acc.wrapping_add(i32::from_str_radix("-80000000", 16).unwrap_or(4) as u32);

    // Build a small string from the inputs and validate it.
    let mut buf = [0u8; 32];
    let mut n = 0usize;
    let mut i = 0u32;
    while i < 5 && n + 4 <= buf.len() {
        let c = char::from_u32(0x30 + (input2.rotate_left(i * 6) % 0x2f)).unwrap_or('?');
        n += c.encode_utf8(&mut buf[n..]).len();
        i += 1;
    }
    acc = acc.wrapping_add(match core::str::from_utf8(&buf[..n]) {
        Ok(z) => z.chars().count() as u32 * 13,
        Err(e) => 0x1000 + e.valid_up_to() as u32,
    });
    acc
}
