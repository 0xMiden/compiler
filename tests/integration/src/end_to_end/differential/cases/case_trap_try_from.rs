// Trap parity: `TryFrom` narrowing conversions panicking through `unwrap`.
// Each conversion reads its own field of the inputs so its boundary is
// reachable on its own: `u8::try_from` over 0..299 (panics above 255),
// `i8::try_from` over -100..155 (panics above 127), `i16::try_from` over
// 0..33999 (panics above 32767) and `u32::try_from(i32)` over -5..250
// (panics on the negative values). `usize::try_from(u64)` is deliberately
// absent: `usize` is 64 bits natively and 32 bits on wasm, so it would
// differ by target rather than by compiler.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let byte_src = input1 % 300;
    let small_src = (((input1 >> 16) & 0xff) as i32) - 100;
    let half_src = (input2 % 34_000) as i32;
    let pos_src = 250 - (((input2 >> 16) & 0xff) as i32);
    // Must not trap: the defaulting sibling of the same narrowing.
    let lax = u8::try_from(byte_src).unwrap_or(0xff) as u32;
    let byte = u8::try_from(byte_src).unwrap() as u32;
    let small = i8::try_from(small_src).unwrap() as u32;
    let half = i16::try_from(half_src).unwrap() as u32;
    let pos = u32::try_from(pos_src).unwrap();
    lax ^ byte ^ (small & 0xff) ^ (half & 0xffff) ^ pos
}
