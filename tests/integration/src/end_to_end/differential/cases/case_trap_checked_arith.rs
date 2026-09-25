// Trap parity: `checked_*(..).unwrap()` at the exact 32-bit overflow
// boundaries, next to the siblings that must NOT trap there. Guest builds
// have overflow checks off, so `a + 1` at `u32::MAX` wraps silently — only
// the `checked_` form panics, and it must do so on both targets for exactly
// `input1 == u32::MAX` (add), `input2 == 0` (sub) and
// `input1 == i32::MIN` (neg).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1;
    let b = input2;
    let sat = a.saturating_add(1) ^ b.saturating_sub(1);
    let dflt = a.checked_add(b).unwrap_or_default();
    let mapped = a.checked_mul(b).map_or(0x0000_dead, |v| v);
    let or = (a as i32).checked_abs().unwrap_or(-1);
    let add = a.checked_add(1).unwrap();
    let sub = b.checked_sub(1).unwrap();
    let neg = (a as i32).checked_neg().unwrap();
    sat ^ dflt ^ mapped ^ (or as u32) ^ add ^ sub ^ (neg as u32)
}
