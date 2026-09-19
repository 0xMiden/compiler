// Trap parity: the `core` validating constructors panicking through
// `unwrap`/`expect`. `str::from_utf8` sees a two-byte sequence whose
// continuation byte is replaced by an ASCII byte when `input1 % 11 == 7`,
// which makes the buffer invalid; `char::from_u32` rejects the surrogate
// range `0xd800..0xe000`; `NonZeroU32::new` rejects zero; and a hand-written
// `Err` is unwrapped for `input2 % 9 == 3`. Each panic site has its own
// predicate, so the pinned grid can reach all four independently.
fn classify(v: u32) -> Result<u32, ()> {
    if v % 9 == 3 {
        Err(())
    } else {
        Ok(v.wrapping_mul(0x0100_0193))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cont = if input1 % 11 == 7 {
        0x20u8
    } else {
        0x80 | (input1 & 0x3f) as u8
    };
    let bytes: [u8; 4] = [b'a', 0xc3, cont, b'z'];
    let s = core::str::from_utf8(&bytes).expect("invalid utf-8");
    let c = char::from_u32(input2 % 0x11_0000).expect("not a scalar value");
    let nz = core::num::NonZeroU32::new(input1 % 4).unwrap();
    let r = classify(input2).unwrap();
    (s.len() as u32)
        .wrapping_mul(0x0100_0193)
        ^ (c as u32)
        ^ nz.get()
        ^ r
}
