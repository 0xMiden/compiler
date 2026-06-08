// Mutable statics via atomics (safe Rust, no `static mut`). Non-zero
// atomic initializers land in the wasm `.data` section, giving a second
// data segment next to `.rodata` — exercising multi-segment layout:
// DataSegmentLayout::insert, merge_data_segments, validate_no_overlaps,
// and word-alignment padding. Atomic load/store lowers to plain
// i32/i64/i32.load8 memory ops at absolute (constant) addresses.
//
// The initial values are restored before returning: the native cdylib is
// loaded once for all proptest runs, so statics must not carry state.
use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};

static A: AtomicU32 = AtomicU32::new(0x1234_5678);
static B: AtomicU64 = AtomicU64::new(0x9abc_def0_1122_3344);
static C: AtomicU8 = AtomicU8::new(0x5a);

static RO: [u32; 8] = [11, 222, 3333, 44444, 555_555, 6_666_666, 77_777_777, 888_888_888];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a0 = A.load(Relaxed);
    let b0 = B.load(Relaxed);
    let c0 = C.load(Relaxed);

    A.store(a0.wrapping_add(input1), Relaxed);
    B.store(b0 ^ ((input2 as u64) << 13), Relaxed);
    C.store(c0 ^ (input1 as u8), Relaxed);

    let a1 = A.load(Relaxed);
    let b1 = B.load(Relaxed);
    let c1 = C.load(Relaxed);

    // Restore the original values to keep the native side stateless.
    A.store(a0, Relaxed);
    B.store(b0, Relaxed);
    C.store(c0, Relaxed);

    a1 ^ (b1 as u32) ^ ((b1 >> 32) as u32) ^ ((c1 as u32) << 24) ^ RO[(input1 % 8) as usize]
}
