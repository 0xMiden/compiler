//! Regression tests for felt cells carried in an `i64`: issues
//! https://github.com/0xMiden/compiler/issues/1427 (`index_mut_on_loaded_word`) and
//! https://github.com/0xMiden/compiler/issues/1428. The shape of #1428, two swapped felts copied
//! as one `i64` rotated by 32, came from LLVM's store merging, which the Rust build flags turn
//! off, so it has no test here.
//!
//! LLVM moves two adjacent `Felt` cells (for example of a `Word` in memory, or a `[Felt; 2]`
//! passed by value) as a single `i64`, and shifts or ORs that `i64` to split or pack the cells.
//! Each cell holds a full field element, so values >= 2^32 must survive these moves unchanged.

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

/// Returns the felt with the canonical value `value`.
fn felt(value: u64) -> Felt {
    Felt::new_unchecked(value)
}

/// Assigning into an element of a `Word` (`IndexMut`) that is read from and written to word-aligned
/// out-pointers keeps the other elements.
#[test]
fn index_mut_on_loaded_word() {
    // LLVM reads `word[0..2]` as one `i64.load` and splits it with `i32.wrap_i64` and
    // `i64.shr_u 32`.
    let main_fn = r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt, p4: Felt) -> Felt {
        let mut word = read(p0, p1, p2, p3);
        word[2] = p4;
        write(word)[0]
    }

    /// Returns `[p0, p1, p2, p3]`, read through a word-aligned out-pointer.
    #[inline(always)]
    fn read(p0: Felt, p1: Felt, p2: Felt, p3: Felt) -> Word {
        let mut out = WordAligned::new(core::mem::MaybeUninit::<Word>::uninit());
        read_into(p0, p1, p2, p3, out.as_mut_ptr());
        unsafe { out.into_inner().assume_init() }
    }

    #[inline(never)]
    fn read_into(p0: Felt, p1: Felt, p2: Felt, p3: Felt, out: *mut Word) {
        unsafe { out.write(Word::new([p0, p1, p2, p3])) }
    }

    /// Returns a word whose every element is a weighted sum of the elements of `word`, written
    /// through a word-aligned out-pointer.
    #[inline(always)]
    fn write(word: Word) -> Word {
        let mut out = WordAligned::new(core::mem::MaybeUninit::<Word>::uninit());
        write_into(word[0], word[1], word[2], word[3], out.as_mut_ptr());
        unsafe { out.into_inner().assume_init() }
    }

    #[inline(never)]
    fn write_into(a: Felt, b: Felt, c: Felt, d: Felt, out: *mut Word) {
        let sum = a * felt!(3) + b * felt!(5) + c * felt!(7) + d * felt!(11);
        unsafe { out.write(Word::new([sum, sum, sum, sum])) }
    }
    "#;

    let args = packed_args();
    let res = run("issue1428_index_mut", main_fn, &args);
    let expected = args[0] * felt(3) + args[1] * felt(5) + args[4] * felt(7) + args[3] * felt(11);
    assert_eq!(res, expected);
}

/// Distinct felts >= 2^32.
fn packed_args() -> [Felt; 5] {
    [
        felt(3_000_000_000_000),
        felt(1_000_000_000_000),
        felt(LARGE),
        felt(0xffff_ffff_0000_0000 - 7),
        felt(5_000_000_000_000),
    ]
}

/// Fixture helper: a word loaded through a ret area.
const LOAD_HELPER: &str = r#"
    #[inline(never)]
    fn load(p0: Felt, p1: Felt, p2: Felt, p3: Felt) -> Word {
        Word::new([p0, p1, p2, p3])
    }
"#;

/// Fixture helper: an identity function taking a `[Felt; 2]` by value.
const PASS_PAIR_HELPER: &str = r#"
    #[inline(never)]
    fn pass_pair(pair: [Felt; 2]) -> [Felt; 2] {
        pair
    }
"#;

/// Fixture helper: an identity function taking a `Result<u32, Felt>` by value.
const PASS_ERR_HELPER: &str = r#"
    #[inline(never)]
    fn pass_err(result: Result<u32, Felt>) -> Result<u32, Felt> {
        result
    }
"#;

/// Fixture helper: an identity function taking a `Result<Felt, u32>` by value.
const PASS_RESULT_HELPER: &str = r#"
    #[inline(never)]
    fn pass_result(result: Result<Felt, u32>) -> Result<Felt, u32> {
        result
    }
"#;

/// A `[Felt; 2]` of two non-adjacent elements of a loaded word, passed by value to a
/// non-inlined function, keeps both felts.
#[test]
fn felt_pair_of_separate_elements_passed_by_value() {
    // LLVM packs the pair into one `i64` from the two felts in locals:
    // `zext(reinterpret(y)) << 32 | zext(reinterpret(x))`.
    let main_fn = format!(
        r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt, _p4: Felt) -> Felt {{
        let w = load(p0, p1, p2, p3);
        let (x, y) = (w[3], w[0]);
        let pair = pass_pair([x, y]);
        pair[0] * felt!(3) + pair[1] * felt!(5) + x * felt!(7) + y * felt!(11)
    }}
    {LOAD_HELPER}{PASS_PAIR_HELPER}"#
    );

    let args = packed_args();
    let res = run("issue1428_pair_separate", &main_fn, &args);
    let expected = args[3] * felt(3) + args[0] * felt(5) + args[3] * felt(7) + args[0] * felt(11);
    assert_eq!(res, expected);
}

/// A `[Felt; 2]` of a felt argument and an element of a loaded word, passed by value to a
/// non-inlined function, keeps both felts.
#[test]
fn felt_pair_with_loaded_element_passed_by_value() {
    // LLVM loads the high half with `i64.load32_u`, shifts it by 32 and ORs in the low half.
    let main_fn = format!(
        r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt, p4: Felt) -> Felt {{
        let w = load(p0, p1, p2, p3);
        let pair = pass_pair([p4, w[3]]);
        pair[0] * felt!(3) + pair[1] * felt!(5)
    }}
    {LOAD_HELPER}{PASS_PAIR_HELPER}"#
    );

    let args = packed_args();
    let res = run("issue1428_pair_loaded", &main_fn, &args);
    assert_eq!(res, args[4] * felt(3) + args[3] * felt(5));
}

/// A `Result<Felt, u32>::Ok` holding an element of a loaded word, passed by value to a
/// non-inlined function, keeps the felt.
#[test]
fn felt_result_passed_by_value() {
    // LLVM builds the `i64` as the felt shifted by 32, with the zero `Ok` tag as the low half.
    let main_fn = format!(
        r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt, p4: Felt) -> Felt {{
        let w = load(p0, p1, p2, p3);
        pass_result(Ok(w[3])).unwrap_or(p4)
    }}
    {LOAD_HELPER}{PASS_RESULT_HELPER}"#
    );

    let args = packed_args();
    let res = run("issue1428_result", &main_fn, &args);
    assert_eq!(res, args[3]);
}

/// A `Result<u32, Felt>::Err` holding an element of a loaded word, passed by value to a
/// non-inlined function, keeps the felt.
#[test]
fn felt_err_passed_by_value() {
    let main_fn = format!(
        r#"(p0: Felt, p1: Felt, p2: Felt, p3: Felt, p4: Felt) -> Felt {{
        let w = load(p0, p1, p2, p3);
        match pass_err(Err(w[3])) {{
            Ok(_) => p4,
            Err(felt) => felt,
        }}
    }}
    {LOAD_HELPER}{PASS_ERR_HELPER}"#
    );

    let args = packed_args();
    let res = run("issue1428_err", &main_fn, &args);
    assert_eq!(res, args[3]);
}
