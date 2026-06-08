// Reads from `static` lookup tables. Non-trivial initializers become wasm
// data segments, exercising the rodata pipeline: DataSegmentLayout::insert
// in the frontend, merge/validate/copy/pad in codegen data_segments.rs, and
// emit_data_segment_initialization. The odd-length byte table forces
// word-alignment padding; runtime indices keep the loads from folding.
static SBOX: [u32; 16] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
];

static BYTES: [u8; 13] = *b"miden-fuzzing";

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = SBOX[(input1 % 16) as usize];
    let b = SBOX[(input2 % 16) as usize];
    let c = BYTES[(input1 % 13) as usize] as u32;
    let d = BYTES[(input2 % 13) as usize] as u32;
    a.wrapping_add(b).rotate_left(c & 31) ^ d.wrapping_mul(0x0101_0101)
}
