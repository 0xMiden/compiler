// Small branchy case — exercises comparisons, if/else, and u32 div/rem
// paths in the compiler pipeline.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    if input2 == 0 {
        return input1.wrapping_mul(3).wrapping_add(1);
    }
    let q = input1 / input2;
    let r = input1 % input2;
    if q > r { q.wrapping_sub(r) } else { r.wrapping_sub(q) }
}
