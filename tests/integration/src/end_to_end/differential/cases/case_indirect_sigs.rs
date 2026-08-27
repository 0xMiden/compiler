// Two function-pointer tables with DIFFERENT function types. All address-taken
// functions land in the single wasm funcref table, but the two dispatch sites
// carry distinct signature type indices — so the lowered `builtin.function_table`
// holds entries with two different signature tags, and each `hir.exec_indirect`
// call site must skip (tag-filter) the other signature's entries while the
// runtime tag check accepts only its own.

#[inline(never)]
fn un_not(a: u32) -> u32 {
    !a
}

#[inline(never)]
fn un_rev(a: u32) -> u32 {
    a.swap_bytes().rotate_left(9)
}

#[inline(never)]
fn wi_fold(a: u64, b: u32) -> u64 {
    a.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(b as u64)
}

#[inline(never)]
fn wi_shear(a: u64, b: u32) -> u64 {
    (a ^ ((b as u64) << 17)).rotate_right(23)
}

// Runtime-indexed loads of fn pointers from static arrays are not
// devirtualized by LLVM without PGO, so both dispatches survive as
// `call_indirect` with distinct type indices.
static UNARY: [fn(u32) -> u32; 2] = [un_not, un_rev];
static WIDE: [fn(u64, u32) -> u64; 2] = [wi_fold, wi_shear];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let u = UNARY[(input1 & 1) as usize];
    let w = WIDE[((input2 >> 1) & 1) as usize];
    let narrow = u(input1.wrapping_add(input2));
    let wide = w(((input1 as u64) << 32) | input2 as u64, narrow);
    (wide as u32).wrapping_add((wide >> 32) as u32)
}
