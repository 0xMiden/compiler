// Constant location ranges for NAMED mutable locals: a variable initialized
// with a constant and first mutated inside a loop gets a DWARF location LIST
// whose first range is a constant expression (DW_OP_consts for negative i64,
// DW_OP_constu for u64) followed by wasm-local ranges. Drives the
// SignedConstant decode arm of `decode_storage_from_expression`, constant
// schedule entries, and the ConstU64/ConstS64 lowering arms of
// `debug_var_location_from_expression` including the felt-range rejection
// (negative / above-modulus constants must yield NO location, not a panic).
// Debug info must never change semantics.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut sacc: i64 = -0x1234_5678_9abc;
    let mut wacc: u64 = 0xffff_fff0_1234_5678;
    let mut small: u64 = 40507;
    let n = (input1 % 9) + 2;
    let mut i = 0u32;
    while i < n {
        sacc = sacc.wrapping_add((input2 ^ i) as i64);
        wacc = wacc.rotate_left(7) ^ (sacc as u64);
        small = small.wrapping_add(wacc & 0xffff);
        i = i.wrapping_add(1);
    }
    (sacc as u32) ^ (wacc as u32).wrapping_add(small as u32)
}
