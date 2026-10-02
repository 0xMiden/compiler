// A frame close to the 1 MiB wasm shadow-stack limit: a `[MaybeUninit<u32>;
// 250000]` local (1,000,000 bytes, no fill) whose address escapes to an
// `#[inline(never)]` helper with a 4 KiB frame of its own, so the stack
// pointer descends to ~0x0B000 (the shadow stack grows down from 0x100000
// towards address 0 with `--stack-first`). A stride-99991 stripe across the
// whole array is written and read back, plus the lowest and highest
// elements, so the frame must really span the megabyte and the helper's
// frame below it must not alias.
use core::mem::MaybeUninit;

#[inline(never)]
fn stripe(big: &mut [MaybeUninit<u32>; 250000], k: u32) -> u32 {
    let mut own = [0u32; 1024];
    let mut acc = k;
    let mut i = 0usize;
    while i < 48 {
        let idx = (i * 99991 + (k % 99991) as usize) % 250000;
        let v = acc.wrapping_mul(0x9e37_79b9) ^ (idx as u32);
        big[idx].write(v);
        own[(idx * 7) & 1023] = v.rotate_left(3);
        acc = acc.rotate_left(7) ^ own[(i * 13) & 1023];
        i += 1;
    }
    big[0].write(acc ^ 0x1111_1111);
    big[249999].write(acc ^ 0x2222_2222);
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut big: [MaybeUninit<u32>; 250000] = [MaybeUninit::uninit(); 250000];
    let x = input1 ^ input2.rotate_left(11);
    let k = input1.wrapping_add(input2);
    let s = stripe(&mut big, k);
    let mut acc = s ^ x;
    let mut i = 0usize;
    while i < 48 {
        let idx = (i * 99991 + (k % 99991) as usize) % 250000;
        // Written by `stripe` above at exactly this index.
        let v = unsafe { big[idx].assume_init() };
        acc = acc.rotate_left(1) ^ v;
        i += 1;
    }
    let lo = unsafe { big[0].assume_init() };
    let hi = unsafe { big[249999].assume_init() };
    acc.wrapping_add(lo).wrapping_add(hi.rotate_left(9))
}
