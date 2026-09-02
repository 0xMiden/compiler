// Shadowing/rebinding shapes for debug info: one function with many NAMED
// locals whose live ranges are disjoint (sequential rebinds of `seed`) or
// nested (block/arm shadows). With full DWARF each binding is its own
// DW_TAG_variable with scoped location ranges, driving multi-range location
// lists through `decode_variable_entry`, the location schedule (declare +
// kill events) in `build_location_schedule`, and the scheduled dbg paths of
// `FunctionBuilderExt::emit_scheduled_dbg_value`. Debug info must never
// change semantics — the result depends only on the inputs.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let seed = input1 ^ 0x9e37_79b9;
    let mix = seed.wrapping_mul(0x9e37_79b1).rotate_left(7);
    let mut out = mix ^ input2;
    {
        let seed = mix.wrapping_add(input2);
        let twist = seed.rotate_right(11) ^ input1;
        if twist & 1 == 0 {
            let seed = twist.wrapping_mul(5);
            out = out.wrapping_add(seed);
        } else {
            let gap = twist | 0x8000_0001;
            out ^= gap.rotate_left(3);
        }
    }
    let seed = input2.wrapping_sub(mix);
    let tail = seed.rotate_left(mix & 15);
    out.wrapping_add(tail)
}
