// Trap parity: `u32` and `i32` division by runtime divisors. `input2 % 4 == 0`
// and `input2 == 0` divide by zero (`attempt to divide by zero`), and
// `i32::MIN / -1` overflows (`attempt to divide with overflow`); every such
// row must trap on both targets, every other row must agree on the value.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let q = input1 / (input2 % 4);
    let s = (input1 as i32) / (input2 as i32);
    q ^ (s as u32)
}
