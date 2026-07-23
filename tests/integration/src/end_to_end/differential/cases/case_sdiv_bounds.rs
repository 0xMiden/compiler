// Signed division boundary relations with MIN/-1 unconstructible by design
// (numerators over negative divisors are forced odd via `| 1`; MIN is even).
// Edge relations asserted by the pinned grid — the row (0x80000000, 0) forces
// all divisors to magnitude 1 at once: i32 MIN/1 == MIN, (MIN|1)/-1 == MAX,
// i64::MIN/1 == i64::MIN, (i64::MIN|1)/-1 == i64::MAX; the rotated remainder
// numerators pin MIN % 1 == 0 (row (0x00008000, 0)) and (MIN|1) % -1 == 0
// (row (0x00800000, 0)) — remainders use rotated numerators so no `%` shares
// an operand pair with a `/` (LLVM would strength-reduce it to mul-sub and
// the VM-side wrapping_mod would never run). i64 `%` is deliberately absent
// (compile-time unimplemented, see i64_srem).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input1 as i32;
    let nr = input1.rotate_left(16) as i32; // remainder numerator (pin via rotation)
    let mr = (input1.rotate_left(8) as i32) | 1; // odd remainder numerator
    let dp = ((input2 % 1000) as i32) + 1; // dynamic divisor in [1, 1000]
    let dn = -((((input2 >> 16) % 1000) as i32) + 1); // dynamic divisor in [-1000, -1]

    let q1 = n / dp; // MIN / 1 reachable
    let r1 = nr % dp; // MIN % 1 == 0 reachable
    let q2 = (n | 1) / dn; // odd numerator / negative divisor (never MIN/-1)
    let r2 = mr % dn; // odd numerator % negative divisor (never MIN/-1)

    // i64: numerator from both words; both divisors derived from input2 so a
    // single row can pin numerator == i64::MIN with divisors 1 and -1.
    let w = (((input1 as u64) << 32) | input2 as u64) as i64;
    let dp64 = ((input2 % 100_000) as i64) + 1; // [1, 100000]
    let dn64 = -((((input2 >> 1) % 1000) as i64) + 1); // [-1000, -1]
    let q3 = w / dp64; // i64::MIN / 1 reachable
    let q4 = (w | 1) / dn64; // odd numerator / negative divisor (never MIN/-1)

    let m = (q3 as u64) ^ (q4 as u64).rotate_left(13);
    (q1 as u32)
        .wrapping_add((r1 as u32).rotate_left(5))
        .wrapping_add((q2 as u32).rotate_left(11))
        .wrapping_add((r2 as u32).rotate_left(19))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}
