// Nearest PASSING neighbours of the i64 F9 family (checked_mul_i64 /
// sat_mul_i64 / pow_i64): the UNSIGNED overflow-checked multiplies —
// u64 `checked_mul`, `overflowing_mul` (flag as a value and feeding a
// `break`), `saturating_mul` and `checked_pow` — in `#[inline(never)]`
// helper and loop forms (ovf_mul / int_logs cover the straight-line forms).
// Their overflow test is `hi != 0` on the `i64.mul_wide_u` high word, a
// unary `i64.eqz` with no second operand subtree for the multiply to sink
// into, so LLVM stackifies every form in a valid order: standalone builds
// agree with native at every opt-level and debuginfo level.
#[inline(never)]
fn forms(x: u64, y: u64) -> u64 {
    let c = x.checked_mul(y).unwrap_or(0x8888_8888_8888_8888);
    let (v, o) = x.overflowing_mul(y | 1);
    let s = x.saturating_mul(y);
    let p = x.checked_pow((y & 7) as u32).unwrap_or(0x1ced_c0ff_ee15_600d);
    c ^ (v ^ ((o as u64) << 1)).rotate_left(5) ^ s.rotate_left(11) ^ p.rotate_left(17)
}

#[inline(never)]
fn forms_loop(x0: u64, y: u64, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let (v, o) = acc.overflowing_mul(y | 1);
        if o {
            r ^= 0xdead;
            break;
        }
        let c = match acc.checked_mul(y | 3) {
            Some(w) => w,
            None => acc ^ y,
        };
        let s = acc.saturating_mul(y | 5);
        let p = acc.checked_pow((y & 3) as u32).unwrap_or(0x1ced_c0ff_ee15_600d);
        r ^= (v ^ c.rotate_left(7) ^ s.rotate_left(13) ^ p.rotate_left(19)).rotate_left(i);
        acc = v ^ (i as u64);
        i = i.wrapping_add(1);
    }
    r ^ acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b = ((input2 as u64) << 32) | input2 as u64;
    let m = forms(a, b)
        ^ forms(b, a >> 1).rotate_left(3)
        ^ forms_loop(a, b, (input2 & 7).wrapping_add(1)).rotate_left(9);
    (m as u32) ^ ((m >> 32) as u32)
}
