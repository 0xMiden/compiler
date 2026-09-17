// Trap parity: the `i32` division family around the two panicking edges,
// divisor zero and `MIN / -1`. `input2 % 5` gives a divisor in -2..=2, so
// zero is reachable, and `input1 as i32` reaches `i32::MIN`. `div_euclid`,
// `rem_euclid` and `checked_div(..).unwrap()` must trap on both edges;
// `wrapping_div`, `overflowing_div` and `checked_rem(..).unwrap_or` are
// defined at `MIN / -1` and must not trap at all (their divisor `b | 1` is
// never zero, and is -1 exactly when `b` is -2 or -1).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32;
    let b = (input2 % 5) as i32 - 2;
    let nz = b | 1;
    let w = a.wrapping_div(nz);
    let (o, ovf) = a.overflowing_div(nz);
    let m = a.checked_rem(nz).unwrap_or(0);
    let de = a.div_euclid(b);
    let re = a.rem_euclid(b);
    let cd = a.checked_div(b).unwrap();
    (w as u32)
        ^ (o as u32)
        ^ (ovf as u32)
        ^ (m as u32)
        ^ (de as u32)
        ^ (re as u32)
        ^ (cd as u32)
}
