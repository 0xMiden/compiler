// A u32 accumulator and a u64 accumulator, each READ several times per
// iteration of a hot loop and written once per iteration. Both are wasm
// locals with many `local.get`s and several `local.set`s, so Local2Reg's
// single-load/single-store precondition fails on them at every debug level
// ("loaded more than once" / "stored more than once"); the loop-invariant
// seeds computed before the loop are the promotable slots. This is the
// value guard for the shape where DWARF-off promotion is expected to change
// nothing: the answer must be identical with and without guest DWARF.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Single-store/single-load seeds: the promotable slots of this function.
    let seed32 = input1 ^ 0x9e37_79b9;
    let seed64 = ((input2 as u64) << 21) | 0x0f;

    let mut acc32: u32 = seed32 | 1;
    let mut acc64: u64 = seed64 | 3;
    let iters = (input2 % 19) + 2;
    let mut i: u32 = 0;
    while i < iters {
        // acc32 read five times in one body.
        acc32 = acc32
            .rotate_left(7)
            .wrapping_add(acc32 ^ (acc32 >> 3))
            .wrapping_sub(acc32 & 0x00ff_00ff)
            ^ acc32.rotate_right(11);
        // acc64 read five times in one body.
        acc64 = acc64.rotate_left(13)
            ^ acc64.wrapping_add(acc64 >> 17)
            ^ (acc64 & 0x0000_ffff_0000_ffff)
            ^ acc64.rotate_right(29);
        i = i.wrapping_add(1);
    }
    acc32 ^ (acc64 as u32) ^ ((acc64 >> 32) as u32)
}
