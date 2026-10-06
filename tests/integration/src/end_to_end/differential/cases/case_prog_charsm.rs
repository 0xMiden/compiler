// A UTF-8 state machine over an input-derived byte stream (campaign 27,
// program 7): the inputs are encoded as a mixture of ASCII, two- and
// three-byte code points with `char::encode_utf8`, malformations are planted
// at input-selected offsets (a bare continuation byte, a truncated sequence,
// a surrogate-range lead), and the stream is then decoded chunk by chunk
// with `core::str::from_utf8`, whose `Utf8Error::valid_up_to` /
// `error_len` drive the resynchronisation. Decoded characters run through
// `to_digit`, `is_ascii_*`, `to_ascii_uppercase` and `char::from_u32`.

fn encode(buf: &mut [u8; 128], input1: u32, input2: u32) -> usize {
    let mut n = 0usize;
    let mut i = 0u32;
    while i < 10 && n + 4 <= buf.len() {
        let v = input1.rotate_left(i * 3) ^ input2.wrapping_mul(i + 1);
        // Spread the code points over the one-, two- and three-byte ranges.
        let cp = match i % 3 {
            0 => 0x20 + v % 0x5f,
            1 => 0x80 + v % 0x700,
            _ => 0x800 + v % 0x800,
        };
        let c = char::from_u32(cp).unwrap_or('?');
        n += c.encode_utf8(&mut buf[n..]).len();
        i += 1;
    }
    // Plant a malformation whose kind and position both depend on the input.
    if n > 8 {
        let at = 1 + (input2 as usize) % (n - 2);
        match input1 % 4 {
            0 => {}
            1 => buf[at] = 0x80,        // stray continuation byte
            2 => buf[at] = 0xe0,        // truncated three-byte sequence
            _ => buf[at] = 0xed,        // surrogate-range lead byte
        }
    }
    n
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; 128];
    let n = encode(&mut buf, input1, input2);

    let mut acc = n as u32;
    let mut faults = 0u32;
    let mut chars = 0u32;
    let mut digits = 0u32;
    let mut pos = 0usize;

    // Decode with resynchronisation, the way a lenient reader does it.
    while pos < n {
        match core::str::from_utf8(&buf[pos..n]) {
            Ok(text) => {
                for (i, c) in text.char_indices() {
                    chars += 1;
                    acc = acc.wrapping_mul(31).wrapping_add(c as u32 ^ i as u32);
                    if let Some(d) = c.to_digit(16) {
                        digits += 1;
                        acc = acc.wrapping_add(d * 7);
                    }
                    acc = acc.wrapping_add(c.is_ascii_alphanumeric() as u32);
                    acc = acc.wrapping_add(c.is_ascii_whitespace() as u32 * 2);
                    acc = acc.wrapping_add(c.to_ascii_uppercase() as u32 >> 2);
                    acc = acc.wrapping_add(c.len_utf8() as u32 * 3);
                }
                break;
            }
            Err(e) => {
                faults += 1;
                let valid = e.valid_up_to();
                acc = acc.wrapping_mul(17).wrapping_add(valid as u32);
                if let Ok(good) = core::str::from_utf8(&buf[pos..pos + valid]) {
                    chars += good.chars().count() as u32;
                    acc = acc.wrapping_add(good.chars().map(|c| c as u32).sum::<u32>());
                }
                match e.error_len() {
                    Some(len) => {
                        acc = acc.wrapping_add(0x100 * len as u32);
                        pos += valid + len;
                    }
                    None => {
                        // Truncated tail: nothing more to decode.
                        acc = acc.wrapping_add(0x1000);
                        break;
                    }
                }
            }
        }
    }

    // Re-encode the decoded prefix and compare byte counts (a round trip a
    // user writes to validate a transcoder).
    let mut out = [0u8; 128];
    let mut m = 0usize;
    if let Ok(text) = core::str::from_utf8(&buf[..core::cmp::min(n, 6)]) {
        for c in text.chars() {
            let up = c.to_ascii_uppercase();
            if m + 4 <= out.len() {
                m += up.encode_utf8(&mut out[m..]).len();
            }
        }
    }
    let mut h = 2166136261u32;
    let mut j = 0usize;
    while j < m {
        h = (h ^ out[j] as u32).wrapping_mul(16777619);
        j += 1;
    }

    acc.wrapping_add(faults * 1009)
        .wrapping_add(chars * 13)
        .wrapping_add(digits * 5)
        .wrapping_add(m as u32)
        .wrapping_add(h)
}
