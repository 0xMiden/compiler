// Bounded loop with multiple carried values and an inner branch.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let limit = (input1 & 7).wrapping_add(1);
    let mut i = 0;
    let mut acc = input1 ^ input2;
    let mut carry = input2.wrapping_add(3);

    while i < limit {
        if (acc ^ carry ^ i) & 1 == 0 {
            acc = acc.wrapping_add(carry ^ i);
        } else {
            acc = acc.wrapping_sub(carry.wrapping_add(i));
        }
        carry = carry.wrapping_add(acc ^ limit);
        i = i.wrapping_add(1);
    }

    acc ^ carry ^ i
}
