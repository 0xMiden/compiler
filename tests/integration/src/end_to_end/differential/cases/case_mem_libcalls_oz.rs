// Constant-length aggregate copies and zero-fills ABOVE LLVM's size-mode
// inline-store threshold: at -Oz (`--optimize=size-min`) 48-byte struct
// copies and a 64-byte zero-init lower to `memory.copy` / `memory.fill`
// with CONSTANT length operands (O2 expands them into i64 load/store
// sequences), so the wasm `memory.copy` -> MemCpy lowering sees immediate
// lengths and the miden-core-lib element/byte copy split runs on
// stack-slot and `.rodata` source addresses. Copies of 12-24 bytes stay
// inline even at -Oz (probe-verified), hence the wide records.
#[derive(Clone, Copy)]
struct Rec {
    a: u32,
    b: u64,
    c: [u8; 12],
    d: [u64; 3],
}

static TABLE: [Rec; 4] = [
    Rec {
        a: 0x1111_1111,
        b: 0x0102_0304_0506_0708,
        c: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        d: [0x1111, 0x2222, 0x3333],
    },
    Rec {
        a: 0x2222_2222,
        b: 0x1112_1314_1516_1718,
        c: [13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24],
        d: [0x4444, 0x5555, 0x6666],
    },
    Rec {
        a: 0x3333_3333,
        b: 0x2122_2324_2526_2728,
        c: [25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36],
        d: [0x7777, 0x8888, 0x9999],
    },
    Rec {
        a: 0x4444_4444,
        b: 0x3132_3334_3536_3738,
        c: [37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48],
        d: [0xaaaa, 0xbbbb, 0xcccc],
    },
];

static BLOB: [u8; 64] = [
    0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff,
    0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe, 0x0f,
    0x20, 0x31, 0x42, 0x53, 0x64, 0x75, 0x86, 0x97, 0xa8, 0xb9, 0xca, 0xdb, 0xec, 0xfd, 0x0e, 0x1f,
    0x30, 0x41, 0x52, 0x63, 0x74, 0x85, 0x96, 0xa7, 0xb8, 0xc9, 0xda, 0xeb, 0xfc, 0x0d, 0x1e, 0x2f,
];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let idx = (input1 & 3) as usize;
    // .rodata -> stack copy (48 bytes) at a runtime index.
    let mut r = TABLE[idx];
    r.c[(input2 & 7) as usize] = input1 as u8;
    r.a ^= input2;
    r.d[(input1 >> 2) as usize % 3] ^= input2 as u64;
    // Stack -> stack copies at runtime slots.
    let mut slots = [Rec {
        a: 0,
        b: 0,
        c: [0; 12],
        d: [0; 3],
    }; 4];
    slots[(input2 & 3) as usize] = r;
    slots[(input1.rotate_right(2) & 3) as usize] = TABLE[(input2 & 3) as usize];
    // 64-byte zero-fill of which the head is overwritten by a 48-byte copy
    // from a runtime offset into a `.rodata` blob and the tail is read back,
    // so neither the fill nor the copy is elided.
    let mut bytes = [0u8; 64];
    let off = (input1 & 15) as usize;
    bytes[..48].copy_from_slice(&BLOB[off..off + 48]);
    let mut h = r.a ^ (r.b as u32) ^ ((r.b >> 32) as u32) ^ (r.d[0] as u32);
    let mut i = 0;
    while i < 64 {
        h = h.rotate_left(3) ^ (bytes[i] as u32).wrapping_mul(i as u32 + 1);
        i += 1;
    }
    let s = &slots[(input2 >> 5) as usize & 3];
    h ^ s.a
        ^ (s.b as u32)
        ^ (s.c[(input1 >> 8) as usize % 12] as u32)
        ^ (s.d[(input2 >> 9) as usize % 3] as u32)
}
