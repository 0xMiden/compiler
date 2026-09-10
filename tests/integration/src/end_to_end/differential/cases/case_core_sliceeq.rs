// Runtime-length slice comparison (campaign 27, Part A): `==`, `!=` and the
// `Ord` operators on `&[u8]` sub-slices whose length comes from the input.
// Every one of them lowers to a `memcmp` libcall no guest can link (there is
// no wasi-libc and compiler-builtins' `mem` symbols are absent), at every
// optimization level. The linkable replacements are in `core_eq_reach`:
// `iter().eq`, `zip(..).all(..)`, `iter().cmp(..)` and, for a
// CONSTANT-length array, plain `==`.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1.to_le_bytes();
    let b = input2.to_le_bytes();
    let n = 1 + (input2 % 4) as usize;
    let eq = (a[..n] == b[..n]) as u32;
    let ne = (a[..n] != b[..n]) as u32;
    let lt = (a[..n] < b[..n]) as u32;
    let ge = (a[..] >= b[..]) as u32;
    eq | (ne << 1) | (lt << 2) | (ge << 3)
}
