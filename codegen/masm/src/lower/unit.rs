//! Immutable facts derived from all HIR emitted into one compilation unit.
use midenc_hir::{
    AddressSpace, CallOpInterface, CanonicalCallableRef, EnumRef, Op, OpOperand, Operation,
    StructRef, Symbol, Type, Value, Visibility, dialects::builtin, traits::TransparentCast,
};

use crate::{emit::addressing::ByteAddressing, hir, plan::CallingConvention};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UnitFacts {
    pub byte_addressing: ByteAddressing,
}

impl UnitFacts {
    /// Include supporting modules in the same closed source environment as the component.
    pub fn derive(root: &Operation, supporting: &[builtin::ModuleRef]) -> Self {
        let mut wide = false;
        let mut visit = |op: &Operation| wide |= can_hold_a_wide_byte_address(op);
        root.prewalk_all(&mut visit);
        for module in supporting {
            module.borrow().as_operation().prewalk_all(&mut visit);
        }
        Self {
            byte_addressing: if wide {
                ByteAddressing::Wide
            } else {
                ByteAddressing::Narrow
            },
        }
    }
}

/// Makers of wide byte addresses, rather than the mere presence of a pointer type.
/// Derived values keep this unit-wide policy through calls, control flow, and memory.
fn can_hold_a_wide_byte_address(op: &Operation) -> bool {
    if let Some(function) = op.downcast_ref::<builtin::Function>() {
        let signature = function.signature();
        return function.visibility() == Visibility::Public
            && !function.is_declaration()
            && CallingConvention::from_source(&signature.cc) == CallingConvention::SameContext
            && signature
                .params
                .iter()
                .chain(&signature.results)
                .any(|abi| holds_a_byte_address(&abi.ty, false));
    }
    if let Some(asm) = op.downcast_ref::<hir::InlineAsm>() {
        // An untyped write can publish a wide address through memory even when the boundary
        // has no pointers. Empty representation changes can reveal pointers through their types.
        return !asm.get_body().trim().is_empty()
            || passes_or_returns_an_address(op.operands().all().iter(), op);
    }
    if op.is::<hir::Load>() || op.is::<hir::LoadLocal>() {
        // Source memory can be written by opaque same-context callees without any typed
        // pointer boundary. Observe its pointer domain at the typed load, including locals.
        return op
            .results()
            .iter()
            .any(|result| holds_a_byte_address(result.borrow().ty(), false));
    }
    if op.is::<hir::Alloca>() || op.is::<hir::LocalByteAddress>() {
        return true;
    }
    if let Some(gep) = op.downcast_ref::<hir::GetElementPtr>() {
        // A narrow base plus a nonzero byte offset can cross 2^32. Malformed paths are
        // diagnosed by selection; they cannot justify selecting the narrower policy.
        return super::units::arithmetic::gep_offset(gep)
            .map_or(true, |offset| is_byte_pointer(&gep.addr().ty()) && offset != 0);
    }
    if op.implements::<dyn TransparentCast>() {
        if op.num_operands() != 1 || op.num_results() != 1 {
            return true;
        }
        let from = op.operands()[0].borrow();
        let to = op.results()[0].borrow();
        return match to.ty() {
            Type::Ptr(pointer) => {
                // The pointer's own address and addresses exposed through its pointee are
                // independent: even a narrow base can reveal memory containing wide pointers.
                pointer.is_byte_pointer() && from.ty().is_felt()
                    || holds_a_byte_address(pointer.pointee(), true)
                        && !from
                            .ty()
                            .pointee()
                            .is_some_and(|pointee| holds_a_byte_address(pointee, true))
            }
            _ => holds_a_byte_address(to.ty(), false) && !holds_a_byte_address(&from.ty(), false),
        };
    }
    let Some(call) = op.as_trait::<dyn CallOpInterface>() else {
        return false;
    };
    if let Some(signature) = call.callee_signature()
        && call.arguments().iter().zip(&signature.params).any(|(argument, parameter)| {
            matches!((argument.borrow().ty(), &parameter.ty),
                (Type::Ptr(argument), Type::Ptr(parameter))
                    if argument.addrspace() == AddressSpace::Element && parameter.is_byte_pointer())
        })
    {
        return true;
    }
    if op.is::<hir::Exec>() || op.is::<hir::ExecIndirect>() || op.is::<hir::DynExec>() {
        return call
            .possible_callees()
            .is_none_or(|callees| callees.iter().any(|callee| !is_defined_function(callee)))
            && passes_or_returns_an_address(call.arguments().iter(), op);
    }
    op.results()
        .iter()
        .any(|result| holds_a_byte_address(result.borrow().ty(), false))
}

fn passes_or_returns_an_address<'a>(
    mut operands: impl Iterator<Item = &'a OpOperand>,
    op: &Operation,
) -> bool {
    operands.any(|operand| holds_a_byte_address(&operand.borrow().ty(), false))
        || op
            .results()
            .iter()
            .any(|result| holds_a_byte_address(result.borrow().ty(), false))
}

/// Follow element pointers into memory, stopping conservatively at recursive aggregates.
/// Byte pointers and lists already carry byte addresses; code pointers do not.
fn holds_a_byte_address(ty: &Type, through_pointer: bool) -> bool {
    match ty {
        Type::Ptr(pointer) if pointer.is_byte_pointer() => true,
        Type::Ptr(pointer) => holds_a_byte_address(pointer.pointee(), true),
        Type::List(_) => true,
        Type::Struct(StructRef::Rec(_)) | Type::Enum(EnumRef::Rec(_)) if through_pointer => true,
        Type::Struct(ty) => ty
            .get()
            .fields()
            .iter()
            .any(|field| holds_a_byte_address(&field.ty, through_pointer)),
        Type::Enum(ty) => ty.get().variants().iter().any(|variant| {
            variant
                .value
                .as_ref()
                .is_some_and(|ty| holds_a_byte_address(ty, through_pointer))
        }),
        Type::Array(ty) => holds_a_byte_address(ty.element_type(), through_pointer),
        _ => false,
    }
}

fn is_defined_function(callee: &CanonicalCallableRef) -> bool {
    callee.as_function().is_some_and(|function| !function.borrow().is_declaration())
}

fn is_byte_pointer(ty: &Type) -> bool {
    matches!(ty, Type::Ptr(pointer) if pointer.is_byte_pointer())
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use midenc_dialect_arith::ArithOpBuilder;
    use midenc_dialect_hir::HirOpBuilder;
    use midenc_hir::{
        ArrayType, Builder, CallConv, Ident, PointerType, SourceSpan, StructType, ValueRef,
        dialects::builtin::{BuiltinOpBuilder, FunctionBuilder, attributes::Signature},
        testing::Test,
    };

    use super::*;
    const SPAN: SourceSpan = SourceSpan::UNKNOWN;

    fn pointer(ty: Type, space: AddressSpace) -> Type {
        Type::from(PointerType::new_with_address_space(ty, space))
    }
    fn facts(test: &Test) -> ByteAddressing {
        UnitFacts::derive(test.module().borrow().as_operation(), &[]).byte_addressing
    }

    #[test]
    fn inline_assembly_hidden_writes_force_wide_but_empty_scalars_do_not() {
        for (body, expected) in [
            ("push.4294967296 mem_store.0", ByteAddressing::Wide),
            ("", ByteAddressing::Narrow),
        ] {
            let mut test = Test::named("inline_facts").in_module("m");
            test.with_function("f", &[], &[]);
            let signature = Signature::new(&test.context_rc(), [], []);
            let mut builder = test.function_builder();
            builder.inline_asm(body, signature, [], SPAN).unwrap();
            builder.ret(None, SPAN).unwrap();
            assert_eq!(facts(&test), expected);
        }
        let byte = pointer(Type::U32, AddressSpace::Byte);
        let mut test = Test::named("inline_typepun_facts").in_module("m");
        test.with_function("f", &[Type::Felt], &[]);
        let signature = Signature::new(&test.context_rc(), [Type::Felt], [byte]);
        let mut builder = test.function_builder();
        let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
        builder.inline_asm("", signature, [input], SPAN).unwrap();
        builder.ret(None, SPAN).unwrap();
        assert_eq!(facts(&test), ByteAddressing::Wide);
    }

    #[test]
    fn typed_pointer_loads_are_wide_makers_but_scalar_loads_are_not() {
        for (ty, expected) in [
            (Type::U32, ByteAddressing::Narrow),
            (Type::I32, ByteAddressing::Narrow),
            (pointer(Type::U32, AddressSpace::Byte), ByteAddressing::Wide),
            (
                pointer(pointer(Type::U32, AddressSpace::Byte), AddressSpace::Element),
                ByteAddressing::Wide,
            ),
        ] {
            for local in [false, true] {
                let mut test = Test::named("loaded_pointer_facts").in_module("m");
                test.with_function("f", &[pointer(ty.clone(), AddressSpace::Element)], &[]);
                test.function().borrow_mut().set_visibility(Visibility::Private);
                let local_var = test.function().borrow_mut().alloc_local(ty.clone());
                let mut builder = test.function_builder();
                let address = builder.entry_block().borrow().arguments()[0] as ValueRef;
                if local {
                    builder.load_local(local_var, SPAN).unwrap();
                } else {
                    builder.load(address, SPAN).unwrap();
                }
                builder.ret(None, SPAN).unwrap();
                assert_eq!(facts(&test), expected, "{ty}, local={local}");
            }
        }
    }

    #[test]
    fn public_same_context_boundaries_and_recursive_containment() {
        let byte = pointer(Type::U32, AddressSpace::Byte);
        let aggregate = Type::from(StructType::new([Type::U32, byte.clone()]));
        for (ty, carries) in [
            (Type::U32, false),
            (byte.clone(), true),
            (pointer(Type::U32, AddressSpace::Element), false),
            (pointer(aggregate.clone(), AddressSpace::Element), true),
            (Type::from(ArrayType::new(aggregate, 2)), true),
        ] {
            for cc in [
                CallConv::C,
                CallConv::Wasm,
                CallConv::Extern("sigma".into()),
                CallConv::ComponentModel,
            ] {
                for visibility in [Visibility::Private, Visibility::Internal, Visibility::Public] {
                    let mut test = Test::named("boundary").in_module("m");
                    let signature = Signature::with_convention(
                        &test.context_rc(),
                        cc.clone(),
                        [ty.clone()],
                        [],
                    );
                    let function = test
                        .builder_mut()
                        .create_function(Ident::from("f"), visibility, signature)
                        .unwrap();
                    FunctionBuilder::new(function, test.builder_mut()).ret([], SPAN).unwrap();
                    let expected = carries
                        && visibility == Visibility::Public
                        && cc != CallConv::ComponentModel;
                    assert_eq!(
                        facts(&test) == ByteAddressing::Wide,
                        expected,
                        "{ty}, {cc}, {visibility}"
                    );
                }
            }
        }
    }

    #[test]
    fn casts_and_gep_are_makers_only_when_they_expand_the_address_domain() {
        for (from, to, indices, wide) in [
            (Type::U32, pointer(Type::U32, AddressSpace::Byte), None, false),
            (Type::Felt, pointer(Type::U32, AddressSpace::Byte), None, true),
            (
                pointer(Type::U32, AddressSpace::Element),
                pointer(pointer(Type::U32, AddressSpace::Byte), AddressSpace::Element),
                None,
                true,
            ),
            (
                pointer(Type::U32, AddressSpace::Element),
                pointer(Type::U8, AddressSpace::Element),
                None,
                false,
            ),
            (
                pointer(Type::from(ArrayType::new(Type::U32, 2)), AddressSpace::Byte),
                Type::Never,
                Some(0),
                false,
            ),
            (
                pointer(Type::from(ArrayType::new(Type::U32, 2)), AddressSpace::Byte),
                Type::Never,
                Some(1),
                true,
            ),
            (
                pointer(Type::from(ArrayType::new(Type::U32, 2)), AddressSpace::Element),
                Type::Never,
                Some(1),
                false,
            ),
        ] {
            let mut test = Test::new("maker", core::slice::from_ref(&from), &[]);
            let mut builder = test.function_builder();
            let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let value = if let Some(index) = indices {
                builder.getelementptr(input, &[index], SPAN).unwrap()
            } else {
                builder.bitcast(input, to, SPAN).unwrap()
            };
            let op = value.borrow().get_defining_op().unwrap();
            assert_eq!(can_hold_a_wide_byte_address(&op.borrow()), wide, "{from}, {indices:?}");
        }
    }

    #[test]
    fn unknown_calls_and_context_returns_cross_unit_boundaries() {
        for defined in [false, true] {
            for context_call in [false, true] {
                for passes in [false, true] {
                    let mut test = Test::named("call_maker").in_module("m");
                    let byte = pointer(Type::U8, AddressSpace::Byte);
                    let params = if passes { vec![byte.clone()] } else { vec![] };
                    let results = if passes { vec![] } else { vec![byte.clone()] };
                    let signature = Signature::with_convention(
                        &test.context_rc(),
                        if context_call {
                            CallConv::ComponentModel
                        } else {
                            CallConv::C
                        },
                        params,
                        results,
                    );
                    let callee = test
                        .builder_mut()
                        .create_function(
                            Ident::from("callee"),
                            Visibility::Private,
                            signature.clone(),
                        )
                        .unwrap();
                    if defined {
                        let mut builder = FunctionBuilder::new(callee, test.builder_mut());
                        let returned = if passes {
                            vec![]
                        } else {
                            let zero = builder.u32(0, SPAN);
                            vec![builder.inttoptr(zero, byte.clone(), SPAN).unwrap()]
                        };
                        builder.ret(returned, SPAN).unwrap();
                    }
                    let module_body = test.module().borrow().body().entry_block_ref().unwrap();
                    test.builder_mut().set_insertion_point_to_end(module_body);
                    test.with_function("caller", &[Type::U32], &[]);
                    let mut builder = test.function_builder();
                    let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
                    let args: Vec<_> = if passes {
                        vec![builder.inttoptr(input, byte, SPAN).unwrap()]
                    } else {
                        vec![]
                    };
                    let call = if context_call {
                        builder.call(callee, signature, args, SPAN).unwrap().as_operation_ref()
                    } else {
                        builder.exec(callee, signature, args, SPAN).unwrap().as_operation_ref()
                    };
                    assert_eq!(
                        can_hold_a_wide_byte_address(&call.borrow()),
                        if context_call { !passes } else { !defined }
                    );
                }
            }
        }
    }

    #[test]
    fn pointer_calls_unknown_same_context_boundaries_and_context_results() {
        let byte = pointer(Type::U32, AddressSpace::Byte);
        let root = hir::DynExec::root_pointer_type();
        for context in [false, true] {
            for returns in [false, true] {
                let params = if returns { vec![] } else { vec![byte.clone()] };
                let results = if returns { vec![byte.clone()] } else { vec![] };
                let mut test = Test::new("pointer_facts", &[root.clone(), byte.clone()], &[]);
                let signature = Signature::with_convention(
                    &test.context_rc(),
                    if context {
                        CallConv::ComponentModel
                    } else {
                        CallConv::Extern("sigma".into())
                    },
                    params,
                    results,
                );
                let mut builder = test.function_builder();
                let pointer = builder.entry_block().borrow().arguments()[0] as ValueRef;
                let arg = builder.entry_block().borrow().arguments()[1] as ValueRef;
                let arguments = if returns { vec![] } else { vec![arg] };
                let op = if context {
                    builder
                        .dyncall_local(pointer, signature, arguments, SPAN)
                        .unwrap()
                        .as_operation_ref()
                } else {
                    builder.dynexec(pointer, signature, arguments, SPAN).unwrap().as_operation_ref()
                };
                assert_eq!(
                    UnitFacts::derive(&op.borrow(), &[]).byte_addressing,
                    if !context || returns {
                        ByteAddressing::Wide
                    } else {
                        ByteAddressing::Narrow
                    }
                );
            }
        }
    }

    #[test]
    fn casts_exposing_pointer_memory_are_makers_in_every_address_space() {
        let byte = pointer(Type::U32, AddressSpace::Byte);
        let mut missed = vec![];
        for destination in [AddressSpace::Byte, AddressSpace::Element] {
            for from in [
                Type::U32,
                pointer(Type::U32, AddressSpace::Byte),
                pointer(Type::U32, AddressSpace::Element),
            ] {
                let to = pointer(byte.clone(), destination);
                let mut test = Test::new("pointer_memory", core::slice::from_ref(&from), &[]);
                let mut builder = test.function_builder();
                let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
                let result = if from.is_pointer() {
                    builder.bitcast(input, to.clone(), SPAN).unwrap()
                } else {
                    builder.inttoptr(input, to.clone(), SPAN).unwrap()
                };
                let op = result.borrow().get_defining_op().unwrap();
                if UnitFacts::derive(&op.borrow(), &[]).byte_addressing != ByteAddressing::Wide {
                    missed.push(alloc::format!("{from} -> {to}"));
                }
            }
        }
        assert!(missed.is_empty(), "casts missed pointer memory exposure: {missed:?}");
    }

    #[test]
    fn pointer_casts_without_new_pointee_addresses_remain_narrow() {
        for source in [AddressSpace::Byte, AddressSpace::Element] {
            for destination in [AddressSpace::Byte, AddressSpace::Element] {
                for (from, to) in [
                    (Type::U32, Type::U8),
                    (pointer(Type::U32, AddressSpace::Byte), pointer(Type::U8, AddressSpace::Byte)),
                ] {
                    let from = pointer(from, source);
                    let to = pointer(to, destination);
                    let mut test = Test::new("pointer_alias", core::slice::from_ref(&from), &[]);
                    let mut builder = test.function_builder();
                    let input = builder.entry_block().borrow().arguments()[0] as ValueRef;
                    let result = builder.bitcast(input, to.clone(), SPAN).unwrap();
                    let op = result.borrow().get_defining_op().unwrap();
                    assert_eq!(
                        UnitFacts::derive(&op.borrow(), &[]).byte_addressing,
                        ByteAddressing::Narrow,
                        "{from} -> {to}"
                    );
                }
            }
        }
    }

    #[test]
    fn context_results_follow_nested_pointer_and_aggregate_containment() {
        let byte = pointer(Type::U32, AddressSpace::Byte);
        let mut missed = vec![];
        for (ty, expected) in [
            (pointer(byte.clone(), AddressSpace::Element), ByteAddressing::Wide),
            (Type::from(StructType::new([Type::U32, byte])), ByteAddressing::Wide),
            (pointer(Type::U32, AddressSpace::Element), ByteAddressing::Narrow),
            (Type::from(StructType::new([Type::U32, Type::U32])), ByteAddressing::Narrow),
        ] {
            let mut test = Test::named("context_result").in_module("m");
            let signature = Signature::with_convention(
                &test.context_rc(),
                CallConv::ComponentModel,
                [],
                [ty.clone()],
            );
            let callee = test
                .builder_mut()
                .create_function(Ident::from("callee"), Visibility::Private, signature.clone())
                .unwrap();
            let module_body = test.module().borrow().body().entry_block_ref().unwrap();
            test.builder_mut().set_insertion_point_to_end(module_body);
            test.with_function("caller", &[], &[]);
            assert_eq!(facts(&test), ByteAddressing::Narrow, "before returning {ty}");
            {
                let mut builder = test.function_builder();
                builder.call(callee, signature, [], SPAN).unwrap();
                builder.ret([], SPAN).unwrap();
            }
            if facts(&test) != expected {
                missed.push(ty);
            }
            let env = crate::select::SelectEnv {
                byte_addressing: expected,
                ..Default::default()
            };
            crate::select::select_function(&test.function().borrow(), &env).unwrap();
        }
        assert!(missed.is_empty(), "context results missed recursive containment: {missed:?}");
    }

    #[test]
    fn malformed_geps_force_wide_before_selection_diagnoses_them() {
        let mut missed = vec![];
        for mutation in 0..4 {
            let array = Type::from(ArrayType::new(Type::U8, 2));
            let mut test = Test::new("invalid_gep", &[pointer(array, AddressSpace::Element)], &[]);
            let mut builder = test.function_builder();
            let mut input = builder.entry_block().borrow().arguments()[0] as ValueRef;
            let mut result = builder.getelementptr(input, &[mutation % 2], SPAN).unwrap();
            match mutation {
                0 => input.borrow_mut().set_type(Type::U32),
                // Indexing a scalar is invalid even when the numeric offset would be zero.
                1 => input.borrow_mut().set_type(pointer(Type::U32, AddressSpace::Element)),
                2 => result.borrow_mut().set_type(Type::U32),
                // The remaining path has a byte offset indivisible by four.
                _ => {}
            }
            let op = result.borrow().get_defining_op().unwrap();
            if UnitFacts::derive(&op.borrow(), &[]).byte_addressing != ByteAddressing::Wide {
                missed.push(mutation);
            }
        }
        assert!(missed.is_empty(), "malformed GEPs missed conservative widening: {missed:?}");
    }

    #[test]
    fn supporting_modules_contribute_address_makers() {
        let root = Test::named("root").in_module("root");
        let mut support = Test::named("support").in_module("support");
        support.with_function("f", &[], &[]);
        let mut builder = support.function_builder();
        builder.alloca(Type::U32, SPAN).unwrap();
        builder.ret([], SPAN).unwrap();
        assert_eq!(facts(&root), ByteAddressing::Narrow);
        assert_eq!(
            UnitFacts::derive(root.module().borrow().as_operation(), &[support.module()])
                .byte_addressing,
            ByteAddressing::Wide
        );
    }
}
