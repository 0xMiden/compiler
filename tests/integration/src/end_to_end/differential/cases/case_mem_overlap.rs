// Overlapping `copy_within` with dst > src. LLVM emits wasm `memory.copy`,
// which has memmove semantics (overlap must behave as-if buffered), but the
// MASM memcpy lowering appears to copy forward, overwriting source elements
// before they are read. The whole-array sum makes any corruption visible
// for every input pair (n >= 4, so the ranges always overlap).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 16];
    let mut i = 0u32;
    while i < 16 {
        a[i as usize] = input1.wrapping_add(i.wrapping_mul(0x9e37_79b9));
        i += 1;
    }
    // src 2..2+n overlaps dst 4..4+n for all n in 4..=11.
    let n = ((input2 % 8) + 4) as usize;
    a.copy_within(2..2 + n, 4);
    let mut sum = 0u32;
    let mut j = 0;
    while j < 16 {
        sum = sum.wrapping_add(a[j] ^ (j as u32));
        j += 1;
    }
    sum
}
