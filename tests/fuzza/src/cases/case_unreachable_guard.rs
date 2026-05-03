// Keeps a panic edge in the Wasm while making that edge unreachable for every u32 input.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let nonzero_delta = input2 | 1;
    if input1 == input1.wrapping_add(nonzero_delta) {
        unreachable!();
    }

    input1.rotate_left(input2 & 31) ^ nonzero_delta
}
