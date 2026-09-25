// Trap parity in the other direction: panics that are statically present but
// dynamically unreachable, so NEITHER target may trap on any input. Both
// predicates are cross-modulus contradictions (`h % 6 == 5` forces `h % 3`
// to be 2, so it can never be 0) — the shape KNOWLEDGE.md records as the way
// to keep a guard opaque to LLVM's known-bits reasoning. If the MASM side
// traps here, a live branch was folded the wrong way.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(7);
    let g = h % 6;
    if g == 5 && h % 3 == 0 {
        panic!("unreachable predicate");
    }
    // The divisor is zero on exactly the same impossible residue pair.
    let d = if g == 5 && h % 3 == 0 { 0 } else { g + 1 };
    let q = h / d;
    // The index is out of range on the same impossible pair.
    let t: [u32; 8] = [3, 1, 4, 1, 5, 9, 2, 6];
    let i = if g == 5 && h % 3 == 0 { 9 } else { (h % 8) as usize };
    q.wrapping_mul(t[i]) ^ h
}
