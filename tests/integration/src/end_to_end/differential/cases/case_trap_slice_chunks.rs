// Trap parity: the slice iterators and rotations whose *argument* — not an
// index — is what panics. `chunks_exact(k)` and `windows(k)` panic on
// `k == 0` (`chunk size must be non-zero` / `window size must be non-zero`),
// which `input1 % 4` reaches, and `rotate_left(r)` panics for `r > len`,
// which `input2 % 11` reaches over a `[u32; 8]` (`r == 8` is still legal).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut data: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    let k = (input1 % 4) as usize;
    let mut acc = 0u32;
    for c in data.chunks_exact(k) {
        acc = acc.wrapping_add(c[0]);
    }
    for w in data.windows(k) {
        acc = acc.rotate_left(1) ^ w[0];
    }
    let r = (input2 % 11) as usize;
    data.rotate_left(r);
    acc ^ data[0] ^ (data[7] << 8)
}
