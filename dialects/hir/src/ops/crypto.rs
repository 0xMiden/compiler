use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::StringAttr,
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

macro_rules! infer_felt_results {
    ($Op:ty, $($result:ident),+ $(,)?) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                $(self.$result().set_type(Type::Felt);)+
                Ok(())
            }
        }
    };
}

/// Compute the Poseidon2 hash of a word.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Hash {
    #[operand]
    input0: IntFelt,
    #[operand]
    input1: IntFelt,
    #[operand]
    input2: IntFelt,
    #[operand]
    input3: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
}

infer_felt_results!(Hash, result0_mut, result1_mut, result2_mut, result3_mut);

/// Compute the Poseidon2 merge hash of two words.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct HMerge {
    #[operand]
    lhs0: IntFelt,
    #[operand]
    lhs1: IntFelt,
    #[operand]
    lhs2: IntFelt,
    #[operand]
    lhs3: IntFelt,
    #[operand]
    rhs0: IntFelt,
    #[operand]
    rhs1: IntFelt,
    #[operand]
    rhs2: IntFelt,
    #[operand]
    rhs3: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
}

infer_felt_results!(HMerge, result0_mut, result1_mut, result2_mut, result3_mut);

/// Apply the Poseidon2 permutation to the top three VM stack words.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct HPerm {
    #[operand]
    state0: IntFelt,
    #[operand]
    state1: IntFelt,
    #[operand]
    state2: IntFelt,
    #[operand]
    state3: IntFelt,
    #[operand]
    state4: IntFelt,
    #[operand]
    state5: IntFelt,
    #[operand]
    state6: IntFelt,
    #[operand]
    state7: IntFelt,
    #[operand]
    state8: IntFelt,
    #[operand]
    state9: IntFelt,
    #[operand]
    state10: IntFelt,
    #[operand]
    state11: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
    #[result]
    result2: IntFelt,
    #[result]
    result3: IntFelt,
    #[result]
    result4: IntFelt,
    #[result]
    result5: IntFelt,
    #[result]
    result6: IntFelt,
    #[result]
    result7: IntFelt,
    #[result]
    result8: IntFelt,
    #[result]
    result9: IntFelt,
    #[result]
    result10: IntFelt,
    #[result]
    result11: IntFelt,
}

infer_felt_results!(
    HPerm,
    result0_mut,
    result1_mut,
    result2_mut,
    result3_mut,
    result4_mut,
    result5_mut,
    result6_mut,
    result7_mut,
    result8_mut,
    result9_mut,
    result10_mut,
    result11_mut,
);

macro_rules! infer_felt_outputs {
    ($Op:ty, $name:literal, $input_count:literal, $output_count:literal) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
                if self.stack().len() != $input_count {
                    return Err(Report::msg(format!(
                        "invalid {}: expected {} operand(s), but got {}",
                        $name,
                        $input_count,
                        self.stack().len()
                    )));
                }

                if !self.op.results.is_empty() && self.op.results.len() != $output_count {
                    return Err(Report::msg(format!(
                        "invalid {}: expected {} result(s), but got {}",
                        $name,
                        $output_count,
                        self.op.results.len()
                    )));
                }

                if self.op.results.is_empty() {
                    let span = self.span();
                    let owner = self.as_operation_ref();
                    for i in 0..$output_count {
                        let value = context.make_result(span, Type::Felt, owner, i as u8);
                        self.op.results.push(value);
                    }
                } else {
                    for result in self.op.results.iter_mut() {
                        result.borrow_mut().set_type(Type::Felt);
                    }
                }

                Ok(())
            }
        }
    };
}

/// Read a Merkle tree node from the advice provider and verify it against a root.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MTreeGet {
    #[operands]
    stack: IntFelt,
    #[results]
    outputs: IntFelt,
}

infer_felt_outputs!(MTreeGet, "hir.mtree_get", 6, 8);

/// Update a Merkle tree node, producing the old node value and new root.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MTreeSet {
    #[operands]
    stack: IntFelt,
    #[results]
    outputs: IntFelt,
}

infer_felt_outputs!(MTreeSet, "hir.mtree_set", 10, 8);

/// Merge two Merkle tree roots in the advice provider and return the merged root.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MTreeMerge {
    #[operands]
    stack: IntFelt,
    #[results]
    outputs: IntFelt,
}

infer_felt_outputs!(MTreeMerge, "hir.mtree_merge", 8, 4);

/// Verify a Merkle path for a node/root pair.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
#[effects(MemoryEffect(MemoryEffect::Read, MemoryEffect::Write))]
pub struct MTreeVerify {
    #[operands]
    stack: IntFelt,
    #[attr]
    #[default]
    message: StringAttr,
    #[results]
    outputs: IntFelt,
}

infer_felt_outputs!(MTreeVerify, "hir.mtree_verify", 10, 10);
