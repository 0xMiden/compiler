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

/// The callee module: each procedure computes its results, then overwrites every element of the
/// window under them with `SENTINEL`.
const CALLEE_SOURCE: &str = r#"
const SENTINEL = 57005

#! Inputs:  [a, b, pad(14)]
#! Outputs: [a + b, a * b, SENTINEL(14)]
pub proc sum_product
    dup.1 dup.1 mul movdn.2 add
    movdn.15 movdn.15
    repeat.14
        push.SENTINEL swap drop movdn.13
    end
    movup.15 movup.15
end

#! Inputs:  [a, pad(15)]
#! Outputs: [a, a + 1, a + 2, a + 3, SENTINEL(12)]
pub proc spread
    repeat.15
        swap drop
    end
    dup add.3 dup.1 add.2 dup.2 add.1 movup.3
    movdnw.3 dropw dropw dropw
    movdnw.3
    repeat.12
        push.SENTINEL swap drop movdn.11
    end
    movupw.3
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
        use miden_assembly::serde::Serializable;

        let dir =
            std::env::temp_dir().join(format!("midenc-call-window-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("callee directory must be created");
        let package = midenc_package_interface::testing::assemble_fixture(
            CALLEE_LIBRARY,
            CALLEE_MODULE,
            CALLEE_SOURCE,
        );
        let library = Self { dir, package };
        std::fs::write(library.path(), library.package.to_bytes()).expect("callee must be written");
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
    /// A `dyncall` to the callee's MAST root, which the entrypoint takes as arguments.
    Dyncall,
}

/// Compiles an entrypoint that invokes `proc` with `invocation` on `params` felt arguments and
/// returns `x` plus the sum of the `results` felts `proc` returns, and runs it on each of
/// `inputs`, given as `[x, args..]`.
///
/// For a [`Invocation::Dyncall`] the entrypoint takes `[x, root(4), args..]`, and the root of
/// `proc` is inserted into each input after `x`.
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
    let root_felts = match invocation {
        Invocation::Call => 0,
        Invocation::Dyncall => midenc_dialect_hir::Dyncall::ROOT_FELTS,
    };
    let (masm, package, context) = compile_test_module_with(
        &["-l", &link],
        |world, context| declare_callee(world, context, proc, params, results),
        vec![Type::Felt; 1 + root_felts + params],
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
            let (root, args) = entry[1..].split_at(root_felts);
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
                    let root = root.try_into().expect("a root of four felts");
                    let call = builder.dyncall(root, signature.clone(), args.to_vec(), span);
                    call.unwrap()
                        .borrow()
                        .results()
                        .iter()
                        .map(|r| r.borrow().as_value_ref())
                        .collect()
                }
            };
            let sum = outputs
                .into_iter()
                .fold(entry[0], |sum, output| builder.add(sum, output, span).unwrap());
            builder.ret(Some(sum), span).unwrap();
        },
    );

    let target = format!("call.::{CALLEE_MODULE}::{proc}");
    match invocation {
        Invocation::Call => assert!(masm.contains(&target), "expected `{target}` in:\n{masm}"),
        Invocation::Dyncall => assert!(
            masm.contains("dyncall") && !masm.contains(&target),
            "expected a `dyncall` and no `{target}` in:\n{masm}"
        ),
    }

    let root = match invocation {
        Invocation::Call => vec![],
        Invocation::Dyncall => library.root(proc).to_vec(),
    };
    inputs
        .iter()
        .map(|input| {
            let args: Vec<Felt> =
                input[..1].iter().chain(&root).chain(&input[1..]).copied().collect();
            run_args::<Felt>(&package, &context, &args)
        })
        .collect()
}

/// A felt from a `u64`.
fn felt(value: u64) -> Felt {
    Felt::new_unchecked(value)
}

/// The inputs `[x, a, b]` of the `sum_product` cases.
const SUM_PRODUCT_CASES: [(u64, u64, u64); 4] =
    [(1, 2, 3), (100, 0, 0), (7, 1_000_000, 3), (u32::MAX as u64, 12345, 678)];

/// Runs the `sum_product` cases through `invocation` and checks each result.
fn check_sum_product(invocation: Invocation) {
    let inputs: Vec<Vec<Felt>> = SUM_PRODUCT_CASES
        .iter()
        .map(|&(x, a, b)| vec![felt(x), felt(a), felt(b)])
        .collect();
    let outputs = run_calling(invocation, "sum_product", 2, 2, &inputs);
    for (&(x, a, b), output) in SUM_PRODUCT_CASES.iter().zip(outputs) {
        let (x, a, b) = (felt(x), felt(a), felt(b));
        assert_eq!(output, x + (a + b) + a * b, "x = {x}, a = {a}, b = {b}");
    }
}

/// The inputs `[x, a]` of the `spread` cases.
const SPREAD_CASES: [(u64, u64); 4] = [(1, 2), (100, 0), (7, 1_000_000), (u32::MAX as u64, 12345)];

/// Runs the `spread` cases through `invocation` and checks each result.
fn check_spread(invocation: Invocation) {
    let inputs: Vec<Vec<Felt>> =
        SPREAD_CASES.iter().map(|&(x, a)| vec![felt(x), felt(a)]).collect();
    let outputs = run_calling(invocation, "spread", 1, 4, &inputs);
    for (&(x, a), output) in SPREAD_CASES.iter().zip(outputs) {
        let (x, a) = (felt(x), felt(a));
        assert_eq!(output, x + a * felt(4) + felt(6), "x = {x}, a = {a}");
    }
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
