//! Support for the Wasm component model translation
//!
//! This module contains all of the internal type definitions to parse and
//! translate the component model.

pub(crate) mod build_ir;
mod canon_abi_utils;
mod flat;
mod lift_exports;
pub(crate) mod lower_imports;
mod parser;
mod shim_bypass;
mod translator;
mod types;

pub use self::{parser::*, types::*};

#[cfg(test)]
pub(super) mod test_support {
    //! Test helpers for synthetic component CanonABI wrapper fixtures.

    use alloc::sync::Arc;

    use midenc_dialect_cf as cf;
    use midenc_dialect_ub as ub;
    use midenc_hir::{
        EnumType, Op, Operation, StructType, SymbolName, SymbolTable, Type, Variant, WalkResult,
        dialects::builtin::{ComponentBuilder, Function, FunctionRef},
    };

    use super::{
        CanonicalAbiField, CanonicalAbiInfo, CanonicalAbiType, CanonicalAbiTypeKind, VariantInfo,
    };

    /// Builds canonical ABI metadata for a two-case unit-only variant.
    pub fn unit_only_variant_type() -> CanonicalAbiType {
        let cases = [None, None];
        let info = VariantInfo::new_static(&cases);
        let abi = CanonicalAbiInfo::variant_static(&cases);
        let ir = Type::Enum(Arc::new(
            EnumType::new(
                "unit-only".into(),
                Type::U8,
                [
                    Variant::c_like("first".into(), Some(0)),
                    Variant::c_like("second".into(), Some(1)),
                ],
            )
            .expect("unit-only enum should be valid"),
        ));

        CanonicalAbiType {
            ir,
            abi,
            kind: CanonicalAbiTypeKind::Variant {
                discriminant: Box::new(CanonicalAbiType {
                    ir: Type::U8,
                    abi: CanonicalAbiInfo::SCALAR1,
                    kind: CanonicalAbiTypeKind::Scalar,
                }),
                payload_offset32: info.payload_offset32,
                cases: Box::new([None, None]),
                payload_flat_types: Box::new([]),
            },
        }
    }

    /// Builds canonical ABI metadata for a two-case variant with scalar payloads.
    pub fn scalar_payload_variant_type() -> CanonicalAbiType {
        let payload_ty = CanonicalAbiType {
            ir: Type::I32,
            abi: CanonicalAbiInfo::SCALAR4,
            kind: CanonicalAbiTypeKind::Scalar,
        };
        let case_abis = [Some(CanonicalAbiInfo::SCALAR4), Some(CanonicalAbiInfo::SCALAR4)];
        let info = VariantInfo::new_static(&case_abis);
        let abi = CanonicalAbiInfo::variant_static(&case_abis);
        let ir = Type::Enum(Arc::new(
            EnumType::new(
                "scalar-payload".into(),
                Type::U8,
                [
                    Variant::new("first".into(), Type::I32, Some(0)),
                    Variant::new("second".into(), Type::I32, Some(1)),
                ],
            )
            .expect("scalar-payload enum should be valid"),
        ));

        CanonicalAbiType {
            ir,
            abi,
            kind: CanonicalAbiTypeKind::Variant {
                discriminant: Box::new(CanonicalAbiType {
                    ir: Type::U8,
                    abi: CanonicalAbiInfo::SCALAR1,
                    kind: CanonicalAbiTypeKind::Scalar,
                }),
                payload_offset32: info.payload_offset32,
                cases: Box::new([Some(payload_ty.clone()), Some(payload_ty)]),
                payload_flat_types: Box::new([Type::I32]),
            },
        }
    }

    /// Builds canonical ABI metadata for a two-field record result.
    pub fn two_field_record_type() -> CanonicalAbiType {
        let field_ty = CanonicalAbiType {
            ir: Type::U32,
            abi: CanonicalAbiInfo::SCALAR4,
            kind: CanonicalAbiTypeKind::Scalar,
        };
        let mut offset = 0;
        let first_offset = field_ty.abi.next_field32(&mut offset);
        let second_offset = field_ty.abi.next_field32(&mut offset);
        let abi = CanonicalAbiInfo::record([&field_ty.abi, &field_ty.abi].into_iter());
        let ir = Type::from(StructType::new(vec![Type::U32, Type::U32]));

        CanonicalAbiType {
            ir,
            abi,
            kind: CanonicalAbiTypeKind::Record {
                fields: Box::new([
                    CanonicalAbiField {
                        offset32: first_offset,
                        ty: field_ty.clone(),
                    },
                    CanonicalAbiField {
                        offset32: second_offset,
                        ty: field_ty,
                    },
                ]),
            },
        }
    }

    /// Counts generated variant-validation control-flow operations in `function`.
    pub fn count_validation_ops(function: FunctionRef) -> (usize, usize) {
        let mut switch_count = 0;
        let mut unreachable_count = 0;
        function
            .borrow()
            .as_operation()
            .prewalk(|op: &Operation| {
                if op.is::<cf::Switch>() {
                    switch_count += 1;
                }
                if op.is::<ub::Unreachable>() {
                    unreachable_count += 1;
                }
                WalkResult::<()>::Continue(())
            })
            .into_result()
            .expect("operation walk should not fail");

        (switch_count, unreachable_count)
    }

    /// Looks up a generated component function by name.
    pub fn component_function(component_builder: &ComponentBuilder, name: &str) -> FunctionRef {
        let symbol = SymbolName::intern(name);
        let symbol_ref = component_builder
            .component
            .borrow()
            .get(symbol)
            .expect("expected component function");
        let op = symbol_ref.borrow();
        op.as_symbol_operation()
            .downcast_ref::<Function>()
            .expect("expected symbol to be a function")
            .as_function_ref()
    }
}
