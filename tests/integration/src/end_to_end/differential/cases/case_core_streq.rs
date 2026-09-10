// `&str` equality (campaign 27, Part A): comparing two `&str` values — and
// two `Option<&str>` values, which forwards to the same impl — lowers to a
// `memcmp` libcall the guest cannot link, at every optimization level, even
// when both operands are string literals of the same length. The linkable
// replacements are `str::eq_ignore_ascii_case`, `as_bytes().iter().eq(..)`
// and a hand-written byte loop (all in `core_eq_reach`).

fn text(i: u32) -> &'static str {
    match i % 3 {
        0 => "alpha",
        1 => "beta",
        _ => "alpha",
    }
}

fn maybe(i: u32) -> Option<&'static str> {
    (i % 4 != 0).then(|| text(i))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let direct = (text(input1) == text(input2)) as u32;
    let optional = (maybe(input1) == maybe(input2)) as u32;
    direct | (optional << 1)
}
