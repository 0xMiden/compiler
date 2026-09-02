// Static data segments of mixed types read by runtime index: [u8; 13],
// [u16; 7], [u32; 5], [u64; 3], a packed static record (unaligned u32/u64
// fields inside .rodata), a `&str`, a zero-initialised atomic (.bss) next
// to a non-zero one (.data, updated across two `#[inline(never)]` helper
// calls and restored), a 4 KiB byte table indexed by both inputs, and
// interior slices (`&S[3..]`) passed to a helper by reference. The layout
// mixes alignments 1/2/4/8 in one .rodata segment, so every table starts
// at a different byte offset within a Miden word.
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Rec {
    tag: u8,
    a: u32,
    b: u64,
    c: u16,
}

static B13: [u8; 13] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9];
static H7: [u16; 7] = [0x1234, 0xfedc, 0x0001, 0x8000, 0x7fff, 0x00ff, 0xff00];
static W5: [u32; 5] = [0xdead_beef, 0x0badf00d, 1, 0x8000_0000, 0xffff_ffff];
static Q3: [u64; 3] = [0x0102_0304_0506_0708, 0xffff_ffff_0000_0001, 0x8000_0000_0000_0000];
static RECS: [Rec; 3] = [
    Rec {
        tag: 1,
        a: 0x1111_2222,
        b: 0x3333_4444_5555_6666,
        c: 0x7777,
    },
    Rec {
        tag: 2,
        a: 0x8888_9999,
        b: 0xaaaa_bbbb_cccc_dddd,
        c: 0xeeee,
    },
    Rec {
        tag: 3,
        a: 0xf0f0_0f0f,
        b: 0x0123_4567_89ab_cdef,
        c: 0x0101,
    },
];
static TEXT: &str = "miden memory layout campaign thirteen";
static ZERO: AtomicU32 = AtomicU32::new(0);
static COUNTER: AtomicU32 = AtomicU32::new(0x0badc0de);

const fn table() -> [u8; 4096] {
    let mut t = [0u8; 4096];
    let mut i = 0;
    while i < 4096 {
        t[i] = ((i as u32).wrapping_mul(2654435761) >> 13) as u8 ^ (i as u8);
        i += 1;
    }
    t
}
static TABLE: [u8; 4096] = table();

#[inline(never)]
fn bump(delta: u32) -> u32 {
    let old = COUNTER.load(Relaxed);
    COUNTER.store(old.wrapping_add(delta), Relaxed);
    ZERO.store(ZERO.load(Relaxed).wrapping_add(old & 0xff), Relaxed);
    old
}

#[inline(never)]
fn sum_slice(s: &[u8], seed: u32) -> u32 {
    let mut acc = seed;
    let mut i = 0usize;
    while i < s.len() {
        acc = acc.rotate_left(3).wrapping_add(s[i] as u32);
        i += 1;
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = input1 as usize;
    let j = input2 as usize;

    let bytes = (B13[i % 13] as u32) | ((B13[j % 13] as u32) << 8);
    let halves = (H7[i % 7] as u32) ^ ((H7[j % 7] as u32) << 16);
    let words = W5[i % 5].wrapping_add(W5[j % 5].rotate_left(5));
    let q = Q3[i % 3] ^ Q3[j % 3].rotate_left(17);
    let r = RECS[(i ^ j) % 3];
    let ra = r.a;
    let rb = r.b;
    let rc = r.c;
    let rec = ra ^ (rb as u32) ^ ((rb >> 32) as u32) ^ ((rc as u32) << 8) ^ (r.tag as u32);
    let text = TEXT.as_bytes();
    let t = text[i % text.len()] as u32 | ((text[j % text.len()] as u32) << 8);
    let big = TABLE[(i ^ (j << 3)) % 4096] as u32
        | ((TABLE[(i.wrapping_mul(31).wrapping_add(j)) % 4096] as u32) << 8);

    // Interior pointers into statics, hashed by a helper.
    let s1 = sum_slice(&B13[3..], input1);
    let s2 = sum_slice(&TABLE[(j % 4000)..(j % 4000) + 17], input2);
    let s3 = sum_slice(&text[5 + (i % 7)..], s1);

    // Mutable .data + .bss traffic across two helper calls, restored.
    let c0 = COUNTER.load(Relaxed);
    let z0 = ZERO.load(Relaxed);
    let b1 = bump(input1 | 1);
    let b2 = bump(input2.rotate_left(3));
    let after = COUNTER.load(Relaxed) ^ ZERO.load(Relaxed).rotate_left(11);
    COUNTER.store(c0, Relaxed);
    ZERO.store(z0, Relaxed);

    bytes
        .wrapping_mul(0x0101_0101)
        .wrapping_add(halves.rotate_left(7))
        .wrapping_add(words)
        .wrapping_add(q as u32 ^ (q >> 32) as u32)
        .wrapping_add(rec.rotate_left(3))
        .wrapping_add(t.rotate_left(19))
        .wrapping_add(big)
        .wrapping_add(s2 ^ s3)
        .wrapping_add(b1 ^ b2.rotate_left(9))
        .wrapping_add(after)
}
