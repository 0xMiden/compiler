// Runtime-indexed u32 array on the shadow stack. Dynamic indices prevent
// SROA from promoting the buffer to registers, so every access survives as
// a wasm i32.load/i32.store with a computed address — exercising wasm->HIR
// pointer preparation (`prepare_addr`) and the word-sized load/store paths
// in `codegen/masm/src/emit/mem.rs`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u32; 16];
    let mut acc = input1 | 1;
    let mut i = 0u32;
    while i < 16 {
        buf[(acc % 16) as usize] = acc ^ input2;
        acc = acc.wrapping_mul(1664525).wrapping_add(1013904223);
        i += 1;
    }
    let j = (input2 % 16) as usize;
    let a = buf[j];
    let b = buf[(j + 5) % 16];
    let c = buf[(input1 % 16) as usize];
    a.wrapping_add(b).rotate_left(c & 31) ^ acc
}
