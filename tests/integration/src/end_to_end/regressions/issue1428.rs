//! Regression tests for https://github.com/0xMiden/compiler/issues/1428, and for
//! https://github.com/0xMiden/compiler/issues/1427 (`index_mut_on_loaded_word`).
//!
//! LLVM moves adjacent `Felt` cells of a `Word` through memory as a single `i64` and then
//! rotates or shifts that `i64` to reorder or split the cells. Each cell holds a full field
//! element, so values >= 2^32 must survive these moves unchanged.

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

/// An account-id prefix from the issue; it is >= 2^32.
const LARGE: u64 = 12393906174523661585;

/// Compiles `main_fn` as the entrypoint body and runs it with `args`, returning the result felt.
fn run(name: &'static str, main_fn: &str, args: &[Felt]) -> Felt {
    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(name, main_fn, config, []);
    let package = test.compile_package();
    eval_package::<Felt, _, _>(package, [], args, &test.session, |_| Ok(())).unwrap()
}

fn felt(value: u64) -> Felt {
    Felt::new_unchecked(value)
}

/// The issue's payee word `[prefix, suffix, tag, 0]`, with a prefix >= 2^32.
fn payee_args() -> [Felt; 4] {
    [felt(LARGE), felt(0xffff_ffff_0000_0000 - 7), felt(7), felt(0)]
}

/// The value both payee helpers compute from `vec![suffix, prefix]` and the tag.
fn expected_payee_result(args: &[Felt; 4]) -> Felt {
    args[1] * felt(3) + args[0] * felt(5) + args[2] + Felt::ONE
}

/// A `Word` passed to an `#[inline(never)]` helper that indexes it to build
/// `vec![payee[1], payee[0]]`.
#[test]
fn word_arg_indexed_in_noninlined_helper() {
    // LLVM copies the two swapped elements as `i64.load; i64.rotl 32; i64.store`.
    let main_fn = r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt) -> Felt {
        let payee = Word::new([p0, p1, p2, p3]);
        pay_word(payee, felt!(1))
    }

    #[inline(never)]
    fn pay_word(payee: Word, amount: Felt) -> Felt {
        consume(alloc::vec![payee[1], payee[0]], payee[2]) + amount
    }

    #[inline(never)]
    fn consume(storage: alloc::vec::Vec<Felt>, tag: Felt) -> Felt {
        storage[0] * felt!(3) + storage[1] * felt!(5) + tag
    }
    "#;

    let args = payee_args();
    let res = run("issue1428_word_arg", main_fn, &args);
    assert_eq!(res, expected_payee_result(&args));
}

/// Control for [word_arg_indexed_in_noninlined_helper]: the helper takes the payee elements as
/// separate `Felt`s.
#[test]
fn scalar_args_in_noninlined_helper() {
    let main_fn = r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt) -> Felt {
        let payee = Word::new([p0, p1, p2, p3]);
        pay_scalars(payee[0], payee[1], payee[2], felt!(1))
    }

    #[inline(never)]
    fn pay_scalars(prefix: Felt, suffix: Felt, tag: Felt, amount: Felt) -> Felt {
        consume(alloc::vec![suffix, prefix], tag) + amount
    }

    #[inline(never)]
    fn consume(storage: alloc::vec::Vec<Felt>, tag: Felt) -> Felt {
        storage[0] * felt!(3) + storage[1] * felt!(5) + tag
    }
    "#;

    let args = payee_args();
    let res = run("issue1428_scalar_args", main_fn, &args);
    assert_eq!(res, expected_payee_result(&args));
}

/// Assigning into an element of a `Word` loaded through a ret area (`IndexMut`), then passing the
/// word's elements on, mirroring `StorageValue::get`/`set` from the SDK.
#[test]
fn index_mut_on_loaded_word() {
    // LLVM reads `state[0..2]` as one `i64.load` and splits it with `i32.wrap_i64` and
    // `i64.shr_u 32`.
    let main_fn = r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt) -> Felt {
        let mut s = Storage::new();
        let now = p3.as_canonical_u64() & 0xffff_ffff;
        assert!(now > 0);
        let mut state: Word = s.get(p0, p1).unwrap_or_else(|_| panic!("bad get"));
        assert!(state[2].as_canonical_u64() == 0);
        state[2] = Felt::new(now).unwrap();
        let old = s.set(state).unwrap_or_else(|_| panic!("bad old"));
        old[0]
    }

    struct Storage { slot_suffix: Felt, slot_prefix: Felt }

    impl Storage {
        #[inline(never)]
        fn new() -> Self {
            Storage { slot_suffix: felt!(11), slot_prefix: felt!(12) }
        }

        #[inline(always)]
        fn get(&self, p0: Felt, p1: Felt) -> Result<Word, &'static str> {
            let mut ret = WordAligned::new(core::mem::MaybeUninit::<Word>::uninit());
            get_item(self.slot_suffix, self.slot_prefix, p0, p1, ret.as_mut_ptr());
            Ok(unsafe { ret.into_inner().assume_init() })
        }

        #[inline(always)]
        fn set(&mut self, value: Word) -> Result<Word, &'static str> {
            let value = Ok::<Word, &'static str>(value).unwrap_or_else(|_| panic!("bad set"));
            let mut ret = WordAligned::new(core::mem::MaybeUninit::<Word>::uninit());
            set_item(
                self.slot_suffix,
                self.slot_prefix,
                value[0],
                value[1],
                value[2],
                value[3],
                ret.as_mut_ptr(),
            );
            Ok(unsafe { ret.into_inner().assume_init() })
        }
    }

    /// Writes the stored word `[p0, p1, 0, 2]` to `out`.
    #[inline(never)]
    fn get_item(_s0: Felt, _s1: Felt, p0: Felt, p1: Felt, out: *mut Word) {
        unsafe { out.write(Word::new([p0, p1, felt!(0), felt!(2)])) }
    }

    /// Writes a digest of the new word to `out`, in place of the old value.
    #[inline(never)]
    fn set_item(_s0: Felt, _s1: Felt, a: Felt, b: Felt, c: Felt, d: Felt, out: *mut Word) {
        let r = a * felt!(3) + b * felt!(5) + c * felt!(7) + d;
        unsafe { out.write(Word::new([r, r, r, r])) }
    }
    "#;

    // `state` = `[LARGE, 1e12, 0, 2]` (the issue's large held retention is `state[1]`), `now` = 9.
    let args = [felt(LARGE), felt(1_000_000_000_000), felt(0), felt(9)];
    let res = run("issue1428_index_mut", main_fn, &args);
    let expected = args[0] * felt(3) + args[1] * felt(5) + felt(9) * felt(7) + felt(2);
    assert_eq!(res, expected);
}

/// A genuine `u64` shifted right and rotated by 32 keeps its limbs in the right order.
#[test]
fn u64_shr_and_rotate_by_32() {
    // The result packs `(x >> 32)` in the low 16 bits and the low limb of `x.rotate_left(32)` in
    // the high 16 bits, so a swapped or zeroed limb changes it.
    let main_fn = r#"(lo: u32, hi: u32) -> u32 {
        let x = make(lo, hi);
        let shifted = (x >> 32) as u32;
        let rotated = low(x.rotate_left(32));
        (rotated << 16) | (shifted & 0xffff)
    }

    #[inline(never)]
    fn make(lo: u32, hi: u32) -> u64 {
        ((hi as u64) << 32) | lo as u64
    }

    #[inline(never)]
    fn low(x: u64) -> u32 {
        x as u32
    }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys("issue1428_u64", main_fn, config, []);
    let package = test.compile_package();
    let args = [Felt::from(0x1111u32), Felt::from(0x2222u32)];
    let res = eval_package::<u32, _, _>(package, [], &args, &test.session, |_| Ok(())).unwrap();
    assert_eq!(res, 0x2222_2222);
}
