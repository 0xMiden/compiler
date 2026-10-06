// Pointers stored in data segments and in the frame: static tables of
// `&[u8]` slices, `&str`s and `&[u16]`s (fat pointers = relocated absolute
// addresses + lengths in .rodata) selected at a runtime index, interior
// slices of statics cut at runtime, and a frame-resident array of slice
// references over runtime sub-ranges of a stack buffer (pointer values
// stored to and loaded from the frame as i32). Every load through a
// table-selected pointer must land on the bytes the native side sees.
static B13: [u8; 13] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8, 9];
static H9: [u16; 9] = [9, 99, 999, 9999, 0x9000, 0x0900, 0x0090, 0x0009, 0x9090];

static TAIL: [u8; 5] = [7, 7, 7, 1, 2];
static PARTS: [&[u8]; 6] = [&B13, b"campaign", &TAIL, b"thirteen-memory", b"x", &B13];
static NAMES: [&str; 4] = ["lane", "packed", "straddle", "copy"];
static H4: [u16; 4] = [0x1111, 0x2222, 0x3333, 0x4444];
static HALVES: [&[u16]; 3] = [&H9, &H4, &H9];

#[inline(never)]
fn hash_slice(s: &[u8], seed: u32) -> u32 {
    let mut acc = seed;
    let mut i = 0usize;
    while i < s.len() {
        acc = acc.rotate_left(5).wrapping_add(s[i] as u32).wrapping_mul(0x0101_0101 | 1);
        i += 1;
    }
    acc ^ (s.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = (input1 % 6) as usize;
    let j = (input2 % 4) as usize;
    let k = (input1 % 3) as usize;

    let p = PARTS[i];
    let n = NAMES[j].as_bytes();
    let h = HALVES[k];
    // Interior slices of statics, selected and cut at runtime.
    let cut = (input2 % 4) as usize;
    let inner: [&[u8]; 3] = [&B13[cut..], &B13[3..3 + cut + 2], &p[cut % p.len()..]];
    let q = inner[(input1 >> 5) as usize % 3];
    let mut acc = hash_slice(p, input2) ^ hash_slice(n, input1).rotate_left(7);
    acc = acc.wrapping_add(hash_slice(q, acc).rotate_left(11));
    acc = acc.wrapping_add(h[(input2 as usize) % h.len()] as u32);
    acc = acc.wrapping_add(p[(input1 as usize >> 3) % p.len()] as u32);

    // Frame-resident slice references over runtime sub-ranges of a buffer.
    let mut buf = [0u8; 40];
    let mut t = 0usize;
    while t < 40 {
        buf[t] = (input1.wrapping_mul(t as u32 + 1) ^ input2) as u8;
        t += 1;
    }
    let a = (input1 % 10) as usize;
    let b = (input2 % 10) as usize;
    let views: [&[u8]; 4] = [&buf[a..a + 10], &buf[b + 10..b + 20], &buf[20 + a..30], &buf[30..30 + b]];
    let v = views[(input1 >> 4) as usize & 3];
    let w = views[(input2 >> 4) as usize & 3];
    acc = acc.wrapping_add(hash_slice(v, acc)).wrapping_add(hash_slice(w, acc.rotate_left(3)));
    if !v.is_empty() {
        acc ^= (v[0] as u32) << 24;
    }
    acc
}
