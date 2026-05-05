// Exercises u32 bitwise / shift / rotate / comparison HIR ops that lower to
// the per-type arms in `codegen/masm/src/emit/binary.rs` —
// `OpEmitter::{bor, shl, shr, rotl, rotr, eq, neq, lt, gt, lte, gte}`.
//
// All comparisons use the two runtime `u32` inputs (no immediates) so the
// non-`_imm` emitter variants are taken. The shift counts are masked to
// avoid panicking shifts on `u32`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Mask shift/rotate amounts so native shifts do not panic on values >= 32.
    let amt: u32 = input2 & 31;

    // Bitwise OR + shifts.
    let mixed: u32 = (input1 | input2).wrapping_add(input1 << amt).wrapping_add(input1 >> amt);

    // Rotates (no panic risk for any u32).
    let rot: u32 = input1.rotate_left(amt).wrapping_add(input1.rotate_right(amt));

    // Six comparisons — each takes two runtime u32s, so the emitter's
    // non-immediate `eq/neq/lt/lte/gt/gte` arms are exercised.
    let eq = (input1 == input2) as u32;
    let ne = (input1 != input2) as u32;
    let lt = (input1 < input2) as u32;
    let le = (input1 <= input2) as u32;
    let gt = (input1 > input2) as u32;
    let ge = (input1 >= input2) as u32;

    mixed
        .wrapping_add(rot)
        .wrapping_add(eq)
        .wrapping_add(ne)
        .wrapping_add(lt)
        .wrapping_add(le)
        .wrapping_add(gt)
        .wrapping_add(ge)
}
