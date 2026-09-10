// A `{:?}` / `{:#?}` dump of a nested value graph (campaign 27, program 3):
// derived `Debug` on a struct holding an enum with struct-, tuple- and
// unit-variants, a fixed array, a runtime-length slice, `Option`s, a `&str`,
// a `char`, a tuple and a `Result` — i.e. `DebugStruct`, `DebugTuple`,
// `DebugList`, the alternate-mode `PadAdapter` indentation and `char` /
// `str` escaping, all running on the VM. Both the compact and the pretty
// rendering go into one stack buffer, and the bytes plus the two lengths and
// the indentation profile are folded into the result.

use core::fmt::{self, Write};

#[derive(Debug, Clone, Copy)]
enum Node {
    Leaf,
    Value(u32, i16),
    Branch { left: u8, right: u8, mask: u16 },
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    id: u16,
    node: Node,
    bytes: [u8; 4],
    tag: Option<char>,
    name: &'static str,
    span: (u16, i8),
    state: Result<u8, Fault>,
}

#[derive(Debug, Clone, Copy)]
enum Fault {
    Reset,
    Code(u16),
}

struct Buf {
    data: [u8; 512],
    len: usize,
}

impl Write for Buf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > self.data.len() {
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

fn frame(i: u32, input1: u32, input2: u32) -> Frame {
    let v = input1.rotate_left(i * 5) ^ input2.wrapping_mul(i + 1);
    Frame {
        id: v as u16,
        node: match v % 3 {
            0 => Node::Leaf,
            1 => Node::Value(v >> 3, v as i16),
            _ => Node::Branch {
                left: v as u8,
                right: (v >> 8) as u8,
                mask: (v >> 16) as u16,
            },
        },
        bytes: v.to_le_bytes(),
        tag: char::from_u32(0x20 + v % 0x40),
        name: match v % 4 {
            0 => "alpha",
            1 => "b\"eta",
            2 => "",
            _ => "gamma\n",
        },
        span: ((v >> 7) as u16, v as i8),
        state: if v & 1 == 0 {
            Ok(v as u8)
        } else if v & 2 == 0 {
            Err(Fault::Reset)
        } else {
            Err(Fault::Code(v as u16))
        },
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let frames: [Frame; 4] = core::array::from_fn(|i| frame(i as u32, input1, input2));
    let n = 1 + (input2 % 4) as usize;
    let mut b = Buf {
        data: [0u8; 512],
        len: 0,
    };
    let compact = write!(b, "{:?}", &frames[..n]).is_ok();
    let compact_len = b.len;
    let pretty = write!(b, "{:#?}", frames[(input1 % 4) as usize]).is_ok();
    let pretty_len = b.len - compact_len;
    let opt: Option<u16> = if input1 & 4 == 0 { None } else { Some(input2 as u16) };
    let _ = write!(b, "{:?}|{:?}|{:?}", opt, (input1 as u8, 'x'), Some(frames[0].node));
    let mut h = 2166136261u32;
    let mut nl = 0u32;
    let mut indent = 0u32;
    let mut i = 0usize;
    while i < b.len {
        let c = b.data[i];
        h = (h ^ c as u32).wrapping_mul(16777619);
        nl += (c == b'\n') as u32;
        indent += (c == b' ') as u32;
        i += 1;
    }
    h.wrapping_add(compact_len as u32)
        .wrapping_add((pretty_len as u32) << 3)
        .wrapping_add(nl * 101)
        .wrapping_add(indent * 7)
        .wrapping_add(compact as u32)
        .wrapping_add(pretty as u32 * 2)
}
