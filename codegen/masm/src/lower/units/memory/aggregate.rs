//! Aggregate memory transfers use the same represented lanes for pointers and typed locals.
use alloc::{rc::Rc, sync::Arc};

use midenc_hir::{Context, SourceSpan, Type};

use super::*;
use crate::{
    OperandStack,
    emit::{OpEmitter, addressing::ByteAddressing},
};

pub(super) enum Address {
    Local(u32),
    Element,
    Byte(ByteAddressing),
}

impl Memory {
    pub(super) fn aggregate_access(
        context: Rc<Context>,
        layout: &ValueLayout,
        address: Address,
        store: bool,
        span: SourceSpan,
    ) -> Result<Self, Report> {
        use masm::Instruction as I;

        // A Felt cannot be split into byte fragments without losing its upper field bits.
        let has_felt = validate_felt_alignment(layout.ty(), 0)?;
        let mut body = raw::Builder::new(span);
        let mut local = None;
        let byte = match address {
            Address::Local(index) => {
                local = Some(index);
                body.emit(I::Locaddr(0u16.into()));
                false
            }
            Address::Element => false,
            Address::Byte(addressing) => {
                body.emit_all(addressing.native_ptr());
                if has_felt {
                    body.emit_all([
                        I::Swap1,
                        I::AssertzWithError(
                            Arc::<str>::from(
                                "aggregate Felt access requires an element-aligned pointer",
                            )
                            .into(),
                        ),
                    ]);
                    false
                } else {
                    true
                }
            }
        };
        let parts = layout.raw_parts();
        let mut proof = UnitEvidence::default();
        let mut argument_lanes = vec![];
        let mut next_control = 0;
        for step in 0..parts.len() {
            let lane = if store { step } else { parts.len() - 1 - step };
            let last = step + 1 == parts.len();
            if !last {
                if byte {
                    if store {
                        // [addr, offset, value, ...] -> [addr, offset, value, addr, offset, ...]
                        body.emit_all([I::MovUp2, I::Dup2, I::Dup2]);
                    } else {
                        body.emit_all([I::Dup1, I::Dup1]);
                    }
                } else {
                    body.emit(I::Dup0);
                    if store {
                        body.emit_all([I::MovUp2, I::Swap1]);
                    }
                }
            }
            // Every represented lane retains its original four-byte interval. Skipped padding
            // never changes the offsets of subsequent lanes.
            if lane != 0 {
                body.emit(I::AddImm(midenc_hir::Felt::from(lane as u32).into()));
            }
            let part = &parts[lane];
            if byte {
                if store && part.size_in_bits() < 32 {
                    // Aggregate padding bits need not have the canonical scalar representation.
                    body.emit(I::MovUp2);
                    mask(&mut body, part.size_in_bits());
                    body.emit(I::MovDn2);
                }
                let mut stack = OperandStack::new(context.clone());
                let mut invoked = Default::default();
                let mut emitter = OpEmitter::new(&mut invoked, &mut body.ops, &mut stack);
                if store {
                    emitter.store_native(part, span);
                } else {
                    emitter.load_native(part, span);
                }
                if store && part.size_in_bits() == 32 {
                    // Whole lanes reach store_sw unchanged, after the normalized pointer pair.
                    argument_lanes.push((proof.calls.len(), 2, lane + 1));
                }
                proof.calls.extend(access_contract(part, store, false, &[], &mut next_control)?);
            } else {
                aligned_lane(&mut body, part.size_in_bits(), store);
            }
            if !store && !last {
                body.emit(if byte { I::MovDn2 } else { I::Swap1 });
            }
        }
        check_construction(&body.ops, &proof)?;
        Ok(Self {
            body: body.ops,
            local,
            proof,
            effects: LoweringEffects {
                memory: if !store {
                    SourceMemoryEffects::READ
                } else if byte || parts.iter().any(|part| part.size_in_bits() < 32) {
                    SourceMemoryEffects::READ_WRITE
                } else {
                    SourceMemoryEffects::WRITE
                },
                may_trap: true,
                normal_return: true,
            },
            argument_lanes,
            direct_results: None,
        })
    }
}

fn mask(body: &mut raw::Builder, bits: usize) {
    body.emit(masm::Instruction::U32Cast);
    body.emit_push((1u32 << bits) - 1);
    body.emit(masm::Instruction::U32And);
}

/// Transfer a lane at an element address, preserving the other bytes of a partial cell.
fn aligned_lane(body: &mut raw::Builder, bits: usize, store: bool) {
    use masm::Instruction as I;
    if store {
        if bits < 32 {
            body.emit_all([I::Dup0, I::MemLoad, I::U32Cast]);
            body.emit_push(!((1u32 << bits) - 1));
            body.emit_all([I::U32And, I::MovUp2]);
            mask(body, bits);
            body.emit_all([I::U32Or, I::Swap1]);
        }
        body.emit(I::MemStore);
    } else {
        body.emit(I::MemLoad);
        if bits < 32 {
            mask(body, bits);
        }
    }
}

/// Inspect original field offsets, rather than fragments which may have erased a split Felt.
/// The enclosing ValueLayout has already bounded the storage to sixteen bytes.
fn validate_felt_alignment(ty: &Type, offset: usize) -> Result<bool, Report> {
    // Zero-sized subtrees contain no stored Felts. Their array lengths are not bounded by
    // the enclosing layout's storage size, so do not descend into them.
    if ty.size_in_bytes() == 0 {
        return Ok(false);
    }
    let mut found = false;
    match ty {
        Type::Felt => {
            if !offset.is_multiple_of(4) {
                return Err(invalid("aggregate memory layout splits a Felt across cells").into());
            }
            found = true;
        }
        Type::Struct(structure) => {
            for field in structure.get().fields() {
                found |= validate_felt_alignment(&field.ty, offset + field.offset as usize)?;
            }
        }
        Type::Array(array) => {
            let element = array.element_type();
            let stride = element.size_in_bytes().next_multiple_of(element.min_alignment());
            for index in 0..array.len() {
                found |= validate_felt_alignment(element, offset + index * stride)?;
            }
        }
        Type::Enum(enumeration) => {
            for (variant_offset, variant) in enumeration.get().variant_offsets() {
                if let Some(ty) = &variant.value {
                    found |= validate_felt_alignment(ty, offset + variant_offset as usize)?;
                }
            }
        }
        _ => {}
    }
    Ok(found)
}
