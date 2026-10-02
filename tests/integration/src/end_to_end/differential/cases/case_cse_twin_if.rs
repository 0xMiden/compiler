// W0 reach probe: two structurally identical `if`s in one function, differing
// only in the values they capture. The question is whether CSE ever sees a
// region-bearing op -- the structural region comparison that upstream
// fab7b7db0 added to `hir/src/ir/operation/equivalence.rs`.
//
// The arms call an `#[inline(never)]` helper so LLVM keeps both branches and
// the wasm keeps both `if` blocks; the compiler lifts them to `scf.if` only in
// `lift-control-flow`, which runs six passes AFTER `cse`.

#[inline(never)]
fn mix(x: u32, y: u32) -> u32 {
    x.wrapping_mul(0x9e37_79b9) ^ y.rotate_left(7) ^ x.wrapping_add(y)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 ^ 0x1234_5678;
    let b = input2 ^ 0x9abc_def0;
    let mut acc = 0u32;
    if input1 & 1 != 0 {
        acc = acc.wrapping_add(mix(a, b));
        acc = acc.rotate_left(3);
    } else {
        acc = acc.wrapping_add(mix(b, a));
        acc = acc.rotate_left(3);
    }
    if input2 & 1 != 0 {
        acc = acc.wrapping_add(mix(a, b));
        acc = acc.rotate_left(3);
    } else {
        acc = acc.wrapping_add(mix(b, a));
        acc = acc.rotate_left(3);
    }
    if input1 & 2 != 0 {
        acc = acc.wrapping_add(mix(a, acc));
        acc = acc.rotate_left(5);
    } else {
        acc = acc.wrapping_add(mix(acc, a));
        acc = acc.rotate_left(5);
    }
    acc
}
