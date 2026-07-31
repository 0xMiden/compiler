// SCALE DIMENSION: data-segment surface. 24 statics of odd sizes (u8 arrays
// of 1/2/3/5/7/9/11/13/15 bytes interleaved with u16/u32/u64 tables), three
// AtomicU32 mutables (written and RESTORED before return), and one ~4KB
// const-fn-generated u32 table — data layout, merge, and padding at a count
// the corpus never had, with runtime reads from every segment.
use core::sync::atomic::{AtomicU32, Ordering};

static S1: [u8; 1] = [17];
static S2: [u8; 2] = [1, 2];
static S3: [u8; 3] = [3, 5, 7];
static S5: [u8; 5] = [11, 13, 17, 19, 23];
static S7: [u8; 7] = [29, 31, 37, 41, 43, 47, 53];
static S9: [u8; 9] = [59, 61, 67, 71, 73, 79, 83, 89, 97];
static S11: [u8; 11] = [2, 4, 8, 16, 32, 64, 128, 3, 9, 27, 81];
static S13: [u8; 13] = [1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233];
static S15: [u8; 15] = [251, 241, 239, 233, 229, 227, 223, 211, 199, 197, 193, 191, 181, 179, 173];
static T1: [u16; 3] = [0x1234, 0x5678, 0x9abc];
static T2: [u16; 5] = [0xdef0, 0x0fed, 0xcba9, 0x8765, 0x4321];
static T3: [u16; 7] = [7, 77, 777, 7777, 0x7000, 0x0700, 0x0070];
static T4: [u16; 9] = [9, 99, 999, 9999, 0x9000, 0x0900, 0x0090, 0x0009, 0x9090];
static T5: [u16; 11] = [1, 3, 7, 15, 31, 63, 127, 255, 511, 1023, 2047];
static U1: [u32; 3] = [0x0102_0304, 0x0506_0708, 0x090a_0b0c];
static U2: [u32; 5] = [0xdead_beef, 0xcafe_babe, 0xfeed_face, 0x8bad_f00d, 0x1bad_b002];
static U3: [u32; 7] = [1, 10, 100, 1000, 10000, 100000, 1000000];
static U4: [u32; 9] = [3, 9, 27, 81, 243, 729, 2187, 6561, 19683];
static V1: [u64; 3] = [0x0102_0304_0506_0708, 0x1112_1314_1516_1718, 0x2122_2324_2526_2728];
static V2: [u64; 5] = [
    0x9e37_79b9_7f4a_7c15,
    0x2545_f491_4f6c_dd1d,
    0x1405_7b7e_f767_814f,
    0x5851_f42d_4c95_7f2d,
    0x1465_0269_1234_5677,
];
static A1: AtomicU32 = AtomicU32::new(0x1111_2222);
static A2: AtomicU32 = AtomicU32::new(0x3333_4444);
static A3: AtomicU32 = AtomicU32::new(0x5555_6666);

/// ~4KB of non-zero rodata generated at compile time.
const fn big_table() -> [u32; 1024] {
    let mut t = [0u32; 1024];
    let mut i = 0;
    while i < 1024 {
        t[i] = (i as u32).wrapping_mul(2654435761) ^ 0x9e37_79b9;
        i += 1;
    }
    t
}
static BIG: [u32; 1024] = big_table();

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = input1 as usize;
    let j = input2 as usize;

    // Runtime-indexed reads from every u8/u16/u32/u64 segment.
    let bytes = (S1[0] as u32)
        .wrapping_add(S2[i % 2] as u32)
        .wrapping_add(S3[j % 3] as u32)
        .wrapping_add(S5[i % 5] as u32)
        .wrapping_add(S7[j % 7] as u32)
        .wrapping_add(S9[i % 9] as u32)
        .wrapping_add(S11[j % 11] as u32)
        .wrapping_add(S13[i % 13] as u32)
        .wrapping_add(S15[j % 15] as u32);
    let halves = (T1[i % 3] as u32)
        .wrapping_add(T2[j % 5] as u32)
        .wrapping_add(T3[i % 7] as u32)
        .wrapping_add(T4[j % 9] as u32)
        .wrapping_add(T5[i % 11] as u32);
    let words = U1[j % 3]
        .wrapping_add(U2[i % 5])
        .wrapping_add(U3[j % 7])
        .wrapping_add(U4[i % 9]);
    let wide = V1[j % 3].wrapping_add(V2[i % 5]);
    let big = BIG[i % 1024].wrapping_add(BIG[j % 1024]).wrapping_add(BIG[(i ^ j) % 1024]);

    // Mutable .data segment traffic, restored before returning (the native
    // cdylib is reused across all proptest inputs).
    let a1 = A1.load(Ordering::Relaxed);
    let a2 = A2.load(Ordering::Relaxed);
    let a3 = A3.load(Ordering::Relaxed);
    A1.store(a1 ^ input1, Ordering::Relaxed);
    A2.store(a2.wrapping_add(input2), Ordering::Relaxed);
    A3.store(a3.rotate_left(input1 & 31), Ordering::Relaxed);
    let mixed = A1.load(Ordering::Relaxed)
        .wrapping_mul(A2.load(Ordering::Relaxed) | 1)
        .wrapping_add(A3.load(Ordering::Relaxed));
    A1.store(a1, Ordering::Relaxed);
    A2.store(a2, Ordering::Relaxed);
    A3.store(a3, Ordering::Relaxed);

    bytes
        .wrapping_mul(0x0101_0101)
        .wrapping_add(halves.rotate_left(7))
        .wrapping_add(words.rotate_right(11))
        .wrapping_add((wide >> 32) as u32 ^ (wide as u32))
        .wrapping_add(big)
        .wrapping_add(mixed)
}
