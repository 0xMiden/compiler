// Trap parity: two independent array bounds checks in one function, on a
// byte array and on a word array, with the second index computed from BOTH
// inputs. `input1 % 16` indexes a `[u8; 13]` (13..16 are out of range) and
// `(input1 ^ input2) % 7` indexes a `[u32; 5]` (5..7 are out of range); the
// first out-of-range index to execute panics with `index out of bounds`, so
// both targets must trap on exactly those rows.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let bytes: [u8; 13] = [9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 11, 12, 13];
    let words: [u32; 5] = [0x1111_1111, 0x2222_2222, 0x3333_3333, 4, 5];
    let i = (input1 % 16) as usize;
    let j = ((input1 ^ input2) % 7) as usize;
    let b = bytes[i] as u32;
    let w = words[j];
    b.wrapping_mul(w) ^ (i as u32) ^ (j as u32)
}
