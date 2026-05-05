// Exercises integer width conversions and bit-counting unary intrinsics across
// multiple types. Targets `OpEmitter::cast` and the per-width arms of
// `clz`/`ctz`/`popcnt`/`bnot` in `codegen/masm/src/emit/unary.rs`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // u32 -> u64 zero-extend, then u64 -> u32 narrow.
    let wide: u64 = (input1 as u64).wrapping_add(input2 as u64);
    let narrow: u32 = wide as u32;

    // u32 -> u8 / u16 narrow casts.
    let lo8: u8 = input1 as u8;
    let lo16: u16 = input2 as u16;

    // Bit-counting on multiple widths to exercise per-type emit arms.
    let pop64: u32 = wide.count_ones();
    let lz32: u32 = narrow.leading_zeros();
    let tz16: u32 = (lo16 | 1).trailing_zeros();
    let pop8: u32 = lo8.count_ones() as u32;

    // Bitwise NOT on u8/u16 to exercise `bnot` smallint paths.
    let not8: u32 = (!lo8) as u32;
    let not16: u32 = (!lo16) as u32;

    pop64
        .wrapping_add(lz32)
        .wrapping_add(tz16)
        .wrapping_add(pop8)
        .wrapping_add(not8)
        .wrapping_add(not16)
        .wrapping_add(narrow)
}
