// Local2Reg gap shapes. Every wasm function parameter gets an unconditional
// `hir.store_local` at entry (frontend/wasm `declare_parameters`), so:
// (1) `lsh_unused`'s ignored second parameter is a stored-but-never-loaded
//     local, reaching the dead-store-erasure arm of the Local2Reg pass
//     (`#[no_mangle]` gives the helper external linkage so LLVM's dead-arg
//     elimination cannot drop the parameter; `#[inline(never)]` keeps the
//     call);
// (2) `lsh_konst` has no parameters and no wasm locals at all, reaching the
//     pass's `locals.is_empty()` early return;
// (3) `lsh_pick` takes a by-value array, which Rust passes indirectly — the
//     incoming pointer travels through a single-use local (promotable), and
//     with debug info the aggregate's `di.debug_declare` references that
//     local, reaching the declare-conversion path of
//     `convert_debug_references_for_local`.

#[inline(never)]
#[unsafe(no_mangle)]
extern "C" fn lsh_unused(a: u32, _dead: u32) -> u32 {
    a.wrapping_mul(2654435761).rotate_left(5)
}

#[inline(never)]
#[unsafe(no_mangle)]
extern "C" fn lsh_konst() -> u32 {
    40507
}

#[inline(never)]
#[unsafe(no_mangle)]
fn lsh_pick(arr: [u32; 4], i: u32) -> u32 {
    arr[(i & 3) as usize]
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let arr = [input1, input2, input1 ^ input2, input1.wrapping_add(input2)];
    let a = lsh_unused(input1, input2);
    let b = lsh_konst();
    let c = lsh_pick(arr, input2 >> 7);
    a ^ b.wrapping_add(c)
}
