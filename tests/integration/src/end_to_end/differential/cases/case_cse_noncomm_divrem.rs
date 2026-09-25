// W2 oracle for the division family: `arith.div` / `arith.sdiv` /
// `arith.mod` / `arith.smod` with swapped operands must never merge.
//
// Same volatile-byte hatch as `cse_comm_arith`. The divisor is a runtime value
// kept in the non-zero band `(byte & 7) + 1`, and the signed dividend is
// restricted to a byte-derived range so `MIN / -1` is unreachable.

use core::ptr::read_volatile;

#[inline(never)]
fn udiv_swap(p: *const u8) -> u32 {
    let a1 = (unsafe { read_volatile(p) } as u32 & 7) + 1;
    let b1 = (unsafe { read_volatile(p.add(1)) } as u32 & 7) + 1;
    let q1 = a1 / b1;
    let b2 = (unsafe { read_volatile(p.add(1)) } as u32 & 7) + 1;
    let a2 = (unsafe { read_volatile(p) } as u32 & 7) + 1;
    let q2 = b2 / a2;
    q1.rotate_left(1).wrapping_sub(q2.rotate_left(3))
}

#[inline(never)]
fn umod_swap(p: *const u8) -> u32 {
    let a1 = (unsafe { read_volatile(p) } as u32 & 7) + 1;
    let b1 = (unsafe { read_volatile(p.add(1)) } as u32 & 7) + 1;
    let r1 = a1 % b1;
    let b2 = (unsafe { read_volatile(p.add(1)) } as u32 & 7) + 1;
    let a2 = (unsafe { read_volatile(p) } as u32 & 7) + 1;
    let r2 = b2 % a2;
    r1.rotate_left(1).wrapping_sub(r2.rotate_left(3))
}

#[inline(never)]
fn sdiv_swap(p: *const u8) -> u32 {
    let a1 = ((unsafe { read_volatile(p) } as i32 & 7) + 1) * -1;
    let b1 = (unsafe { read_volatile(p.add(1)) } as i32 & 7) + 1;
    let q1 = a1 / b1;
    let b2 = (unsafe { read_volatile(p.add(1)) } as i32 & 7) + 1;
    let a2 = ((unsafe { read_volatile(p) } as i32 & 7) + 1) * -1;
    let q2 = b2 / a2;
    (q1 as u32).rotate_left(1).wrapping_sub((q2 as u32).rotate_left(3))
}

#[inline(never)]
fn smod_swap(p: *const u8) -> u32 {
    let a1 = ((unsafe { read_volatile(p) } as i32 & 7) + 1) * -1;
    let b1 = (unsafe { read_volatile(p.add(1)) } as i32 & 7) + 1;
    let r1 = a1 % b1;
    let b2 = (unsafe { read_volatile(p.add(1)) } as i32 & 7) + 1;
    let a2 = ((unsafe { read_volatile(p) } as i32 & 7) + 1) * -1;
    let r2 = b2 % a2;
    (r1 as u32).rotate_left(1).wrapping_sub((r2 as u32).rotate_left(3))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    udiv_swap(p)
        ^ umod_swap(p).rotate_left(1)
        ^ sdiv_swap(p).rotate_left(2)
        ^ smod_swap(p).rotate_left(3)
        ^ udiv_swap(unsafe { p.add(2) }).rotate_left(4)
        ^ smod_swap(unsafe { p.add(2) }).rotate_left(5)
}
