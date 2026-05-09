use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
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
