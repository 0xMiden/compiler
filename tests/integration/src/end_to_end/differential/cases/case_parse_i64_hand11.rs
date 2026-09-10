// Discriminator for `case_parse_i64_hand.rs`: identical, except that the
// plain wrapping path multiplies by 11 instead of 10, so the two paths no
// longer share the constant value. Native and MASM must agree; if they do
// while the shared-constant twin diverges, the shared value is the trigger.
static DIGITS: [u8; 16] = *b"9007199254740993";

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let end = 1 + (input2 as usize % 16);
    let v: i64 = if end == 16 {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < 16 {
            let d = (DIGITS[i] - b'0') as i64;
            acc = match acc.checked_mul(10).and_then(|a| a.checked_add(d)) {
                Some(a) => a,
                None => return 0xdead_beef,
            };
            i += 1;
        }
        acc
    } else {
        let mut acc: i64 = 0;
        let mut i = 0;
        while i < end {
            acc = acc.wrapping_mul(11).wrapping_add((DIGITS[i] - b'0') as i64);
            i += 1;
        }
        acc
    };
    ((v as u32) ^ ((v >> 32) as u32)).wrapping_add(input1 & 1)
}
