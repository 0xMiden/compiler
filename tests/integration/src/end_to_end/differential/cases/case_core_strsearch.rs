// Substring search (campaign 27, Part A): `str::contains(&str)` and
// `str::find(&str)` run `core`'s two-way searcher, which compares slices
// with `==` and therefore lowers to the `memcmp` libcall the guest cannot
// link, at every optimization level. The linkable replacements are a
// hand-written window scan over `as_bytes()`, or searching for a single
// `char` (`core_str_find`, which links at the default level and at
// `--optimize=max`).

fn text(i: u32) -> &'static str {
    match i % 3 {
        0 => "alpha-beta-gamma",
        1 => "beta",
        _ => "gamma-delta",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let haystack = text(input1);
    let needle = text(input2);
    let found = haystack.contains(needle) as u32;
    let at = haystack.find(needle).unwrap_or(31) as u32;
    let last = haystack.rfind(needle).unwrap_or(31) as u32;
    found | (at << 1) | (last << 8)
}
