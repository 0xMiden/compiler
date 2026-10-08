// A guest that does not link: `combine` is declared but defined nowhere, so
// `rust-lld` fails with an undefined symbol, the way the outlined `memcmp` of
// `core_eq_reach` does. The build must fail with an error, not end the process
// (see `corelib::guest_link_failure`).
unsafe extern "C" {
    fn combine(a: u32, b: u32) -> u32;
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { combine(input1, input2) }
}
