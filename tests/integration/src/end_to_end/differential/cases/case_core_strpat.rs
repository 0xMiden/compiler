// `str::split` / `split_once` with a `char` pattern (campaign 27, Part A):
// ONE splitter per function, which is what makes it linkable. The pattern
// machinery's `SplitInternal::<CharSearcher>::next` compares the encoded
// pattern bytes with a slice `==`; when the searcher is fully inlined the
// compare folds to a byte load, and when it is not, the outlined `memcmp`
// libcall the guest cannot link survives. That inlining decision is what the
// optimization level moves: this case links at the default level, at
// `--optimize=size-min` and at `--optimize=max`, and NOT at
// `--optimize=basic` (pinned by the `core_str_patterns_basic` twin). Two
// nested splitters do not link at any level (`prog_expr`).

fn text(i: u32) -> &'static str {
    match i % 5 {
        0 => "10,200,3000",
        1 => "7",
        2 => ",",
        3 => "",
        _ => "1,,2,",
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = text(input1);
    let mut acc = input2 % 13;
    let mut fields = 0u32;
    for f in s.split(',') {
        fields += 1;
        acc = acc.wrapping_mul(31).wrapping_add(f.len() as u32);
        acc = acc.wrapping_add(f.parse::<u32>().unwrap_or(1));
        acc = acc.wrapping_add(f.trim().is_empty() as u32 * 7);
    }
    acc.wrapping_add(fields * 101)
}
