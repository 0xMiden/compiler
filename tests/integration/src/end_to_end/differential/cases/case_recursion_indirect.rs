// PROBE: bounded recursion THROUGH A FUNCTION-POINTER TABLE (depth
// input1 % 6, per-frame state). No direct call edge closes the cycle: each
// frame loads its callee from a runtime-indexed static table and dispatches
// via `call_indirect`, so the linker's direct call-graph cycle check does
// not see a cycle. Natively this is ordinary recursion.
type Step = fn(u32, u64) -> u64;

#[inline(never)]
fn leaf(_n: u32, s: u64) -> u64 {
    s.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

#[inline(never)]
fn rec_a(n: u32, s: u64) -> u64 {
    if n == 0 {
        return s;
    }
    let f = STEPS[((s >> 3) % 3) as usize];
    let r = f(n - 1, s ^ (n as u64).wrapping_mul(0x9e37_79b9));
    r.rotate_left(n) ^ s
}

#[inline(never)]
fn rec_b(n: u32, s: u64) -> u64 {
    if n == 0 {
        return !s;
    }
    let f = STEPS[(s % 3) as usize];
    let r = f(n - 1, s.wrapping_add(n as u64));
    r.rotate_right(n) ^ (s << 1)
}

static STEPS: [Step; 3] = [leaf, rec_a, rec_b];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = ((input1 as u64) << 32) | input2 as u64;
    let f = STEPS[(input2 % 3) as usize];
    let r = f(input1 % 6, s | 1);
    (r as u32) ^ ((r >> 32) as u32)
}
