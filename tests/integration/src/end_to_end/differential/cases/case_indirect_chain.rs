// Chained indirect dispatch: the runtime-selected stage functions themselves
// dispatch through a second fn-pointer array, so a `dynexec` callee performs
// another `dynexec` (nested dispatch frames on the VM). The outer dispatch
// also sits inside a loop, and one arm of a conditional dispatches while the
// other computes directly — call_indirect in every control-flow position.

type Leaf = fn(u32) -> u32;

#[inline(never)]
fn leaf_gray(x: u32) -> u32 {
    x ^ (x >> 1)
}

#[inline(never)]
fn leaf_spread(x: u32) -> u32 {
    x.wrapping_mul(0x8100_0101).rotate_right(3)
}

static LEAVES: [Leaf; 2] = [leaf_gray, leaf_spread];

// Each stage dispatches through LEAVES with a runtime index: an indirect
// callee that itself calls indirectly.
#[inline(never)]
fn stage_mask(x: u32) -> u32 {
    LEAVES[(x & 1) as usize](x).wrapping_add(0x5a5a)
}

#[inline(never)]
fn stage_swap(x: u32) -> u32 {
    LEAVES[((x >> 2) & 1) as usize](x.swap_bytes())
}

type Stage = fn(u32) -> u32;
static STAGES: [Stage; 2] = [stage_mask, stage_swap];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1;
    // Indirect dispatch inside a loop, index depending on the loop-carried value
    let mut k = 0u32;
    while k < 3 {
        acc = STAGES[((acc ^ k) & 1) as usize](acc.wrapping_add(input2));
        k += 1;
    }
    // Indirect dispatch in one branch arm only
    if input2 & 4 == 0 {
        acc = LEAVES[(acc & 1) as usize](acc);
    } else {
        acc = acc.wrapping_mul(3).wrapping_sub(input1);
    }
    acc
}
