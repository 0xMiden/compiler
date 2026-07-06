// Select-heavy code: one condition feeding two selects with swapped
// operands, operands kept live past the selects, and a u64 select. Forces
// the MASM select emitter to copy/move operands on the operand stack
// (`dup_select`/`mov_select` variants) instead of consuming them.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let c = (input1 ^ input2) & 1 == 0;
    let a = input1 | 3;
    let b = input2.rotate_left(4);
    let s1 = if c { a } else { b };
    let s2 = if c { b } else { a };
    let t = a.wrapping_add(b); // keeps `a` and `b` live past the selects
    let wa = ((input1 as u64) << 17) | input2 as u64;
    let wb = (input2 as u64).wrapping_mul(0x9e37_79b9);
    let s3 = if wa & 1 == 0 { wa } else { wb }; // 64-bit select
    s1.wrapping_mul(3) ^ s2 ^ t ^ (s3 as u32) ^ ((s3 >> 32) as u32)
}
