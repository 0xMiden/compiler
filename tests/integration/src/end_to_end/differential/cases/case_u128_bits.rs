// u128 bit-count intrinsics on dynamic values: `count_ones` is two i64.popcnt
// limbs added; `leading_zeros`/`trailing_zeros` are clz/ctz limb selects
// (hi == 0 ? 64 + clz(lo) : clz(hi)) — limb compositions the corpus never
// executed (widening/u64_ucmp/zext_wide_ctz stop at single-limb 64-bit forms).
// `vz`/`wz` zero one limb on ~half the inputs (opaque to LLVM: a parity
// multiply, not a provable mask), so BOTH legs of each clz/ctz select run
// across the 16 random inputs. No `| 1` bit pinning, so nothing folds.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = (input2 as u64).wrapping_mul(0x0101_0193) ^ ((input1 as u64) << 13);
    let v: u128 = ((a as u128) << 64) | b as u128;
    // High limb dynamically zero on ~half the inputs.
    let vz: u128 = (((a.wrapping_mul((input2 & 1) as u64)) as u128) << 64) | b as u128;
    // Low limb dynamically zero on ~half the inputs.
    let wz: u128 = ((a as u128) << 64) | (b.wrapping_mul((input1 & 1) as u64)) as u128;

    let po = v.count_ones();
    let lz = vz.leading_zeros();
    let tz = wz.trailing_zeros();

    po.wrapping_add(lz.rotate_left(7)).wrapping_add(tz.rotate_left(14))
}
