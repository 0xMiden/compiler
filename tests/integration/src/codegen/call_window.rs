//! Executed tests of the call window around a cross-context `call` and `dyncall`.
//!
//! A `call` hands the callee the caller's top 16 operand stack elements and replaces them with
//! whatever the callee leaves there. Procedures written in Miden Assembly treat everything under
//! their arguments as padding, `[args, pad(16 - n)]` on entry and `[results, pad(16 - m)]` on
//! exit, which they are free to drop or overwrite. Code generation therefore zero-fills the
//! window under the arguments before the call, so no caller value sits inside it, and discards
//! the callee's padding under the results after it.
//!
//! Callees compiled from HIR preserve the window by construction, so they cannot tell a padded
//! call from an unpadded one. The callees here are hand-written Miden Assembly that overwrite
//! every padding element with a sentinel, while the caller keeps a value live across the call
//! that an unpadded window would expose to them.
//!
//! The widths (arguments to results: 2 to 2, 1 to 4, 9 to 0, 0 to 16, 0 to 9, 12 to 12, 4 to 8,
//! 8 to 8) reach every arm of the padding and of the discard but the empty ones:
//!
//! - the felt-wise padding of few (1, 2) and of many (9) arguments, and the whole-word padding of
//!   0, 4, 8 and 12 arguments;
//! - the felt-wise discard under few (2) and many (9) results, and the whole-word discard under
//!   0, 4, 8 and 12 results;
//! - 16 results, which fill the window and leave nothing to discard.
//!
//! Every callee traps unless the window under its arguments is zero. The entrypoint weights each
//! result by its position, so results out of order are visible as well.
//!
//! A `dyncall` shares the window handling and is executed here as well, reaching the same callees
//! through their MAST roots. A `syscall` shares it too, but needs a kernel, and is covered by the
//! emitter's unit tests only.

use std::{path::PathBuf, rc::Rc, sync::Arc};

use miden_mast_package::Package;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    CallConv, Context, Felt, Ident, Op, SourceSpan, Type, ValueRef, Visibility,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, FunctionRef, WorldBuilder, attributes::Signature,
    },
};

use super::support::run_args;
use crate::testing::compile_test_module_with;

/// The library name of the callee package, which `-l` derives from its file stem.
const CALLEE_LIBRARY: &str = "window_callee";

/// The Miden path of the callee module, and the name of the component declaring it in HIR.
const CALLEE_MODULE: &str = "window_callee::callee";

/// The callee module: each procedure traps unless the window under its arguments is zero, as the
/// caller's padding leaves it, computes its results, then overwrites every element of the window
/// under them with `SENTINEL`.
const CALLEE_SOURCE: &str = r#"
const SENTINEL = 57005

#! Inputs:  [a, b, 0(14)]
#! Outputs: [a + b, a * b, SENTINEL(14)]
#!
#! Traps unless every element of the window under the arguments is zero.
pub proc sum_product
    repeat.14
        movup.2 assertz
    end
    dup.1 dup.1 mul movdn.2 add
    movdn.15 movdn.15
    repeat.14
        push.SENTINEL swap drop movdn.13
    end
    movup.15 movup.15
end

#! Inputs:  [a, 0(15)]
#! Outputs: [a, a + 1, a + 2, a + 3, SENTINEL(12)]
#!
#! Traps unless every element of the window under the argument is zero.
pub proc spread
    repeat.15
        swap assertz
    end
    dup add.3 dup.1 add.2 dup.2 add.1 movup.3
    movdnw.3 dropw dropw dropw
    movdnw.3
    repeat.12
        push.SENTINEL swap drop movdn.11
    end
    movupw.3
end

#! Inputs:  [a0, ..., a8, 0(7)]
#! Outputs: [SENTINEL(16)]
#!
#! Traps unless every element of the window under the arguments is zero.
pub proc nine_to_none
    repeat.7
        movup.9 assertz
    end
    repeat.16
        push.SENTINEL swap drop movdn.15
    end
end

#! Inputs:  [0(16)]
#! Outputs: [1, 2, ..., 16]
#!
#! Traps unless every element of the window is zero.
pub proc none_to_sixteen
    repeat.16
        assertz
    end
    push.1 swap drop movdn.15
    push.2 swap drop movdn.15
    push.3 swap drop movdn.15
    push.4 swap drop movdn.15
    push.5 swap drop movdn.15
    push.6 swap drop movdn.15
    push.7 swap drop movdn.15
    push.8 swap drop movdn.15
    push.9 swap drop movdn.15
    push.10 swap drop movdn.15
    push.11 swap drop movdn.15
    push.12 swap drop movdn.15
    push.13 swap drop movdn.15
    push.14 swap drop movdn.15
    push.15 swap drop movdn.15
    push.16 swap drop movdn.15
end

#! Inputs:  [0(16)]
#! Outputs: [1, 2, ..., 9, SENTINEL(7)]
#!
#! Traps unless every element of the window is zero.
pub proc none_to_nine
    repeat.16
        assertz
    end
    push.1 swap drop movdn.15
    push.2 swap drop movdn.15
    push.3 swap drop movdn.15
    push.4 swap drop movdn.15
    push.5 swap drop movdn.15
    push.6 swap drop movdn.15
    push.7 swap drop movdn.15
    push.8 swap drop movdn.15
    push.9 swap drop movdn.15
    repeat.7
        push.SENTINEL swap drop movdn.15
    end
end

#! Inputs:  [a0, ..., a11, 0(4)]
#! Outputs: [a0 + 1, a1 + 2, ..., a11 + 12, SENTINEL(4)]
#!
#! Traps unless every element of the window under the arguments is zero.
pub proc twelve_to_twelve
    repeat.4
        movup.12 assertz
    end
    add.1 movdn.11
    add.2 movdn.11
    add.3 movdn.11
    add.4 movdn.11
    add.5 movdn.11
    add.6 movdn.11
    add.7 movdn.11
    add.8 movdn.11
    add.9 movdn.11
    add.10 movdn.11
    add.11 movdn.11
    add.12 movdn.11
    movupw.3
    repeat.4
        push.SENTINEL swap drop movdn.3
    end
    movdnw.3
end

#! Inputs:  [a0, a1, a2, a3, 0(12)]
#! Outputs: [a0 + 1, a1 + 2, a2 + 3, a3 + 4, 5, 6, 7, 8, SENTINEL(8)]
#!
#! Traps unless every element of the window under the arguments is zero.
pub proc four_to_eight
    repeat.12
        movup.4 assertz
    end
    add.1 movdn.3
    add.2 movdn.3
    add.3 movdn.3
    add.4 movdn.3
    movdnw.3
    push.5 swap drop movdn.15
    push.6 swap drop movdn.15
    push.7 swap drop movdn.15
    push.8 swap drop movdn.15
    repeat.8
        push.SENTINEL swap drop movdn.15
    end
end

#! Inputs:  [a0, ..., a7, 0(8)]
#! Outputs: [a0 + 1, a1 + 2, ..., a7 + 8, SENTINEL(8)]
#!
#! Traps unless every element of the window under the arguments is zero.
pub proc eight_to_eight
    repeat.8
        movup.8 assertz
    end
    add.1 movdn.7
    add.2 movdn.7
    add.3 movdn.7
    add.4 movdn.7
    add.5 movdn.7
    add.6 movdn.7
    add.7 movdn.7
    add.8 movdn.7
    swapdw
    repeat.8
        push.SENTINEL swap drop movdn.15
    end
end
"#;

/// The callee package written to a directory of its own, removed when dropped.
struct CalleeLibrary {
    /// The directory holding the package.
    dir: PathBuf,
    /// The assembled callee package.
    package: Arc<Package>,
}

impl CalleeLibrary {
    /// Assembles [CALLEE_SOURCE] and writes it as `<CALLEE_LIBRARY>.masp` to a fresh directory
    /// whose name is unique to this process and `test`.
    fn write(test: &str) -> Self {
        use midenc_package_interface::testing::{assemble_fixture, write_package};

        let dir =
            std::env::temp_dir().join(format!("midenc-call-window-{}-{test}", std::process::id()));
        let package = assemble_fixture(CALLEE_LIBRARY, CALLEE_MODULE, CALLEE_SOURCE);
        let library = Self { dir, package };
        write_package(&library.path(), &library.package);
        library
    }

    /// The path of the callee package.
    fn path(&self) -> PathBuf {
        self.dir.join(format!("{CALLEE_LIBRARY}.masp"))
    }

    /// The MAST root of the callee procedure `proc`, element 0 first.
    fn root(&self, proc: &str) -> [Felt; 4] {
        let path = format!("{CALLEE_MODULE}::{proc}");
        let root = self
            .package
            .get_procedure_root_by_path(miden_assembly::ast::Path::new(&path))
            .unwrap_or_else(|| panic!("the callee package must export `{path}`"));
        root.into()
    }
}

impl Drop for CalleeLibrary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Declares `proc` of the callee module in `world`, taking `params` and returning `results`
/// felts, under the component-model convention of a cross-context callee.
fn declare_callee(
    world: &mut WorldBuilder,
    context: &Rc<Context>,
    proc: &str,
    params: usize,
    results: usize,
) -> (FunctionRef, Signature) {
    let signature = Signature::with_convention(
        context,
        CallConv::ComponentModel,
        vec![Type::Felt; params],
        vec![Type::Felt; results],
    );
    let component = world
        .define_component(Ident::from(CALLEE_MODULE))
        .expect("callee component must be declared");
    let function = ComponentBuilder::new(component)
        .define_function(Ident::from(proc), Visibility::Internal, signature.clone())
        .expect("callee function must be declared");
    (function, signature)
}

/// How the entrypoint invokes the callee.
#[derive(Clone, Copy, Debug)]
enum Invocation {
    /// A `call` to the callee declared at its path.
    Call,
    /// A `dyncall` to the callee's MAST root.
    Dyncall,
}

/// Compiles an entrypoint that invokes `proc` with `invocation` on `params` felt arguments and
/// returns the [`weighted`] sum of `x` and the `results` felts `proc` returns, and runs it on each
/// of `inputs`, given as `[x, args..]`.
///
/// For a [`Invocation::Dyncall`] the entrypoint materializes the root of `proc` as constants, so
/// that the entrypoint's inputs fit the operand stack beside the widest argument list.
///
/// Returns the VM outputs, after asserting that the MASM invokes `proc` as requested.
fn run_calling(
    invocation: Invocation,
    proc: &str,
    params: usize,
    results: usize,
    inputs: &[Vec<Felt>],
) -> Vec<Felt> {
    let library = CalleeLibrary::write(&format!("{invocation:?}-{proc}"));
    let link = library.path().to_str().expect("temporary path must be UTF-8").to_string();
    let root = library.root(proc);
    let (masm, package, context) = compile_test_module_with(
        &["-l", &link],
        |world, context| declare_callee(world, context, proc, params, results),
        vec![Type::Felt; 1 + params],
        [Type::Felt],
        |builder, (callee, signature)| {
            let span = SourceSpan::default();
            let entry: Vec<ValueRef> = builder
                .current_block()
                .borrow()
                .arguments()
                .iter()
                .map(|arg| *arg as ValueRef)
                .collect();
            let args = &entry[1..];
            let outputs: Vec<ValueRef> = match invocation {
                Invocation::Call => {
                    let call = builder.call(*callee, signature.clone(), args.to_vec(), span);
                    call.unwrap()
                        .borrow()
                        .results()
                        .iter()
                        .map(|r| r.borrow().as_value_ref())
                        .collect()
                }
                Invocation::Dyncall => {
                    let root = root.map(|element| builder.felt(element, span));
                    let call = builder.dyncall(root, signature.clone(), args.to_vec(), span);
                    call.unwrap()
                        .borrow()
                        .results()
                        .iter()
                        .map(|r| r.borrow().as_value_ref())
                        .collect()
                }
            };
            let mut sum = None;
            for (i, output) in outputs.into_iter().enumerate() {
                let weight = builder.felt(felt(i as u64 + 1), span);
                let term = builder.mul_unchecked(output, weight, span).unwrap();
                sum = Some(match sum {
                    None => term,
                    Some(sum) => builder.add(sum, term, span).unwrap(),
                });
            }
            // `x` is added once the results are reduced: under sixteen of them it sits past the
            // operand stack window the operand scheduler reaches.
            let sum = match sum {
                None => entry[0],
                Some(sum) => builder.add(sum, entry[0], span).unwrap(),
            };
            builder.ret(Some(sum), span).unwrap();
        },
    );

    let target = format!("call.::{CALLEE_MODULE}::{proc}");
    match invocation {
        Invocation::Call => assert!(masm.contains(&target), "expected `{target}` in:\n{masm}"),
        Invocation::Dyncall => assert!(
            masm.split_whitespace().any(|token| token == "dyncall") && !masm.contains(&target),
            "expected a `dyncall` instruction and no `{target}` in:\n{masm}"
        ),
    }

    inputs.iter().map(|input| run_args::<Felt>(&package, &context, input)).collect()
}

/// A felt from a `u64`.
fn felt(value: u64) -> Felt {
    Felt::new_unchecked(value)
}

/// `x + Σ (i + 1) · results[i]`, what the entrypoint [`run_calling`] compiles returns: weighted
/// by position, so that results returned out of order are visible.
fn weighted(x: Felt, results: &[Felt]) -> Felt {
    results
        .iter()
        .enumerate()
        .fold(x, |sum, (i, &result)| sum + felt(i as u64 + 1) * result)
}

/// Runs `proc`, taking `params` and returning `results` felts, through `invocation` on each
/// `[x, args..]` of `cases`, and checks each output against the [`weighted`] sum of `x` and
/// `expected(args)`.
fn check(
    invocation: Invocation,
    proc: &str,
    params: usize,
    results: usize,
    cases: &[&[u64]],
    expected: fn(&[Felt]) -> Vec<Felt>,
) {
    let inputs: Vec<Vec<Felt>> =
        cases.iter().map(|case| case.iter().copied().map(felt).collect()).collect();
    let outputs = run_calling(invocation, proc, params, results, &inputs);
    for (input, output) in inputs.iter().zip(outputs) {
        let returned = expected(&input[1..]);
        assert_eq!(returned.len(), results, "`{proc}` returns {results} felts");
        assert_eq!(output, weighted(input[0], &returned), "{invocation:?} `{proc}` on {input:?}");
    }
}

/// Runs the `sum_product` cases, `[x, a, b]`, through `invocation` and checks each result: two
/// arguments, padded by pushing the zeros and raising the arguments over them, and two results,
/// sunk under the padding before it is dropped.
///
/// A caller value left in the window traps the callee, and a sentinel or the padding left among
/// the results changes the returned sum.
fn check_sum_product(invocation: Invocation) {
    check(
        invocation,
        "sum_product",
        2,
        2,
        &[&[1, 2, 3], &[100, 0, 0], &[7, 1_000_000, 3], &[u32::MAX as u64, 12345, 678]],
        |args| vec![args[0] + args[1], args[0] * args[1]],
    );
}

/// Runs the `spread` cases, `[x, a]`, through `invocation` and checks each result: one argument,
/// padded as `sum_product`'s two are, and a word of results, under which three whole words of
/// padding are dropped.
///
/// A caller value left in the window traps the callee, and a sentinel left among the results
/// changes the returned sum.
fn check_spread(invocation: Invocation) {
    check(
        invocation,
        "spread",
        1,
        4,
        &[&[1, 2], &[100, 0], &[7, 1_000_000], &[u32::MAX as u64, 12345]],
        |args| (0..4).map(|i| args[0] + felt(i)).collect(),
    );
}

/// Runs `nine_to_none` through `invocation`: nine arguments, padded by sinking each of the seven
/// zeros under them, and no results, discarded as four whole words of padding.
///
/// A caller value left in the window traps the callee, as does an argument left in the padding,
/// and a padding element left on top changes the `x` the entrypoint returns.
fn check_nine_to_none(invocation: Invocation) {
    check(
        invocation,
        "nine_to_none",
        9,
        0,
        &[&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[u32::MAX as u64, 9, 8, 7, 6, 5, 4, 3, 2, 1]],
        |_| vec![],
    );
}

/// Runs `none_to_sixteen` through `invocation`: no arguments, padded as four whole words, and
/// sixteen results, which fill the window and leave nothing to discard.
///
/// A caller value left in the window traps the callee, and a result discarded or a padding
/// element left beside the results changes the returned sum.
fn check_none_to_sixteen(invocation: Invocation) {
    check(invocation, "none_to_sixteen", 0, 16, &[&[1], &[u32::MAX as u64]], |_| {
        (1..=16).map(felt).collect()
    });
}

/// Runs `none_to_nine` through `invocation`: no arguments, padded as four whole words, and nine
/// results, under which each of the seven padding elements is raised and dropped.
///
/// A sentinel left among the results, a result dropped instead, or results out of order change
/// the returned sum.
fn check_none_to_nine(invocation: Invocation) {
    check(invocation, "none_to_nine", 0, 9, &[&[1], &[u32::MAX as u64]], |_| {
        (1..=9).map(felt).collect()
    });
}

/// Runs `twelve_to_twelve` through `invocation`: twelve arguments, padded with one word moved
/// under them, and twelve results, whose padding word is raised and dropped.
///
/// A zero left among the arguments or an argument left in the padding traps the callee, and
/// arguments or results out of order, or the sentinel word left among the results, change the
/// returned sum.
fn check_twelve_to_twelve(invocation: Invocation) {
    check(
        invocation,
        "twelve_to_twelve",
        12,
        12,
        &[
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
            &[u32::MAX as u64, 120, 110, 100, 90, 80, 70, 60, 50, 40, 30, 20, 10],
        ],
        |args| args.iter().enumerate().map(|(i, &a)| a + felt(i as u64 + 1)).collect(),
    );
}

/// Runs `four_to_eight` through `invocation`: four arguments, padded with three words under which
/// the arguments' word is moved, and eight results, under whose padding the results' two words
/// are swapped before it is dropped.
///
/// A caller value left in the window traps the callee, and results out of order, or a sentinel
/// left among the results, change the returned sum.
fn check_four_to_eight(invocation: Invocation) {
    check(
        invocation,
        "four_to_eight",
        4,
        8,
        &[&[1, 2, 3, 4, 5], &[u32::MAX as u64, 40, 30, 20, 10]],
        |args| {
            let mut results: Vec<Felt> =
                args.iter().enumerate().map(|(i, &a)| a + felt(i as u64 + 1)).collect();
            results.extend((5..=8).map(felt));
            results
        },
    );
}

/// Runs `eight_to_eight` through `invocation`: eight arguments, padded with two words swapped
/// under them, and eight results, discarded as [`check_four_to_eight`]'s are.
///
/// A caller value left in the window traps the callee, and arguments or results out of order, or
/// a sentinel left among the results, change the returned sum.
fn check_eight_to_eight(invocation: Invocation) {
    check(
        invocation,
        "eight_to_eight",
        8,
        8,
        &[&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[u32::MAX as u64, 80, 70, 60, 50, 40, 30, 20, 10]],
        |args| args.iter().enumerate().map(|(i, &a)| a + felt(i as u64 + 1)).collect(),
    );
}

/// A value live across a call with two arguments and two results survives a callee that
/// overwrites the rest of the window.
#[test]
fn call_window_two_felts_in_two_out() {
    check_sum_product(Invocation::Call);
}

/// A value live across a call with one argument and a word of results survives a callee that
/// overwrites the rest of the window.
#[test]
fn call_window_one_felt_in_word_out() {
    check_spread(Invocation::Call);
}

/// A value live across a `dyncall` with two arguments and two results survives a callee that
/// overwrites the rest of the window.
#[test]
fn dyncall_window_two_felts_in_two_out() {
    check_sum_product(Invocation::Dyncall);
}

/// A value live across a `dyncall` with one argument and a word of results survives a callee
/// that overwrites the rest of the window.
#[test]
fn dyncall_window_one_felt_in_word_out() {
    check_spread(Invocation::Dyncall);
}

/// A `call` with nine arguments and no results: the felt-wise padding of more than eight
/// arguments, and the whole-word discard of an empty result list.
#[test]
fn call_window_nine_felts_in_none_out() {
    check_nine_to_none(Invocation::Call);
}

/// A `dyncall` with nine arguments and no results; see [`call_window_nine_felts_in_none_out`].
#[test]
fn dyncall_window_nine_felts_in_none_out() {
    check_nine_to_none(Invocation::Dyncall);
}

/// A `call` with no arguments and sixteen results: the whole-word padding of an empty argument
/// list, and a full result window with nothing to discard.
#[test]
fn call_window_none_in_sixteen_felts_out() {
    check_none_to_sixteen(Invocation::Call);
}

/// A `dyncall` with no arguments and sixteen results; see
/// [`call_window_none_in_sixteen_felts_out`].
#[test]
fn dyncall_window_none_in_sixteen_felts_out() {
    check_none_to_sixteen(Invocation::Dyncall);
}

/// A `call` with no arguments and nine results: the felt-wise discard of the padding under more
/// than eight results.
#[test]
fn call_window_none_in_nine_felts_out() {
    check_none_to_nine(Invocation::Call);
}

/// A `dyncall` with no arguments and nine results; see [`call_window_none_in_nine_felts_out`].
#[test]
fn dyncall_window_none_in_nine_felts_out() {
    check_none_to_nine(Invocation::Dyncall);
}

/// A `call` with twelve arguments and twelve results: the whole-word padding of twelve arguments
/// and the whole-word discard under twelve results.
#[test]
fn call_window_twelve_felts_in_twelve_out() {
    check_twelve_to_twelve(Invocation::Call);
}

/// A `dyncall` with twelve arguments and twelve results, the widest argument list beside the
/// callee's root; see [`call_window_twelve_felts_in_twelve_out`].
#[test]
fn dyncall_window_twelve_felts_in_twelve_out() {
    check_twelve_to_twelve(Invocation::Dyncall);
}

/// A `call` with four arguments and eight results: the whole-word padding of four arguments and
/// the whole-word discard under eight results.
#[test]
fn call_window_four_felts_in_eight_out() {
    check_four_to_eight(Invocation::Call);
}

/// A `dyncall` with four arguments and eight results; see [`call_window_four_felts_in_eight_out`].
#[test]
fn dyncall_window_four_felts_in_eight_out() {
    check_four_to_eight(Invocation::Dyncall);
}

/// A `call` with eight arguments and eight results: the whole-word padding of eight arguments.
#[test]
fn call_window_eight_felts_in_eight_out() {
    check_eight_to_eight(Invocation::Call);
}

/// A `dyncall` with eight arguments and eight results; see
/// [`call_window_eight_felts_in_eight_out`].
#[test]
fn dyncall_window_eight_felts_in_eight_out() {
    check_eight_to_eight(Invocation::Dyncall);
}
