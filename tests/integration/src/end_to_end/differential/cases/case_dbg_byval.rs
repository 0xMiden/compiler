// By-value aggregate params with a SINGLE field read: the aggregate travels
// as a pointer in wasm local 0, and the DWARF variable for the param is a
// memory location `DW_OP_WASM_local 0` (no DW_OP_stack_value). The pointer
// local has exactly one store (frontend entry store) and one load (the field
// read), so Local2Reg promotes it — the safe-declare conversion path of
// `convert_debug_references_for_local` (declare loop) if LLVM names the
// param. A second helper reads TWO fields (two loads — promotion skipped,
// stores preserved). Debug info must never change semantics.

#[derive(Clone, Copy)]
struct Blob {
    lo: u32,
    hi: u32,
    tail: u64,
}

#[inline(never)]
fn dbg_bv_one(blob: Blob, k: u32) -> u32 {
    blob.lo.rotate_left(k & 7)
}

#[inline(never)]
fn dbg_bv_two(blob: Blob, k: u32) -> u32 {
    blob.hi.wrapping_add((blob.tail as u32) ^ k)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let blob = Blob {
        lo: input1 ^ 0x0135_7bd9,
        hi: input2.wrapping_mul(0x0101_0101),
        tail: ((input1 as u64) << 32) | input2 as u64,
    };
    dbg_bv_one(blob, input2).wrapping_add(dbg_bv_two(blob, input1))
}
