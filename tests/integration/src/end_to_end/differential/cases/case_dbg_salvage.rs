// Salvaged debug expressions: NAMED locals whose defining computation is
// dead. LLVM's dead-instruction elimination salvages the dbg.value into an
// arithmetic DWARF expression over a live value (DW_OP_mul / DW_OP_and /
// DW_OP_shl / DW_OP_plus_uconst ... + DW_OP_stack_value). The wasm frontend
// decoder has arms only for the plus-constant form; the other operators must
// take the catch-all of `decode_storage_from_expression` and drop the whole
// expression instead of panicking. Debug info must never change semantics —
// the dead bindings must stay dead.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let tripled = input1.wrapping_mul(3);
    let masked = input2 & 0x00ff_ff00;
    let shifted = input1 << 5;
    let bumped = input2.wrapping_add(9);
    let _ = (tripled, masked, shifted, bumped);
    input1.rotate_left(9) ^ input2.wrapping_mul(0x0101_0101)
}
