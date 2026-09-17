// Trap parity: the assertion macros. `debug_assert!` / `debug_assert_eq!`
// are compiled out of a release guest and must NOT trap even though their
// predicates are false for every input; `assert!`, `assert_eq!` and
// `assert_ne!` must trap on both targets exactly when their predicate fails
// (`a == 13`, the two divisibility flags disagreeing, `a == 42`).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 % 100;
    let b = input2 % 100;
    debug_assert!(a == 0xdead, "debug assertions are off in a release guest");
    debug_assert_eq!(a, b);
    assert!(a != 13, "a is 13");
    assert_eq!(a % 7 == 0, b % 7 == 0);
    assert_ne!(a, 42);
    a.wrapping_mul(b).rotate_left(5) ^ (a ^ b)
}
