// Bound for the `core_parse_i64` divergence (campaign 27): the same
// runtime-length-slice parse with `u64` instead of `i64`. The unsigned
// `from_str` accumulation agrees with native at every optimization level,
// so the miscompile is specific to the SIGNED 64-bit path.
fn text(i: u32) -> &'static str {
    match i % 3 {
        0 => "9007199254740993",
        1 => "123456789",
        _ => "42",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = text(input1);
    let end = 1 + (input2 as usize % s.len().max(1));
    let w = &s[..end];
    w.parse::<u64>().map(|v| v as u32).unwrap_or(2).wrapping_add(input2)
}
