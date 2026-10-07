//! Effects the compiler knows core-library procedures to have.
//!
//! A package manifest declares no effects (spec §7), and the compiler never trusts an external
//! declaration of them; these six are the compiler's own knowledge of the toolchain's core
//! library, in the same sense as the intrinsics' effects, and the advice-taint lint (`-Zlint`)
//! depends on them.

use midenc_hir::{
    SmallVec,
    effects::{AdviceEffect, AdviceMapResource, AdviceStackResource, MemoryEffect},
    smallvec,
};
use midenc_session::miden_assembly_syntax::ast::Path;

use crate::intrinsics::IntrinsicEffect;

/// The memory and advice effects of the procedure at `path`: those of the six
/// `::miden::core::mem::pipe_*` procedures, and none for any other path.
///
/// The linker-stub lowering attaches them to the declaration of every callee it resolves from a
/// linked package, and the advice-taint lint (`-Zlint`) reads them off that declaration to see
/// that a value piped out of the advice provider is unconstrained. Three are the effects the
/// deleted hand table (`miden_abi::stdlib::mem::function_effects`) attached; the other three are
/// the procedures core 0.35 added beside them, which the generated `miden-stdlib-sys` now
/// exposes: the `_in_domain` variant pipes from the advice stack as `pipe_words_to_memory` does,
/// and the two `_preimage_` variants pipe a map entry's preimage as `pipe_preimage_to_memory`
/// does.
///
/// `path` is the callee's MASM path, e.g. `::miden::core::mem::pipe_words_to_memory`; a leading
/// `::` is ignored the same way [`PackageInterface::procedure`](
/// midenc_package_interface::PackageInterface::procedure) ignores it when resolving, so both an
/// absolute and a relative spelling of a matching path are recognized. Any other path yields no
/// effects.
pub(crate) fn known_effects(path: &Path) -> SmallVec<[IntrinsicEffect; 2]> {
    let memory_write = || IntrinsicEffect::Memory {
        effect: MemoryEffect::Write,
        result: None,
        argument: None,
    };
    let advice_read = |resource: Box<dyn midenc_hir::effects::Resource>| IntrinsicEffect::Advice {
        effect: AdviceEffect::Read,
        resource,
        result: None,
        argument: None,
    };
    match path.to_relative().to_string().as_str() {
        "miden::core::mem::pipe_words_to_memory"
        | "miden::core::mem::pipe_words_to_memory_in_domain"
        | "miden::core::mem::pipe_double_words_to_memory" => {
            smallvec![advice_read(Box::new(AdviceStackResource)), memory_write()]
        }
        "miden::core::mem::pipe_preimage_to_memory"
        | "miden::core::mem::pipe_double_words_preimage_to_memory"
        | "miden::core::mem::pipe_double_words_preimage_to_memory_with_domain" => {
            smallvec![advice_read(Box::new(AdviceMapResource)), memory_write()]
        }
        _ => smallvec![],
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use midenc_hir::{FunctionIdent, SymbolPath};

    use super::*;

    /// The three `mem` procedures keep the effects the deleted hand table attached, their three
    /// core 0.35 siblings get the same, and nothing else gets any: see [`known_effects`].
    #[test]
    fn the_mem_procedures_keep_the_effects_the_hand_table_attached() {
        for (name, resource) in [
            ("pipe_words_to_memory", "advice-stack"),
            ("pipe_words_to_memory_in_domain", "advice-stack"),
            ("pipe_double_words_to_memory", "advice-stack"),
            ("pipe_preimage_to_memory", "advice-map"),
            ("pipe_double_words_preimage_to_memory", "advice-map"),
            ("pipe_double_words_preimage_to_memory_with_domain", "advice-map"),
        ] {
            let path = alloc::format!("::miden::core::mem::{name}");
            let effects = known_effects(Path::new(&path));
            assert_eq!(effects.len(), 2, "{name}");
            match &effects[0] {
                IntrinsicEffect::Advice {
                    effect,
                    resource: declared,
                    ..
                } => {
                    assert_eq!(*effect, AdviceEffect::Read, "{name}");
                    assert_eq!(declared.name(), resource, "{name}");
                }
                _ => panic!("{name}: expected an advice read as the first effect"),
            }
            assert!(
                matches!(
                    &effects[1],
                    IntrinsicEffect::Memory {
                        effect: MemoryEffect::Write,
                        ..
                    }
                ),
                "{name}"
            );
        }

        // Everything else carries no effects.
        assert!(known_effects(Path::new("::miden::core::mem::pipe_words_to_memory_v2")).is_empty());
        assert!(known_effects(Path::new("::miden::core::collections::smt::get")).is_empty());
        assert!(known_effects(Path::new("::miden::protocol::tx::get_block_timestamp")).is_empty());
    }

    /// The real lowering renders a callee path through
    /// [`SymbolPath::to_library_path`](midenc_hir::SymbolPath::to_library_path), which keeps the
    /// leading `::`; `known_effects` must recognize that rendering, not just the relative spelling
    /// used directly above.
    #[test]
    fn effects_recognizes_the_path_rendering_the_real_lowering_uses() {
        let id = FunctionIdent::from_str("miden::core::mem::pipe_words_to_memory").unwrap();
        let library_path = SymbolPath::from_masm_function_id(id).to_library_path();
        assert_eq!(known_effects(&library_path).len(), 2);
    }
}
