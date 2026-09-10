// A report formatter (campaign 27, program 2): three rows of a table are
// written into a 256-byte stack buffer through a `fmt::Write` implementation
// with runtime width, runtime fill/alignment, zero padding, explicit sign,
// lower/upper hex, binary and octal radices and a `{:e}`-free precision
// field, then the printed decimal and hex columns are located by a
// hand-written byte scan (`str::split` is not linkable here) and parsed back
// with `str::parse` / `u32::from_str_radix`, and the whole buffer is hashed.
// This is `core::fmt`'s `Formatter::pad` / `pad_integral` / `write_formatted`
// machinery plus `core::num`'s `from_str` running on the VM.

use core::fmt::{self, Write};

struct Buf {
    data: [u8; 256],
    len: usize,
    truncated: bool,
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > self.data.len() {
            self.truncated = true;
            return Err(fmt::Error);
        }
        let mut i = 0usize;
        while i < b.len() {
            self.data[self.len + i] = b[i];
            i += 1;
        }
        self.len += b.len();
        Ok(())
    }
}

// The `index`-th `|`-separated field of the buffer, as a `&str`.
fn field(buf: &Buf, index: u32) -> &str {
    let mut start = 0usize;
    let mut k = 0u32;
    let mut i = 0usize;
    while i < buf.len && k < index {
        if buf.data[i] == b'|' {
            k += 1;
            start = i + 1;
        }
        i += 1;
    }
    let mut end = start;
    while end < buf.len && buf.data[end] != b'|' {
        end += 1;
    }
    core::str::from_utf8(&buf.data[start..end]).unwrap_or("")
}

fn row(b: &mut Buf, label: &str, value: u32, signed: i32, width: usize) -> fmt::Result {
    write!(b, "{label}|{value}|{value:x}|{value:08X}|{signed:+}|")?;
    write!(b, "{value:>width$}|{signed:<8}|{:#b}|{:o}|", value & 0xff, value >> 21)?;
    write!(b, "{:.3}|{:^7}|{:+06}|", value % 1000, label, signed % 1000)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf {
        data: [0u8; 256],
        len: 0,
        truncated: false,
    };
    let width = (input2 % 14) as usize;
    let mut written = 0u32;
    for (i, label) in ["one", "two", "three"].iter().enumerate() {
        let v = input1.rotate_left(i as u32 * 7) ^ (input2.wrapping_mul(i as u32 + 1));
        let s = (v as i32) / (1 + i as i32);
        if row(&mut b, label, v, s, width + i).is_ok() {
            written += 1;
        }
    }
    // Round-trip the first row's decimal and hex columns.
    let dec = field(&b, 1).parse::<u32>().unwrap_or(0xdead_beef);
    let hex = u32::from_str_radix(field(&b, 2), 16).unwrap_or(0xdead_beef);
    let padded = u32::from_str_radix(field(&b, 3), 16).unwrap_or(0xdead_beef);
    let sign = field(&b, 4).parse::<i32>().unwrap_or(-7);
    let roundtrip = (dec == hex) as u32 + ((hex == padded) as u32) * 2 + ((sign != 0) as u32) * 4;
    let mut h = 2166136261u32;
    let mut i = 0usize;
    while i < b.len {
        h = (h ^ b.data[i] as u32).wrapping_mul(16777619);
        i += 1;
    }
    h.wrapping_add(b.len as u32)
        .wrapping_add(written * 31)
        .wrapping_add(roundtrip)
        .wrapping_add(b.truncated as u32 * 1024)
}
