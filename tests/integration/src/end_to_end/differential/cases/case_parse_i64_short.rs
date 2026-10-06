// Director bisect of `case_core_parse_i64.rs`: the same `str::parse::<i64>`
// of a runtime-length slice, but the length never reaches 16, so LLVM emits
// only the plain `i64.mul` accumulation path (no `i64.mul_wide_s` overflow
// path in the function). If this agrees with native, the divergence needs
// the two accumulation paths to coexist in one function.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = "9007199254740993";
    let end = 1 + (input2 as usize % 15);
    let w = &s[..end];
    match w.parse::<i64>() {
        Ok(v) => (v as u32) ^ ((v >> 32) as u32),
        Err(_) => 0xdead_beef,
    }
    .wrapping_add(input1 & 1)
}
