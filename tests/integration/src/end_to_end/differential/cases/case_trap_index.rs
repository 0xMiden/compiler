// Trap parity: a runtime index into a fixed `[u32; 8]`. Indices 0..8 return
// the element mixed with `input2`; 8..12 fail the bounds check (`index out
// of bounds`) and must trap on both targets.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let table: [u32; 8] = [3, 1, 4, 1, 5, 9, 2, 6];
    let i = (input1 % 12) as usize;
    table[i].wrapping_mul(input2 | 1) ^ (i as u32)
}
