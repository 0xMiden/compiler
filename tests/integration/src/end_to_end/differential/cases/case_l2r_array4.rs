// A `[u32; 4]` indexed only by CONSTANTS: LLVM SROAs it into four scalars, so
// the array never reaches linear memory and its elements arrive at the
// compiler as four ordinary wasm locals — Local2Reg candidates exactly like
// named scalars. A second `[u32; 4]` is indexed by a runtime value derived
// from the inputs, which defeats SROA and keeps that one in the guest's
// shadow stack (real loads/stores, never a `hir.store_local`). Both halves
// feed one answer, so any promotion that reorders a write against a read
// shows up as a divergence.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // SROA-able: constant indices only.
    let mut lanes = [0u32; 4];
    lanes[0] = input1 ^ 0x1234_5678;
    lanes[1] = input2.rotate_left(5);
    lanes[2] = lanes[0].wrapping_add(lanes[1]);
    lanes[3] = lanes[2] ^ (lanes[0] >> 3);
    lanes[0] = lanes[3].rotate_right(7).wrapping_sub(lanes[1]);

    // Not SROA-able: runtime index.
    let mut table = [1u32, 2, 3, 4];
    let idx = (input1 ^ input2) & 3;
    table[idx as usize] = lanes[0] ^ lanes[2];
    let other = ((idx + 1) & 3) as usize;
    table[other] = table[other].wrapping_add(lanes[3]);

    lanes[0]
        ^ lanes[1].rotate_left(idx)
        ^ lanes[2]
        ^ lanes[3]
        ^ table[0]
        ^ table[1]
        ^ table[2]
        ^ table[3]
}
