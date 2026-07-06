// A dynamically-impossible panic path that LLVM cannot prove away:
// `h % 6 == 5` implies `h % 3 == 2`, so the conjunction below is never true,
// but relating remainders by different moduli is beyond known-bits analysis.
// The surviving `unreachable` exercises `translate_unreachable_operator`,
// `ub::Unreachable` lowering, and the cfg-to-scf handling of branch regions
// that end in different return-like ops (ret vs unreachable).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(7);
    if h % 6 == 5 && h % 3 == 0 {
        panic!();
    }
    h ^ input2
}
