// `str::find` / `rfind` with a `char` pattern (campaign 27, Part A): the
// same `CharSearcher` as `core_str_patterns`, reached through the searching
// entry points instead of the splitting ones. It links at the default level
// and at `--optimize=max`, and NOT at `--optimize=size-min` or
// `--optimize=basic`, where the searcher stays outlined and keeps the
// `memcmp` libcall (pinned by the `core_str_find_oz` twin). `find` with a
// `&str` pattern (the two-way searcher) does not link at ANY level — see
// `core_str_search_nolink`.

fn text(i: u32) -> &'static str {
    match i % 5 {
        0 => "alpha-beta-gamma",
        1 => "a",
        2 => "-",
        3 => "",
        _ => "no separator here",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = text(input1);
    let c = if input2 & 1 == 0 { '-' } else { 'a' };
    let first = s.find(c).unwrap_or(31) as u32;
    let last = s.rfind(c).unwrap_or(31) as u32;
    let counted = s.chars().filter(|&x| x == c).count() as u32;
    let head = &s[..first.min(s.len() as u32) as usize];
    first
        .wrapping_mul(1009)
        .wrapping_add(last * 31)
        .wrapping_add(counted)
        .wrapping_add(head.len() as u32)
        .wrapping_add(s.len() as u32)
}
