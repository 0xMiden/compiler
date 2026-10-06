// The variant of `case_prog_numeric.rs` that compiles at every optimization
// level (campaign 27, program 6): the same measurement pipeline over the
// `core::num` families, with TWO of the full program's five per-iteration
// blocks removed — the 64-bit lane (`wide`) and the byte-order round trips
// (`to_le_bytes` / `to_be_bytes` / `from_*_bytes` / `swap_bytes` /
// `reverse_bits`). The i8 / u16 / i32 lanes, the classification calls
// (`leading_zeros` / `count_ones` / `checked_ilog2` / `isqrt` / `abs_diff` /
// `pow`), the Euclidean division and all four overflow families, plus the
// `from_str_radix` text leg, are unchanged.
//
// The measured ladder (default / size-min / max / basic), each rung
// value-checked natively on the 1225-pair boundary grid:
//   full program                     panic / OK    / panic / panic
//   minus the 64-bit lane            panic / OK    / panic / panic
//   minus the byte-order block       OK    / OK    / OK    / panic
//   minus both (this case)           OK    / OK    / OK    / OK
// No single construct is the lever: dropping either block on its own is
// enough at the default level, so what fails is the total number of values
// the loop body keeps live, not any one `core::num` API.

fn reading(i: u32, input1: u32, input2: u32) -> u32 {
    input1.rotate_left(i & 31) ^ input2.wrapping_mul(i.wrapping_add(1))
}

// Render `v` in base 16 into `buf` and return the text, so the parse leg has
// something the program itself produced.
fn hex_text(buf: &mut [u8; 8], mut v: u32) -> &str {
    let mut i = 8usize;
    loop {
        i -= 1;
        let d = (v & 0xf) as u8;
        buf[i] = if d < 10 { b'0' + d } else { b'a' + d - 10 };
        v >>= 4;
        if v == 0 || i == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[i..]).unwrap_or("0")
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = 0u32;
    let mut small = 0i8;
    let mut mid = 0u16;

    let mut i = 0u32;
    while i < 12 {
        let r = reading(i, input1, input2);
        let s = r as i32;

        // Narrow lanes with their own overflow behaviour.
        small = small.wrapping_add(r as i8).saturating_mul(if i & 1 == 0 { 1 } else { -1 });
        mid = mid.checked_add(r as u16).unwrap_or_else(|| (r as u16) >> 1);
        mid = mid.rotate_left(i & 15) ^ (r as u16).reverse_bits();

        // Classification.
        acc = acc.wrapping_add(r.leading_zeros() ^ r.trailing_zeros());
        acc = acc.wrapping_add(r.count_ones().wrapping_mul(3));
        acc = acc.wrapping_add(r.checked_ilog2().unwrap_or(99));
        acc = acc.wrapping_add(r.isqrt());
        acc = acc.wrapping_add(r.abs_diff(input2));
        acc = acc.wrapping_add(s.abs_diff(input1 as i32));
        acc = acc.wrapping_add((r as u16).pow(2) as u32);

        // Euclidean division against a non-zero divisor.
        let d = (r % 97) as i32 + 1;
        acc = acc.wrapping_add(s.div_euclid(d) as u32 ^ s.rem_euclid(d) as u32);
        acc = acc.wrapping_add((r / (1 + r % 13)) ^ (input1 % (1 + r % 7)));

        // The four overflow families on one operand pair.
        let (ov, o) = r.overflowing_mul(input2 | 1);
        acc = acc.wrapping_add(ov ^ (o as u32) << 16);
        acc = acc.wrapping_add(r.checked_sub(input2).unwrap_or(0xfeed));
        acc = acc.wrapping_add(r.saturating_add(input2));
        acc = acc.wrapping_add(s.checked_neg().unwrap_or(i32::MAX) as u32);
        acc = acc.wrapping_add(s.saturating_abs() as u32);
        acc = acc.wrapping_add((r as u8).checked_shl(i).unwrap_or(0) as u32);

        i += 1;
    }

    // Text round trip of a value the program printed itself.
    let mut buf = [0u8; 8];
    let text = hex_text(&mut buf, acc);
    let back = u32::from_str_radix(text, 16).unwrap_or(0xdead_beef);
    let signed = i32::from_str_radix("-2147483648", 10).unwrap_or(1);
    let bad = u32::from_str_radix("zz", 16).is_err() as u32;

    acc.wrapping_add(back)
        .wrapping_add(signed as u32)
        .wrapping_add(bad * 31)
        .wrapping_add(small as u32)
        .wrapping_add(mid as u32)
}
