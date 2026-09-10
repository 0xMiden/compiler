// A measurement pipeline over the integer APIs (campaign 27, program 6):
// readings are widened and narrowed across i8 / u16 / i32 / u64, combined
// with the `checked_*` / `saturating_*` / `overflowing_*` / `wrapping_*`
// families, classified with `leading_zeros` / `count_ones` / `ilog2` /
// `isqrt` / `pow` / `abs_diff`, serialized with `to_le_bytes` /
// `to_be_bytes` and read back with `from_le_bytes` / `from_be_bytes`,
// permuted with `swap_bytes` / `reverse_bits` / `rotate_*`, and finally
// re-parsed from text with `u32::from_str_radix` / `i32::from_str_radix`.
// No 128-bit types (the F9 guest-toolchain family) and no floats.

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
    let mut wide = 0u64;
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

        // Wide lane, fed by the pairs the readings form.
        let pair = ((r as u64) << 32) | reading(i + 1, input1, input2) as u64;
        wide = wide.wrapping_add(pair.swap_bytes()) ^ pair.rotate_right(i & 63);
        wide = wide.saturating_sub(u64::from(r)).wrapping_mul(0x9e37_79b9);

        // Classification.
        acc = acc.wrapping_add(r.leading_zeros() ^ r.trailing_zeros());
        acc = acc.wrapping_add(r.count_ones().wrapping_mul(3));
        acc = acc.wrapping_add(r.checked_ilog2().unwrap_or(99));
        acc = acc.wrapping_add(r.isqrt() ^ wide.isqrt() as u32);
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

        // Byte order round trips.
        let le = r.to_le_bytes();
        let be = r.to_be_bytes();
        acc = acc.wrapping_add(u32::from_be_bytes(le) ^ u32::from_le_bytes(be));
        let w = wide.to_le_bytes();
        acc = acc.wrapping_add(u64::from_be_bytes(w) as u32);
        acc = acc.wrapping_add(u16::from_le_bytes([le[0], be[3]]) as u32);
        acc = acc.wrapping_add(r.swap_bytes() ^ r.reverse_bits());

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
        .wrapping_add(wide as u32 ^ (wide >> 32) as u32)
}
