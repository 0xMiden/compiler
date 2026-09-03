// `core::fmt` into a stack buffer (campaign 17, program 13): a `fmt::Write`
// implementation over a 160-byte stack buffer receives `write!` of the
// inputs in decimal, hex (lower / upper, zero-padded), signed with an
// explicit sign, width-padded and aligned, alternate binary, octal, a
// `Debug` byte slice and a `Debug` tuple — i.e. the real `core::fmt`
// machinery (`Arguments`, `Formatter`, `pad_integral`, `DebugList`,
// `dyn Write` vtable dispatch) running on the VM — then the decimal field
// is parsed back with `str::parse::<u32>` and the hex field with
// `u32::from_str_radix` (the fields are located by a hand-written byte
// scan: `str::split` compares slices with `==`, which lowers to a `memcmp`
// libcall the guest link does not provide); the text, its length, the
// parse results and the overflow status are folded into the result.
use core::fmt::{self, Write};

struct Buf {
    data: [u8; 160],
    len: usize,
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.len + bytes.len() > self.data.len() {
            return Err(fmt::Error);
        }
        self.data[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

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

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf {
        data: [0u8; 160],
        len: 0,
    };
    let s = input2 as i32;
    let bytes = input1.to_le_bytes();
    let width = (input2 % 12) as usize;
    let r = write!(
        b,
        "{}|{:x}|{:08X}|{:+}|{:>6}|{:<4}|{:#b}|{:o}|{:?}|{:?}|{:>w$}|{:^9}",
        input1,
        input1,
        input2,
        s,
        input1 % 1000,
        s % 100,
        input1 & 0xff,
        input2 >> 20,
        &bytes[..(1 + (input2 % 4) as usize)],
        (input1 >> 16, s >> 16),
        input2 % 100,
        (input1 as u16) as i16,
        w = width,
    );
    let ok = r.is_ok() as u32;
    let dec = field(&b, 0).parse::<u32>().unwrap_or(0xdead_beef);
    let hex = u32::from_str_radix(field(&b, 1), 16).unwrap_or(0xdead_beef);
    let signed = field(&b, 3).parse::<i32>().unwrap_or(-1);
    let roundtrip =
        (dec == input1) as u32 | ((hex == input1) as u32) << 1 | ((signed == s) as u32) << 2;
    let mut h = 0x811c_9dc5u32 ^ (b.len as u32) << 20 ^ ok << 30 ^ roundtrip << 27;
    let mut i = 0usize;
    while i < b.len {
        h ^= b.data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}
