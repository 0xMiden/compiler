// Unsigned division/remainder boundary relations on u32 and u64, divisors
// guarded non-zero with `| 1`. Edge relations asserted by the pinned grid:
// divisor 1, divisor == dividend (q=1, r=0), divisor > dividend (q=0, r=x),
// dividend 0, MAX/1, MAX/MAX, and u64 divisors with the high bit set (the
// largest-divisor path of miden-core-lib `u64::div`). The u64 dividend and
// divisor mirror each other's words from the same input pair, so single grid
// rows force each exact relation (e.g. (5,5) -> equal, (0,0) -> 0/1). Every
// `%` uses a mirrored operand pair with no matching `/`, so LLVM cannot
// strength-reduce it to mul-sub and the VM-side mod ops really execute.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // u32: division and remainder on mirrored pairs (never the same pair).
    let q32 = input1 / (input2 | 1);
    let r32 = input2 % (input1 | 1);

    // u64: word-mirrored dividend/divisor from the same pair.
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = (((input2 as u64) << 32) | input1 as u64) | 1;
    let q64 = a / b;
    let r64 = b % (a | 1);

    let m = q64 ^ r64.rotate_left(17);
    q32.wrapping_add(r32.rotate_left(7))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}
