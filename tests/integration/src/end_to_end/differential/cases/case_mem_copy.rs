// Runtime-length slice copies. `copy_from_slice`/`copy_within` with a
// length LLVM cannot constant-fold become wasm `memory.copy`, which lowers
// to the HIR MemCpy op — exercising `OpEmitter::memcpy` (element-aligned
// fast path + byte fallback loop, both emitted at compile time) and the
// MemoryCopy translation arm. The u8 copy at odd offsets also takes the
// fallback loop at runtime; the u32 copies take the element fast path.
// All copies are between disjoint ranges — overlapping copies diverge
// (see case_mem_overlap) and are tracked as a separate ignored case.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 16];
    let mut i = 0u32;
    while i < 16 {
        a[i as usize] = input1.wrapping_mul(i).wrapping_add(input2);
        i += 1;
    }

    // Aligned u32 copy, runtime length 4..=11 elements.
    let n = ((input2 % 8) + 4) as usize;
    let mut b = [0u32; 16];
    b[..n].copy_from_slice(&a[..n]);

    // Second in-buffer copy, disjoint ranges (src 0..=5, dst 10..).
    let m2 = (n % 4) + 2;
    a.copy_within(0..m2, 10);

    // Byte copy at odd offsets / odd length — defeats the element-aligned
    // fast path at runtime.
    let mut bytes = [0u8; 32];
    let mut j = 0u32;
    while j < 16 {
        bytes[j as usize] = (input1 >> (j % 24)) as u8;
        j += 1;
    }
    let m = ((input1 % 8) + 3) as usize;
    bytes.copy_within(1..1 + m, 17);

    let k = (input2 % 16) as usize;
    b[k].wrapping_add(a[(input1 % 16) as usize]) ^ (bytes[17 + m - 1] as u32)
}
