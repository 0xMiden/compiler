//! Resolving a linker stub's name to the procedure it calls.
//!
//! The stub resolves against the packages the session linked, and nothing else: the first package
//! exporting the path wins. The stub's own Wasm signature is then checked against what the rule
//! set derives from the export, so bindings generated against a different package version are
//! rejected instead of miscompiled.
//!
//! The conversions the rule set implies at the `exec` boundary also live here. The contract
//! between the frontend and the SDK is in Wasm *carrier* types — `i32` for every integer of 32
//! bits or narrower, `i64` for 64-bit, `felt` for a field element — while the import is declared
//! with the callee's own types, which keep the manifest's width, signedness and address space.
//! Codegen validates every `exec` argument against the resolved callee's declaration by exact
//! type (`codegen/masm/src/emit/primop.rs`), so [`convert_arguments`] converts each argument from
//! its carrier to the declared type before the call, and [`convert_results`] converts each result
//! back before it is returned or stored through the out pointer.

use alloc::{format, string::String, vec::Vec};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, SourceSpan, SymbolPath, Type, ValueRef, diagnostics::Report,
    dialects::builtin::attributes::Signature,
};
use midenc_package_interface::{
    ExportResolver, LoweredSignature, PackageInterface, ProcedureClass, ResolvedProcedure,
    WasmScalar,
};

use crate::{
    WasmTranslationConfig, error::WasmResult, module::function_builder_ext::FunctionBuilderExt,
};

/// Resolve `path` and check `stub` against the derived signature.
///
/// Returns the shape the Miden ABI rule set derives for the callee.
pub(crate) fn resolve_stub(
    path: &SymbolPath,
    stub: &Signature,
    config: &WasmTranslationConfig,
) -> Result<LoweredSignature, Report> {
    let masm_path = path.to_library_path();
    let linked: &[PackageInterface] = config.linked_packages.as_deref().unwrap_or(&[]);

    let Some(ResolvedProcedure { package, procedure }) = linked.resolve_procedure(&masm_path)
    else {
        let searched = if linked.is_empty() {
            String::from("no packages are linked")
        } else {
            format!("searched: {}", linked.iter().map(describe).collect::<Vec<_>>().join(", "))
        };
        return Err(Report::msg(format!(
            "linker stub '{masm_path}' does not name a procedure exported by any linked package \
             ({searched}); link the package that defines it with `-l` or a `miden-project.toml` \
             dependency"
        )));
    };

    let package_id = describe(package);
    let lowered = match &procedure.class {
        ProcedureClass::Bindable(lowered) => lowered,
        ProcedureClass::Skipped(reason) => {
            // `SkipReason`'s `Display` says what is wrong for all three variants; only `Untyped`
            // is about a missing signature, so no advice is appended to it here.
            return Err(Report::msg(format!(
                "linker stub '{masm_path}' names a procedure of {package_id} that cannot be \
                 bound: {reason}"
            )));
        }
        ProcedureClass::Role(role) => {
            // The attribute spelling, not the variant name: `note_script` is what the user wrote.
            return Err(Report::msg(format!(
                "linker stub '{masm_path}' names a {} procedure of {package_id}, which is reached \
                 through its component interface, not as an `exec` binding",
                role.attribute()
            )));
        }
    };

    let expected = lowered.stub_signature();
    let actual_params: Vec<Type> = stub.params().iter().map(|p| p.ty.clone()).collect();
    let actual_results: Vec<Type> = stub.results().iter().map(|p| p.ty.clone()).collect();
    if actual_params != expected.params() || actual_results != expected.results() {
        return Err(Report::msg(format!(
            "linker stub '{masm_path}' was generated against a different package version: the \
             stub is {} but {package_id} declares a procedure whose binding is {}; regenerate the \
             bindings against the linked package",
            render(&actual_params, &actual_results),
            render(expected.params(), expected.results()),
        )));
    }

    Ok(lowered.clone())
}

/// A package as the diagnostics name it: `<name> <version>`.
fn describe(package: &PackageInterface) -> String {
    format!("{} {}", AsRef::<str>::as_ref(&package.name), package.version)
}

fn render(params: &[Type], results: &[Type]) -> String {
    let list = |types: &[Type]| types.iter().map(|t| format!("{t}")).collect::<Vec<_>>().join(", ");
    format!("({}) -> ({})", list(params), list(results))
}

/// Convert the stub's arguments from their Wasm carrier types to the types the callee declares.
///
/// The stub receives every argument in the carrier type [`WasmScalar::frontend_type`] names, and
/// the import is declared with [`WasmScalar::miden_type`]; codegen requires the two to agree
/// exactly at the `exec`, so each argument is converted here:
///
/// - a same-width difference in signedness (`i32` → `u32`, `i64` → `u64`) is a `bitcast`;
/// - a narrower declared integer (`u16`, `u8`, `i16`, `i8`, `i1`) is a `trunc`. The value is in
///   range by construction — the SDK's Rust wrapper extends such a parameter to `i32` at the Wasm
///   boundary — and the deleted hand tables passed the raw `i32` with no check either, so an
///   unchecked truncation preserves the previous behaviour;
/// - a pointer is the carrier given the declared pointer type (`inttoptr`). The address itself
///   is never changed: the stub's `i32` is taken to be an address in the address space the
///   callee declares already. Converting a
///   Rust byte address to an element address — which is only valid for an element-aligned
///   address — is the binding wrapper's job, in Rust, where the check is visible; see
///   [`convert_results`] for the other direction.
///
/// The out pointer of [`midenc_package_interface::ReturnStrategy::OutPointer`], if any, is the
/// last argument and has no [`midenc_package_interface::WasmParam`], so it is never converted:
/// it stays the `i32` that [`super::transform::store_results_to_pointer`] expects.
pub(crate) fn convert_arguments<B: ?Sized + Builder>(
    lowered: &LoweredSignature,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let mut args = args.to_vec();
    for (index, param) in lowered.params.iter().enumerate() {
        // `resolve_stub` already checked the stub's arity against `expected.params()` (which has
        // the same length as `lowered.params`), so `index` is always in range here. The check is
        // kept anyway as a defensive guard against the two falling out of sync.
        let Some(arg) = args.get(index).copied() else {
            return Err(Report::msg(format!(
                "linker stub is missing argument {index}, the '{}' parameter of its callee",
                param.scalar.miden_type()
            )));
        };
        args[index] = convert_argument(&param.scalar, arg, builder, span)?;
    }
    Ok(args)
}

/// Convert one argument from its carrier type to `scalar`'s declared type. See
/// [`convert_arguments`].
fn convert_argument<B: ?Sized + Builder>(
    scalar: &WasmScalar,
    arg: ValueRef,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let declared = scalar.miden_type();
    if arg.borrow().ty() == &declared {
        return Ok(arg);
    }
    Ok(match scalar {
        WasmScalar::Ptr(_) => builder.inttoptr(arg, declared, span)?,
        WasmScalar::U32 | WasmScalar::U64 => builder.bitcast(arg, declared, span)?,
        WasmScalar::I1 | WasmScalar::I8 | WasmScalar::U8 | WasmScalar::I16 | WasmScalar::U16 => {
            builder.trunc(arg, declared, span)?
        }
        // The carrier is the declared type, so the early return above already handled these.
        WasmScalar::I32 | WasmScalar::I64 | WasmScalar::Felt => arg,
    })
}

/// Convert the callee's results back to the Wasm carrier types the stub deals in.
///
/// The inverse of [`convert_arguments`], and it must run *before* the results reach the stub's
/// `return` or [`super::transform::store_results_to_pointer`]: the latter stores each value in
/// the slot [`LoweredSignature::return_area`] lays out for its carrier and writes as many bytes
/// as the value's type has, so a `u16` result has to be widened to its `i32` carrier first if it
/// is to fill the 4-byte slot the SDK wrapper reads back.
///
/// A pointer result is cast to its `i32` carrier and nothing else: an element-space pointer
/// reaches the stub as an element address. Scaling it to a byte address, and checking that the
/// scaled address still fits in 32 bits, belongs to the binding wrapper, like the argument side.
///
/// Widening a narrow unsigned result takes two steps — `zext` requires an unsigned result type,
/// so the value is extended to `u32` and then reinterpreted as `i32` — the same shape
/// `component::canon_abi_utils::widen_loaded_value` uses for the canonical ABI.
pub(crate) fn convert_results<B: ?Sized + Builder>(
    lowered: &LoweredSignature,
    results: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let scalars = lowered.ret.scalars();
    if scalars.len() != results.len() {
        return Err(Report::msg(format!(
            "linker stub callee returned {} values, but the Miden ABI rule set derives {} for it",
            results.len(),
            scalars.len()
        )));
    }
    scalars
        .iter()
        .zip(results)
        .map(|(scalar, result)| convert_result(scalar, *result, builder, span))
        .collect()
}

/// Convert one result from `scalar`'s declared type back to its carrier type. See
/// [`convert_results`].
fn convert_result<B: ?Sized + Builder>(
    scalar: &WasmScalar,
    result: ValueRef,
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let carrier = scalar.frontend_type();
    if result.borrow().ty() == &carrier {
        return Ok(result);
    }
    Ok(match scalar {
        WasmScalar::Ptr(_) => builder.ptrtoint(result, carrier, span)?,
        WasmScalar::U32 | WasmScalar::U64 => builder.bitcast(result, carrier, span)?,
        WasmScalar::I1 | WasmScalar::U8 | WasmScalar::U16 => {
            let widened = builder.zext(result, Type::U32, span)?;
            builder.bitcast(widened, Type::I32, span)?
        }
        WasmScalar::I8 | WasmScalar::I16 => builder.sext(result, Type::I32, span)?,
        // The carrier is the declared type, so the early return above already handled these.
        WasmScalar::I32 | WasmScalar::I64 | WasmScalar::Felt => result,
    })
}

#[cfg(test)]
mod tests {
    use alloc::{rc::Rc, sync::Arc, vec, vec::Vec};
    use core::str::FromStr;

    use midenc_hir::{
        CallConv, Context, FunctionIdent, FunctionType, SymbolPath, Type,
        dialects::builtin::attributes::Signature,
    };
    use midenc_package_interface::{
        PackageInterface, ProcedureClass, ProcedureItem, SkipReason, lower_signature,
    };

    use super::*;
    use crate::WasmTranslationConfig;

    fn item(path: &str, sig: Option<FunctionType>) -> ProcedureItem {
        use midenc_session::miden_assembly_syntax::ast::{AttributeSet, Path};
        let class = match &sig {
            Some(s) => ProcedureClass::Bindable(lower_signature(s).unwrap()),
            None => ProcedureClass::Skipped(SkipReason::Untyped),
        };
        ProcedureItem {
            path: Arc::from(Path::new(path).to_path_buf().into_boxed_path()),
            digest: Default::default(),
            signature: sig,
            attributes: AttributeSet::default(),
            class,
        }
    }

    /// An export that fills a protocol role: typed, but reached through the component interface
    /// rather than as an `exec` binding.
    fn role_item(path: &str, role: midenc_package_interface::Role) -> ProcedureItem {
        use midenc_session::miden_assembly_syntax::ast::{Attribute, AttributeSet, Ident, Path};
        let marker = Attribute::Marker(Ident::new(role.attribute()).unwrap());
        ProcedureItem {
            path: Arc::from(Path::new(path).to_path_buf().into_boxed_path()),
            digest: Default::default(),
            signature: None,
            attributes: AttributeSet::from_iter([marker]),
            class: ProcedureClass::Role(role),
        }
    }

    fn package(name: &str, items: Vec<ProcedureItem>) -> PackageInterface {
        use miden_mast_package::{PackageId, TargetType, Version};
        PackageInterface {
            name: PackageId::from(name),
            version: Version::new(1, 2, 3),
            kind: TargetType::Library,
            digest: Default::default(),
            procedures: items,
            types: Vec::new(),
            constants: Vec::new(),
            modules: Vec::new(),
        }
    }

    fn config(packages: Vec<PackageInterface>) -> WasmTranslationConfig {
        WasmTranslationConfig {
            linked_packages: Some(packages.into()),
            ..Default::default()
        }
    }

    fn path(s: &str) -> SymbolPath {
        SymbolPath::from_masm_function_id(FunctionIdent::from_str(s).unwrap())
    }

    fn stub(context: &Rc<Context>, params: &[Type], results: &[Type]) -> Signature {
        Signature::new(context, params.iter().cloned(), results.iter().cloned())
    }

    #[test]
    fn a_typed_export_resolves_to_its_lowered_signature() {
        let context = Rc::new(Context::default());
        let sig = FunctionType::new(CallConv::Fast, [Type::Felt, Type::Felt], [Type::Felt]);
        let cfg = config(vec![package("lib", vec![item("::lib::add", Some(sig))])]);
        let lowered = resolve_stub(
            &path("lib::add"),
            &stub(&context, &[Type::Felt, Type::Felt], &[Type::Felt]),
            &cfg,
        )
        .unwrap();
        assert_eq!(lowered.stub_signature().params(), &[Type::Felt, Type::Felt]);
    }

    #[test]
    fn an_unknown_export_names_the_path_and_the_packages_searched() {
        let context = Rc::new(Context::default());
        let cfg = config(vec![package("lib", vec![])]);
        let err = resolve_stub(&path("lib::nope"), &stub(&context, &[], &[]), &cfg)
            .unwrap_err()
            .to_string();
        assert!(err.contains("lib::nope"), "{err}");
        assert!(err.contains("lib 1.2.3"), "{err}");
    }

    #[test]
    fn an_unknown_export_with_nothing_linked_says_so() {
        let context = Rc::new(Context::default());
        let cfg = config(vec![]);
        let err = resolve_stub(&path("lib::nope"), &stub(&context, &[], &[]), &cfg)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no packages are linked"), "{err}");
    }

    #[test]
    fn an_untyped_export_is_rejected_naming_the_package_and_version() {
        let context = Rc::new(Context::default());
        let cfg = config(vec![package("lib", vec![item("::lib::raw", None)])]);
        let err = resolve_stub(&path("lib::raw"), &stub(&context, &[], &[]), &cfg)
            .unwrap_err()
            .to_string();
        assert!(err.contains("lib 1.2.3"), "{err}");
        // `SkipReason`'s own `Display` is the whole explanation; no advice is appended, which
        // for `Untyped` would only repeat it and for the other two reasons would be false.
        assert!(err.ends_with("no typed signature in the package manifest"), "{err}");
    }

    #[test]
    fn a_role_procedure_is_rejected_by_its_attribute_name() {
        let context = Rc::new(Context::default());
        let cfg = config(vec![package(
            "lib",
            vec![role_item("::lib::consume", midenc_package_interface::Role::NoteScript)],
        )]);
        let err = resolve_stub(&path("lib::consume"), &stub(&context, &[], &[]), &cfg)
            .unwrap_err()
            .to_string();
        assert!(err.contains("names a note_script procedure of lib 1.2.3"), "{err}");
        assert!(err.contains("component interface"), "{err}");
    }

    #[test]
    fn a_stub_whose_signature_disagrees_with_the_package_is_rejected() {
        let context = Rc::new(Context::default());
        let sig = FunctionType::new(CallConv::Fast, [Type::U16], [Type::U32]);
        let cfg = config(vec![package("lib", vec![item("::lib::f", Some(sig))])]);
        // the SDK passed a felt where the manifest says u16
        let err = resolve_stub(&path("lib::f"), &stub(&context, &[Type::Felt], &[Type::I32]), &cfg)
            .unwrap_err()
            .to_string();
        assert!(err.contains("different package version"), "{err}");
        assert!(err.contains("expected (i32) -> (i32)") || err.contains("i32"), "{err}");
    }

    /// A stub with the wrong *number* of parameters is rejected by the same diagnostic, because
    /// the comparison in `resolve_stub` is over the whole parameter list. That is what makes the
    /// missing-argument guard in [`convert_arguments`] purely defensive: nothing that reaches it
    /// can have fewer arguments than the callee has parameters.
    #[test]
    fn a_stub_with_the_wrong_parameter_count_is_rejected_as_a_version_mismatch() {
        let context = Rc::new(Context::default());
        let sig = FunctionType::new(CallConv::Fast, [Type::Felt, Type::Felt], [Type::Felt]);
        let cfg = config(vec![package("lib", vec![item("::lib::add", Some(sig))])]);
        let stub = stub(&context, &[Type::Felt], &[Type::Felt]);
        let err = resolve_stub(&path("lib::add"), &stub, &cfg).unwrap_err().to_string();
        assert!(err.contains("different package version"), "{err}");
        assert!(err.contains("the stub is (felt) -> (felt)"), "{err}");
        assert!(err.contains("(felt, felt) -> (felt)"), "{err}");
    }

    /// A `miden::…` path gets no special treatment: with no packages linked, even a procedure
    /// the SDK binds does not resolve, and the diagnostic is the one any unknown export gets.
    #[test]
    fn without_linked_packages_nothing_resolves() {
        let context = Rc::new(Context::default());
        let cfg = WasmTranslationConfig::default();
        let err = resolve_stub(
            &path("miden::protocol::tx::get_block_timestamp"),
            &stub(&context, &[], &[Type::I32]),
            &cfg,
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(
                "linker stub '::miden::protocol::tx::get_block_timestamp' does not name a \
                 procedure exported by any linked package"
            ),
            "{err}"
        );
        assert!(err.contains("no packages are linked"), "{err}");
    }
}
