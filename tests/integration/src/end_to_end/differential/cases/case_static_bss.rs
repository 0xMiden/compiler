#[inline(never)]
fn op_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[inline(never)]
fn op_mix(a: u32, b: u32) -> u32 {
    (a ^ b).wrapping_mul(2654435761)
}

static OPS: [fn(u32, u32) -> u32; 2] = [op_add, op_mix];

// A large zero-initialized static lands in `.bss`, which occupies linear memory after the data
// segments without appearing in the wasm binary; only the declared minimum memory size accounts
// for it. A layout that used the data segments as its only floor would place compiler-managed
// regions (globals at ~data end + one page, the function table on the next page boundary after
// them) inside this array.
static mut BIG: [u8; 139264] = [0; 139264]; // 136 KiB of .bss starting at ~0x100010

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let big = unsafe { &mut *core::ptr::addr_of_mut!(BIG) };
    // Overwrite (via a wasm `memory.fill`) the slice of the array spanning the pages where a
    // `.bss`-blind layout would place the function table (~0x120000, i.e. ~0x1FFF0 into the
    // array): the indirect calls below would then dispatch through corrupted MAST roots and
    // fail. The window is ±4 KiB wide to tolerate data-segment drift, and kept small so the
    // fill stays cheap on the VM.
    big[0x1F000..0x21000].fill(input1 as u8);
    let f = OPS[(input2 & 1) as usize];
    let folded = f(input1, input2);
    let i = (input2 % 139264) as usize;
    let j = (input1 % 139264) as usize;
    // Volatile reads keep the array and the fill observable, preventing store-forwarding from
    // optimizing both away
    let a = unsafe { core::ptr::read_volatile(&big[i]) } as u32;
    let b = unsafe { core::ptr::read_volatile(&big[j]) } as u32;
    folded.wrapping_add(a).wrapping_add(b.rotate_left(7))
}
