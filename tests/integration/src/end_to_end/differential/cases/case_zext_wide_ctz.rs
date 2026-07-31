// Exercises `i64.mul_wide_u` with a constant multiplicand: translation
// zero-extends both operands to u128, and after `hir.bitcast` constant folding
// the constant side becomes `arith.zext(arith.constant : u64) : u128`,
// reaching `Zext::fold`'s success path (U128 arm) which the corpus never hits.
// The constant must be positive as an i64: `materialize_constant` coerces the
// folded I64 immediate via `as_u64`, which fails for negative values.
//
// Constant-lhs shifts feed `hir.bitcast(constant)` folds whose materialization
// goes through `ArithDialect::materialize_constant`'s U32/U64 coercion arms.
//
// Also the first use of `i32.ctz`/`i64.ctz` on genuinely unknown values —
// LLVM constant-folds shapes like `(x | 1).trailing_zeros()`, so the existing
// `widening` case never emits a ctz.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = ((input1 as u64) << 32) | input2 as u64;

    // i64.mul_wide_u(x, C) with C positive in i64.
    let wide = (x as u128).wrapping_mul(0x1EAD_BEEF_CAFE_F00D_u64 as u128);
    let hi = (wide >> 64) as u64;
    let lo = wide as u64;

    // Dynamic wide multiply as well.
    let wide2 = (x as u128).wrapping_mul(input2 as u128);
    let hi2 = (wide2 >> 64) as u64;

    // Constant-lhs unsigned shifts: the constant is bitcast to U32/U64 by
    // translation and folds through the arith constant-materialization arms.
    let s32 = 0x0F0F_0F0Fu32 >> (input1 & 31);
    let s64 = 0x0FED_CBA9_8765_4321u64 >> (input2 & 63);

    // arith.ctz creation (i32.ctz / i64.ctz).
    let t1 = input1.trailing_zeros();
    let t2 = x.trailing_zeros();

    (hi ^ lo ^ hi2 ^ s64) as u32 ^ ((hi >> 32) as u32) ^ s32 ^ t1 ^ t2
}
