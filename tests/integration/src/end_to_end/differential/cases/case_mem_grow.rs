// wasm `memory.grow` lowering. `memory_grow(0, 0)` is a side-effecting op
// LLVM cannot fold, so both calls survive; growing by zero pages returns
// the current size twice, making the difference deterministically zero on
// the MASM side, which the native build mirrors with a constant. Exercises
// the MemoryGrow translation arm and `OpEmitter::mem_grow`.
#[cfg(target_arch = "wasm32")]
fn grow_diff() -> u32 {
    let a = core::arch::wasm32::memory_grow(0, 0) as u32;
    let b = core::arch::wasm32::memory_grow(0, 0) as u32;
    a.wrapping_sub(b)
}

#[cfg(not(target_arch = "wasm32"))]
fn grow_diff() -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let d = grow_diff();
    input1.wrapping_mul(3).wrapping_add(input2 ^ d).rotate_left(d.wrapping_add(5) & 31)
}
