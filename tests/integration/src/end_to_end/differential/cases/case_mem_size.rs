// wasm `memory.size` lowering. memory.size is only clobbered by
// memory.grow, so two bare calls get CSE'd and folded away; a `memory_grow`
// call behind a dynamically-impossible condition (h % 6 == 5 implies
// h % 3 == 2, so it can never also be 0 — a cross-modulus contradiction
// LLVM does not fold) keeps both calls alive without ever executing the
// grow at runtime. The page-count difference is then deterministically
// zero, mirrored by the native build's constant. Exercises the MemorySize
// translation arm and `OpEmitter::mem_size`.
#[cfg(target_arch = "wasm32")]
fn size_diff(h: u32) -> u32 {
    let a = core::arch::wasm32::memory_size(0) as u32;
    if h % 6 == 5 && h % 3 == 0 {
        core::arch::wasm32::memory_grow(0, 1);
    }
    let b = core::arch::wasm32::memory_size(0) as u32;
    a.wrapping_sub(b)
}

#[cfg(not(target_arch = "wasm32"))]
fn size_diff(_h: u32) -> u32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let d = size_diff(input1 ^ (input2 >> 3));
    input2.wrapping_add(input1.rotate_right(d.wrapping_add(11) & 31)) ^ d
}
