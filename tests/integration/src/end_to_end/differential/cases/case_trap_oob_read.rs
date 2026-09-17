// W7 probe, NOT a normal trap-parity case: a raw read at an input-derived
// offset 256 MiB past the guest's linear memory. This is undefined behaviour
// in Rust, so the native side is not a language-level oracle — it reports
// whatever the host does with an unmapped address (SIGSEGV). What the probe
// answers is what MIDEN does with a wasm address that no page backs, next to
// wasmtime's "out of bounds memory access" trap on the same wasm.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let local: [u32; 4] = [1, 2, 3, 4];
    let base = local.as_ptr() as usize;
    let off = 0x1000_0000usize + ((input1 as usize) & 0xffff) * 4;
    let v = unsafe { core::ptr::read_volatile((base + off) as *const u32) };
    v ^ input2 ^ local[3]
}
