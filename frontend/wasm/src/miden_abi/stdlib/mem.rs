use midenc_hir::{
    CallConv, FunctionType, SmallVec, SymbolNameComponent, SymbolPath,
    Type::*,
    effects::{AdviceEffect, AdviceMapResource, AdviceStackResource, MemoryEffect},
    interner::{Symbol, symbols},
    smallvec,
};

use crate::{
    intrinsics::IntrinsicEffect,
    miden_abi::{FunctionTypeMap, ModuleFunctionTypeMap},
};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Miden),
    SymbolNameComponent::Component(symbols::Core),
    SymbolNameComponent::Component(symbols::Mem),
];

pub(crate) const PIPE_WORDS_TO_MEMORY: &str = "pipe_words_to_memory";
pub(crate) const PIPE_DOUBLE_WORDS_TO_MEMORY: &str = "pipe_double_words_to_memory";
pub(crate) const PIPE_PREIMAGE_TO_MEMORY: &str = "pipe_preimage_to_memory";

pub(crate) fn signatures() -> ModuleFunctionTypeMap {
    let mut m: ModuleFunctionTypeMap = Default::default();
    let mut funcs: FunctionTypeMap = Default::default();
    funcs.insert(
        Symbol::from(PIPE_WORDS_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // num_words
                I32,  // write_ptr
            ],
            [
                Felt, Felt, Felt, Felt, // R0
                Felt, Felt, Felt, Felt, // R1 (digest)
                Felt, Felt, Felt, Felt, // C (capacity)
                I32,  // write_ptr'
            ],
        ),
    );
    funcs.insert(
        Symbol::from(PIPE_DOUBLE_WORDS_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, Felt, Felt, Felt, // R0
                Felt, Felt, Felt, Felt, // R1 (digest)
                Felt, Felt, Felt, Felt, // C (capacity)
                I32,  // write_ptr
                I32,  // end_ptr
            ],
            [
                Felt, Felt, Felt, Felt, // R0'
                Felt, Felt, Felt, Felt, // R1' (digest)
                Felt, Felt, Felt, Felt, // C' (capacity)
                I32,  // write_ptr
            ],
        ),
    );
    funcs.insert(
        Symbol::from(PIPE_PREIMAGE_TO_MEMORY),
        FunctionType::new(
            CallConv::Wasm,
            [
                Felt, // num_words
                I32,  // write_ptr
                Felt, Felt, Felt, Felt, // COM (commitment)
            ],
            [
                I32, // write_ptr'
            ],
        ),
    );
    m.insert(SymbolPath::from_iter(MODULE_PREFIX.iter().copied()), funcs);
    m
}

pub(crate) fn function_effects(function: Symbol) -> Option<SmallVec<[IntrinsicEffect; 2]>> {
    let memory_write = || IntrinsicEffect::Memory {
        effect: MemoryEffect::Write,
        result: None,
        argument: None,
    };

    match function.as_str() {
        PIPE_WORDS_TO_MEMORY | PIPE_DOUBLE_WORDS_TO_MEMORY => Some(smallvec![
            IntrinsicEffect::Advice {
                effect: AdviceEffect::Read,
                resource: Box::new(AdviceStackResource),
                result: None,
                argument: None,
            },
            memory_write(),
        ]),
        PIPE_PREIMAGE_TO_MEMORY => Some(smallvec![
            IntrinsicEffect::Advice {
                effect: AdviceEffect::Read,
                resource: Box::new(AdviceMapResource),
                result: None,
                argument: None,
            },
            memory_write(),
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_words_to_memory_declares_advice_effects() {
        let effects = function_effects(Symbol::from(PIPE_WORDS_TO_MEMORY))
            .expect("pipe_words_to_memory should have stdlib effects");

        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                IntrinsicEffect::Advice {
                    effect: AdviceEffect::Read,
                    ..
                }
            )
        }));
    }
}
