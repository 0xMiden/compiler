// A wire-record patcher over a byte buffer (campaign 27, program 10):
// records are read at runtime offsets with `core::ptr::read_unaligned` at
// u16 / u32 / u64 widths, rewritten with `write_unaligned`, moved with
// `copy` (overlapping, distinct positions) and `copy_nonoverlapping`,
// cleared with `write_bytes`, and located with `slice::iter().position`.
// Every offset is masked into range, so no read or write can leave the
// buffer; the whole buffer is hashed at the end.

const LEN: usize = 128;

fn fill(buf: &mut [u8; LEN], input1: u32, input2: u32) {
    let mut i = 0usize;
    while i < LEN {
        let v = input1.wrapping_mul(i as u32 + 7) ^ input2.rotate_left(i as u32 & 31);
        buf[i] = (v >> (i as u32 & 7)) as u8;
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u8; LEN];
    fill(&mut buf, input1, input2);

    let a = (input1 % 64) as usize;
    let b = (input2 % 48) as usize;
    let n = 1 + (input2 % 16) as usize;
    let mut acc = 0u32;

    unsafe {
        let p = buf.as_mut_ptr();

        // Unaligned reads at three widths.
        let r16 = core::ptr::read_unaligned(p.add(a) as *const u16);
        let r32 = core::ptr::read_unaligned(p.add(a + 1) as *const u32);
        let r64 = core::ptr::read_unaligned(p.add(a + 3) as *const u64);
        acc = acc
            .wrapping_add(r16 as u32)
            .wrapping_add(r32)
            .wrapping_add((r64 >> 32) as u32 ^ r64 as u32);

        // Patch two fields in place.
        core::ptr::write_unaligned(p.add(b) as *mut u32, r32 ^ 0x5a5a_5a5a);
        core::ptr::write_unaligned(p.add(b + 5) as *mut u16, r16.rotate_left(3));

        // Move a record forward (overlapping ranges, never src == dst).
        let src = a % 32;
        core::ptr::copy(p.add(src), p.add(src + 8), 16);
        // And clone one into the scratch area at the end.
        core::ptr::copy_nonoverlapping(p.add(b), p.add(LEN - 24), n);
        // Clear a field.
        core::ptr::write_bytes(p.add(64 + b % 16), input1 as u8, n);

        // Read the patched fields back.
        acc = acc.wrapping_add(core::ptr::read_unaligned(p.add(b) as *const u32));
        acc = acc.wrapping_add(core::ptr::read_unaligned(p.add(src + 8) as *const u32));
        acc = acc.wrapping_add(core::ptr::read_unaligned(p.add(LEN - 24) as *const u16) as u32);
        acc = acc.wrapping_add(core::ptr::read_volatile(p.add(LEN - 1)) as u32);

        // Pointer arithmetic a record walker does.
        let start = p.add(a) as *const u8;
        let end = p.add(LEN) as *const u8;
        acc = acc.wrapping_add(end.offset_from(start) as u32);
        acc = acc.wrapping_add(start.align_offset(4) as u32);
    }

    acc = acc.wrapping_add(buf.iter().position(|&x| x == input2 as u8).unwrap_or(LEN) as u32);
    acc = acc.wrapping_add(buf.iter().rposition(|&x| x == 0).unwrap_or(LEN) as u32);

    let mut h = 2166136261u32;
    let mut i = 0usize;
    while i < LEN {
        h = (h ^ buf[i] as u32).wrapping_mul(16777619);
        i += 1;
    }
    acc.wrapping_add(h)
}
