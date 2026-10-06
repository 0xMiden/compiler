// Trap parity: the 64-bit `checked_*(..).unwrap()` boundaries. The shift
// count `input2 % 80` reaches and passes the 64-bit width, where
// `checked_shl` / `checked_shr` panic while `wrapping_shl` and
// `rotate_right` mask the count and must not; `checked_add(1)` panics only
// at `u64::MAX` (which the harness's forced-equal `(MAX, MAX)` draw and the
// pinned grid both reach); `checked_pow` panics when the u64 power
// overflows. The signed `checked_shr` keeps the i64 arithmetic-shift
// lowering in the picture. No i64 `checked_mul`/`checked_pow` here: those
// are the known wide-arithmetic guest-toolchain family.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let w = ((input1 as u64) << 32) | (input2 as u64);
    let s = input2 % 80;
    let ws = w.wrapping_shl(s) ^ w.rotate_right(s);
    let shl = w.checked_shl(s).unwrap();
    let shr = (w as i64).checked_shr(s).unwrap() as u64;
    let add = w.checked_add(1).unwrap();
    let pw = ((input1 & 0xffff) as u64).checked_pow(input2 % 6).unwrap();
    let r = ws ^ shl ^ shr ^ add ^ pw;
    (r as u32) ^ ((r >> 32) as u32)
}
